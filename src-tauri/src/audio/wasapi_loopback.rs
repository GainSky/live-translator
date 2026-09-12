//! Windows WASAPI Loopback 系统声音环回（cpal 不支持环回，故用 wasapi crate）
//!
//! 规划（readme §2.1）：枚举默认/全部渲染设备并暴露为 Loopback 输入源；
//! 采集线程输出与麦克风同构的 f32 块进入统一流水线。
//!
//! M1 实现：wasapi crate 的 get_default_device / initialize_client(loopback=true)。

use super::{AudioDeviceDescriptor, DeviceKind};
use crate::error::{AppError, AppResult};

pub fn enumerate() -> AppResult<Vec<AudioDeviceDescriptor>> {
    Err(AppError::NotImplemented(
        "M1: WASAPI Loopback 设备枚举（仅 Windows 构建生效）".into(),
    ))
}

pub fn open_capture(_device_id: &str) -> AppResult<()> {
    Err(AppError::NotImplemented("M1: WASAPI Loopback 采集流".into()))
}

#[allow(unused)]
fn _kind_guard(_k: DeviceKind) {}
