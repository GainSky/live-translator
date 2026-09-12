//! cpal 通用麦克风枚举/采集（Linux ALSA 兜底 / Windows WASAPI / macOS CoreAudio）
//!
//! Linux 启用 `audio-pipewire` / `audio-pulseaudio` feature 后由 pw_host / pa_host 接管，
//! 本模块作为兜底路径；环回采集不走 cpal：Windows 用 wasapi crate（M1-Win），
//! Linux 用 PipeWire STREAM_CAPTURE_SINK。

use cpal::traits::{DeviceTrait, HostTrait};

use super::{classify, AudioDeviceDescriptor, DeviceKind, OpenedSource};
use crate::error::{AppError, AppResult};

pub const ID_PREFIX: &str = "cpal:";

fn default_host() -> cpal::Host {
    cpal::default_host()
}

fn describe(device: &cpal::Device, is_default: bool) -> Option<AudioDeviceDescriptor> {
    let desc = device.description().ok()?;
    if !device.supports_input() {
        return None;
    }
    let name = desc.name().to_string();
    let (sample_rate, channels) = device
        .default_input_config()
        .map(|c| (c.sample_rate(), c.channels()))
        .unwrap_or((0, 0));
    Some(AudioDeviceDescriptor {
        id: format!("{ID_PREFIX}{name}"),
        name,
        kind: classify(&desc),
        sample_rate,
        channels,
        is_default,
        signal: 0.0,
    })
}

pub fn enumerate() -> AppResult<Vec<AudioDeviceDescriptor>> {
    let host = default_host();
    let mut out = Vec::new();

    let default_in_name = host
        .default_input_device()
        .and_then(|d| d.description().ok())
        .map(|d| d.name().to_string());

    let devices = host
        .devices()
        .map_err(|e| AppError::Message(format!("音频设备枚举失败: {e}")))?;

    for device in devices {
        let is_default = device
            .description()
            .map(|desc| default_in_name.as_deref() == Some(desc.name()))
            .unwrap_or(false);
        if let Some(d) = describe(&device, is_default) {
            out.push(d);
        }
    }
    Ok(out)
}

pub fn open_default() -> AppResult<OpenedSource> {
    let device = default_host()
        .default_input_device()
        .ok_or_else(|| AppError::Message("无默认输入设备".into()))?;
    let desc = describe(&device, true)
        .ok_or_else(|| AppError::Message("默认输入设备不支持采集".into()))?;
    Ok(OpenedSource { desc, device })
}

pub fn open(id_suffix: &str) -> AppResult<OpenedSource> {
    let devices = default_host()
        .devices()
        .map_err(|e| AppError::Message(format!("音频设备枚举失败: {e}")))?;
    for device in devices {
        if let Some(desc) = describe(&device, false) {
            if desc.name == id_suffix {
                return Ok(OpenedSource { desc, device });
            }
        }
    }
    Err(AppError::Message(format!("未找到音频设备: {id_suffix}")))
}

#[allow(unused)]
fn _kind_guard(_k: DeviceKind) {}
