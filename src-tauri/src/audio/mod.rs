pub mod capture;
pub mod cpal_mic;
pub mod probe;
pub mod resample;

#[cfg(all(target_os = "linux", feature = "audio-pipewire"))]
pub mod pw_host;
#[cfg(all(target_os = "linux", feature = "audio-pulseaudio"))]
pub mod pa_host;
#[cfg(target_os = "windows")]
pub mod wasapi_loopback;

use serde::Serialize;

use crate::error::AppResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DeviceKind {
    Microphone,
    /// Windows WASAPI Loopback / PipeWire sink（STREAM_CAPTURE_SINK）
    Loopback,
    /// PulseAudio/PipeWire-pulse monitor 设备
    Monitor,
    /// 应用播放流（PipeWire StreamOutput 节点，如 Firefox 的音频输出）
    Application,
    Virtual,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioDeviceDescriptor {
    pub id: String,
    pub name: String,
    pub kind: DeviceKind,
    pub sample_rate: u32,
    pub channels: u16,
    pub is_default: bool,
    /// 信号测试峰值（0~1，枚举时为 0，探测后由 list 命令填充）
    pub signal: f32,
}

/// 已解析的可采集源（描述符 + cpal 设备句柄）
pub struct OpenedSource {
    pub desc: AudioDeviceDescriptor,
    pub device: cpal::Device,
}

/// cpal 0.18 设备元数据 → 设备类型分类
pub(crate) fn classify(desc: &cpal::DeviceDescription) -> DeviceKind {
    match desc.device_type() {
        cpal::DeviceType::Microphone => DeviceKind::Microphone,
        cpal::DeviceType::Headset => DeviceKind::Microphone,
        cpal::DeviceType::Speaker | cpal::DeviceType::Headphones => DeviceKind::Loopback,
        _ => match desc.direction() {
            cpal::DeviceDirection::Input => DeviceKind::Microphone,
            _ => DeviceKind::Unknown,
        },
    }
}

/// 枚举全部音频输入源（麦克风 + 系统声音环回）。
///
/// 后端选择链（readme §2.1）：
/// - Linux: 原生 PipeWire（feature `audio-pipewire`）→ PulseAudio 兼容（feature
///   `audio-pulseaudio`，经 pipewire-pulse）→ cpal ALSA 兜底
/// - Windows: cpal（麦克风）+ WASAPI Loopback（系统声音）
pub fn enumerate_devices() -> AppResult<Vec<AudioDeviceDescriptor>> {
    #[cfg(target_os = "linux")]
    {
        #[cfg(feature = "audio-pipewire")]
        match pw_host::enumerate() {
            Ok(devs) if !devs.is_empty() => return Ok(devs),
            Ok(_) => tracing::warn!("PipeWire 枚举为空，回退下一后端"),
            Err(e) => tracing::warn!("PipeWire 枚举失败，回退下一后端: {e}"),
        }
        #[cfg(feature = "audio-pulseaudio")]
        match pa_host::enumerate() {
            Ok(devs) if !devs.is_empty() => return Ok(devs),
            Ok(_) => tracing::warn!("PulseAudio 枚举为空，回退 ALSA"),
            Err(e) => tracing::warn!("PulseAudio 枚举失败，回退 ALSA: {e}"),
        }
        cpal_mic::enumerate()
    }

    #[cfg(target_os = "windows")]
    {
        let mut devs = cpal_mic::enumerate()?;
        match wasapi_loopback::enumerate() {
            Ok(mut loopback) => devs.append(&mut loopback),
            Err(e) => tracing::warn!("WASAPI Loopback 枚举失败: {e}"),
        }
        Ok(devs)
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    cpal_mic::enumerate()
}

/// 解析设备 id → 可采集源（id 前缀路由到对应后端）
pub fn resolve_source(id: &str) -> AppResult<OpenedSource> {
    #[cfg(all(target_os = "linux", feature = "audio-pipewire"))]
    if let Some(suffix) = id.strip_prefix(pw_host::ID_PREFIX) {
        return pw_host::open(suffix);
    }
    #[cfg(all(target_os = "linux", feature = "audio-pulseaudio"))]
    if let Some(suffix) = id.strip_prefix(pa_host::ID_PREFIX) {
        return pa_host::open(suffix);
    }
    #[cfg(target_os = "windows")]
    if let Some(suffix) = id.strip_prefix(wasapi_loopback::ID_PREFIX) {
        return wasapi_loopback::open(suffix);
    }
    let suffix = id.strip_prefix(cpal_mic::ID_PREFIX).unwrap_or(id);
    cpal_mic::open(suffix)
}

/// 默认输入源（跟随系统默认麦克风）
pub fn default_source() -> AppResult<OpenedSource> {
    #[cfg(all(target_os = "linux", feature = "audio-pipewire"))]
    if let Ok(src) = pw_host::open_default() {
        return Ok(src);
    }
    cpal_mic::open_default()
}
