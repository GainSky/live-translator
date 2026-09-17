//! Linux 原生 PipeWire 后端（feature = `audio-pipewire`）
//!
//! cpal 0.18 的 PipeWire host（HostId::PipeWire）：
//! - `Audio/Source` 节点 → 麦克风
//! - `Audio/Sink` 节点 → 暴露为可输入设备（Duplex），打开输入流时自动携带
//!   `STREAM_CAPTURE_SINK=true`，捕获该 sink 正在播放的音频（系统声音环回）
//! - `Stream/Output` 节点 → 应用播放流（如 Firefox 每个标签页一条流）
//! 另有合成设备：`input_default`（默认麦克风）/ `sink_default`（默认输出环回）。
//!
//! 设备 id：PipeWire 节点名（`DeviceId::id()`）+ 应用流序号。
//! - 同一声卡的输入/输出节点描述名相同（如"K66 模拟立体声"），但节点名唯一
//!   （alsa_input.xxx / alsa_output.xxx），否则按名匹配会撞车
//! - 同一应用的多个播放流节点名相同（多个 Firefox 标签页都叫 "Firefox"），
//!   cpal 的 DeviceId 不含 object.serial 无法区分 → 附加 `#序号`
//!   （enumerate 顺序内第 N 个同名流）。序号在流增删后会移位，因此刷新设备
//!   后需重新勾选；对「枚举 → 勾选 → 启动」的同进程使用流是正确的。

use std::collections::HashMap;

use cpal::traits::{DeviceTrait, HostTrait};

use super::{AudioDeviceDescriptor, DeviceKind, OpenedSource, SourceDevice};
use crate::error::{AppError, AppResult};

pub const ID_PREFIX: &str = "pw:";

fn host() -> AppResult<cpal::Host> {
    cpal::host_from_id(cpal::HostId::PipeWire)
        .map_err(|e| AppError::Message(format!("PipeWire host 不可用: {e}")))
}

fn node_id(device: &cpal::Device) -> Option<String> {
    device.id().ok().map(|i| i.id().to_string())
}

fn describe(
    device: &cpal::Device,
    is_default: bool,
    app_seq: Option<usize>,
) -> Option<AudioDeviceDescriptor> {
    let desc = device.description().ok()?;
    let node = node_id(device).unwrap_or_else(|| desc.name().to_string());
    let name = {
        let n = desc.name();
        // 应用流节点常无 node.description（显示 unknown/空）→ 回退节点名（如 "Firefox"）
        if n.is_empty() || n == "unknown" {
            node.clone()
        } else {
            n.to_string()
        }
    };
    let (sample_rate, channels) = match device.default_input_config() {
        Ok(c) => (c.sample_rate(), c.channels()),
        // 应用流（StreamOutput）方向为 Output：input_config 会拒绝，但 output_config
        // 返回同样的节点参数（cpal PW 后端两方向共用 channels/rate 字段）
        Err(_) => device
            .default_output_config()
            .map(|c| (c.sample_rate(), c.channels()))
            .unwrap_or((0, 0)),
    };
    // 分类（结合 device_type / direction / 节点名前缀）：
    // - Microphone/Headset 图标 → 麦克风；Speaker/Headphones → sink 环回
    // - direction Output：output_default 合成设备 → 环回；其余为应用播放流
    // - direction Duplex → sink 节点（STREAM_CAPTURE_SINK 环回）
    // - direction Input：alsa_input/bluez 前缀或 input_default → 真麦克风；
    //   其余为应用录音流（StreamInput，无输出端口不可捕获）→ 排除
    let kind = match desc.device_type() {
        cpal::DeviceType::Microphone | cpal::DeviceType::Headset => DeviceKind::Microphone,
        cpal::DeviceType::Speaker | cpal::DeviceType::Headphones => DeviceKind::Loopback,
        _ => match desc.direction() {
            cpal::DeviceDirection::Output => {
                if node == "output_default" {
                    DeviceKind::Loopback
                } else {
                    DeviceKind::Application
                }
            }
            cpal::DeviceDirection::Duplex => DeviceKind::Loopback,
            cpal::DeviceDirection::Input => {
                if node == "input_default"
                    || node.starts_with("alsa_input")
                    || node.starts_with("bluez")
                {
                    DeviceKind::Microphone
                } else {
                    return None; // 应用录音流（StreamInput），不可捕获
                }
            }
            _ => DeviceKind::Unknown,
        },
    };
    // 应用流 id 带同名序号（Firefox#1 / Firefox#2），其余设备用节点名；
    // 显示名同步加序号，多条同名流（如多个 Firefox 标签页）一目了然
    let (id, name) = match app_seq {
        Some(seq) if kind == DeviceKind::Application => (
            format!("{ID_PREFIX}{node}#{seq}"),
            format!("{name}（流#{seq}）"),
        ),
        _ => (format!("{ID_PREFIX}{node}"), name),
    };
    Some(AudioDeviceDescriptor {
        id,
        name,
        kind,
        sample_rate,
        channels,
        is_default,
        signal: 0.0, // 探测后由 list 命令填充
    })
}

