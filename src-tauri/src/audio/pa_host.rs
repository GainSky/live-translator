//! Linux PulseAudio / pipewire-pulse 兼容后端（feature = `audio-pulseaudio`）
//!
//! 经 cpal 的 pulseaudio host 枚举：monitor 设备（sink 监听源）会以普通 source 形式
//! 出现（如 `alsa_output.pci-...analog-stereo.monitor`），kind 归类为 Monitor。
//! 环回语义与原生 PipeWire 路径一致，作为其回退方案。

use cpal::traits::{DeviceTrait, HostTrait};

use super::{classify, AudioDeviceDescriptor, DeviceKind, OpenedSource, SourceDevice};
use crate::error::{AppError, AppResult};

pub const ID_PREFIX: &str = "pa:";

fn host() -> AppResult<cpal::Host> {
    cpal::host_from_id(cpal::HostId::PulseAudio)
        .map_err(|e| AppError::Message(format!("PulseAudio host 不可用: {e}")))
}

fn describe(device: &cpal::Device, is_default: bool) -> Option<AudioDeviceDescriptor> {
    let desc = device.description().ok()?;
    if !device.supports_input() {
        return None;
    }
    let name = desc.name().to_string();
    let kind = if name.ends_with(".monitor") {
        DeviceKind::Monitor
    } else {
        classify(&desc)
    };
    let (sample_rate, channels) = device
        .default_input_config()
        .map(|c| (c.sample_rate(), c.channels()))
        .unwrap_or((0, 0));
    Some(AudioDeviceDescriptor {
        id: format!("{ID_PREFIX}{name}"),
        name,
        kind,
        sample_rate,
        channels,
        is_default,
        signal: 0.0,
    })
}

pub fn enumerate() -> AppResult<Vec<AudioDeviceDescriptor>> {
    let host = host()?;
    let mut out = Vec::new();
    for device in host
        .devices()
        .map_err(|e| AppError::Message(format!("PulseAudio 设备枚举失败: {e}")))?
    {
        if let Some(d) = describe(&device, false) {
            out.push(d);
        }
    }
    Ok(out)
}

pub fn open_default() -> AppResult<OpenedSource> {
    let device = host()?
        .default_input_device()
        .ok_or_else(|| AppError::Message("PulseAudio 无默认输入设备".into()))?;
    let desc = describe(&device, true)
        .ok_or_else(|| AppError::Message("默认输入设备不支持采集".into()))?;
    Ok(OpenedSource { desc, device: SourceDevice::Cpal(device) })
}

pub fn open(id_suffix: &str) -> AppResult<OpenedSource> {
    for device in host()?
        .devices()
        .map_err(|e| AppError::Message(format!("PulseAudio 设备枚举失败: {e}")))?
    {
        if let Some(desc) = describe(&device, false) {
            if desc.name == id_suffix {
                return Ok(OpenedSource { desc, device: SourceDevice::Cpal(device) });
            }
        }
    }
    Err(AppError::Message(format!("未找到 PulseAudio 设备: {id_suffix}")))
}

#[allow(unused)]
fn _kind_guard(_k: DeviceKind) {}
