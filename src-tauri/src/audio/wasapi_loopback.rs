//! Windows WASAPI Loopback 系统声音环回（cpal 不支持环回，故用 wasapi crate）
//!
//! 规划（readme §2.1）：枚举默认/全部渲染设备并暴露为 Loopback 输入源；
//! 采集线程输出与麦克风同构的 f32 块进入统一流水线。
//!
//! M1-Win 实现：wasapi crate 的 get_default_device / initialize_client(loopback=true)。
//! 当前为接口占位（保持 Windows 构建通过），实现时补齐下方三个函数。

use super::{AudioDeviceDescriptor, DeviceKind, OpenedSource};
use crate::error::{AppError, AppResult};

pub const ID_PREFIX: &str = "wasapi:";

pub fn enumerate() -> AppResult<Vec<AudioDeviceDescriptor>> {
    Err(AppError::NotImplemented(
        "M1-Win: WASAPI Loopback 设备枚举（仅 Windows 构建生效）".into(),
    ))
}

pub fn open_default() -> AppResult<OpenedSource> {
    Err(AppError::NotImplemented(
        "M1-Win: WASAPI 默认输出环回采集".into(),
    ))
}

/// 按 id 后缀打开设备（后缀为 WASAPI 设备名）
pub fn open(id_suffix: &str) -> AppResult<OpenedSource> {
    Err(AppError::NotImplemented(format!(
        "M1-Win: WASAPI Loopback 采集流（设备: {id_suffix}）"
    )))
}

#[allow(unused)]
fn _kind_guard(_k: DeviceKind) {}