pub fn enumerate() -> AppResult<Vec<AudioDeviceDescriptor>> {
    let host = host()?;
    let default_in_name = host
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|d| d.name().to_string());

    let mut out = Vec::new();
    // 应用流同名计数（node 名 → 已见数量），用于生成稳定序号
    let mut app_seen: HashMap<String, usize> = HashMap::new();
    for device in host
        .devices()
        .map_err(|e| AppError::Message(format!("PipeWire 设备枚举失败: {e}")))?
    {
        let is_default = device
            .description()
            .map(|desc| default_in_name.as_deref() == Some(desc.name()))
            .unwrap_or(false);
        // 先粗判是否为应用流（Output 方向且非合成设备）以分配序号
        let app_seq = {
            let d = device.description().ok();
            let is_app = d
                .map(|dd| {
                    dd.direction() == cpal::DeviceDirection::Output
                        && node_id(&device).as_deref() != Some("output_default")
                })
                .unwrap_or(false);
            if is_app {
                let node = node_id(&device).unwrap_or_default();
                let n = app_seen.entry(node).or_insert(0);
                *n += 1;
                Some(*n)
            } else {
                None
            }
        };
        if let Some(d) = describe(&device, is_default, app_seq) {
            out.push(d);
        }
    }
    Ok(out)
}

/// 打开默认输入（合成 input_default 设备，跟随系统默认麦克风）
pub fn open_default() -> AppResult<OpenedSource> {
    let host = host()?;
    let device = host
        .default_input_device()
        .ok_or_else(|| AppError::Message("PipeWire 无默认输入设备".into()))?;
    let desc = describe(&device, true, None)
        .ok_or_else(|| AppError::Message("默认输入设备不支持采集".into()))?;
    Ok(OpenedSource { desc, device: SourceDevice::Cpal(device) })
}

/// 按节点名（应用流可带 #序号）打开设备
/// （sink 节点 → STREAM_CAPTURE_SINK 环回 / source 节点 → 麦克风 /
///  应用流 → TARGET_OBJECT 定向捕获）
pub fn open(id_suffix: &str) -> AppResult<OpenedSource> {
    // 解析 "节点名#N" → ("节点名", Some(N))；设备节点不含 '#' 原样匹配
    let (base, seq) = match id_suffix.rsplit_once('#') {
        Some((b, s)) if s.chars().all(|c| c.is_ascii_digit()) && !s.is_empty() => {
            (b.to_string(), s.parse::<usize>().ok())
        }
        _ => (id_suffix.to_string(), None),
    };

    let host = host()?;
    // 应用流按序号选第 N 个同名节点；普通设备按节点名精确匹配
    let mut app_index: HashMap<String, usize> = HashMap::new();
    for device in host
        .devices()
        .map_err(|e| AppError::Message(format!("PipeWire 设备枚举失败: {e}")))?
    {
        let node = match node_id(&device) {
            Some(n) => n,
            None => continue,
        };
        let is_app = device
            .description()
            .map(|d| {
                d.direction() == cpal::DeviceDirection::Output && node != "output_default"
            })
            .unwrap_or(false);
        if is_app {
            if node == base {
                let n = app_index.entry(node.clone()).or_insert(0);
                *n += 1;
                if seq == Some(*n) {
                    if let Some(desc) = describe(&device, false, Some(*n)) {
                        return Ok(OpenedSource { desc, device: SourceDevice::Cpal(device) });
                    }
                }
            }
        } else if node == base && seq.is_none() {
            if let Some(desc) = describe(&device, false, None) {
                return Ok(OpenedSource { desc, device: SourceDevice::Cpal(device) });
            }
        }
    }
    Err(AppError::Message(format!(
        "未找到 PipeWire 设备: {id_suffix}（应用流可能已关闭，请刷新设备列表）"
    )))
}

#[allow(unused)]
fn _kind_guard(_k: DeviceKind) {}
