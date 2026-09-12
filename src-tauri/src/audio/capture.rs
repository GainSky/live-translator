//! cpal 采集流构建：任意采样格式 → 单声道 f32 块 → 通道
//!
//! 适配 cpal 0.18（StreamConfig 手动构造）；F32/I16/U16 三种格式全处理，
//! 多声道均值下混（移植自上游 rust-api-examples 的麦克风示例逻辑）。

use cpal::traits::DeviceTrait;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;

use crate::error::{AppError, AppResult};

pub struct OpenedStream {
    pub stream: cpal::Stream,
    /// 设备原生采样率（重采样到 16k 用）
    pub native_rate: u32,
}

pub fn build_input_stream(
    device: &cpal::Device,
    tx: Sender<Vec<f32>>,
    // 停止标志：置位后流销毁触发的 "Device disconnected" 等错误回调不再记日志
    stopping: Arc<AtomicBool>,
) -> AppResult<OpenedStream> {
    let supported = match device.default_input_config() {
        Ok(s) => s,
        Err(e) => {
            // 应用播放流（PipeWire StreamOutput）方向为 Output，input_config 会拒绝；
            // output_config 返回同样的节点参数（channels/sample_rate）
            device.default_output_config().map_err(|_| {
                AppError::Message(format!("获取设备默认配置失败: {e}"))
            })?
        }
    };
    let sample_format = supported.sample_format();
    let channels = supported.channels();
    let native_rate = supported.sample_rate();
    let config = cpal::StreamConfig {
        channels,
        sample_rate: native_rate,
        buffer_size: cpal::BufferSize::Default,
    };

    // err_fn 被 cpal 存起来在流销毁时也可能触发（Device disconnected），需可移动
    let stop_flag = stopping;
    let err_fn = move |err| {
        if !stop_flag.load(Ordering::Relaxed) {
            tracing::warn!("音频流错误: {err}");
        }
    };

    let stream = match sample_format {
        cpal::SampleFormat::F32 => device.build_input_stream(
            config,
            move |data: &[f32], _| {
                if !data.is_empty() {
                    let _ = tx.send(downmix(data, channels));
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::I16 => device.build_input_stream(
            config,
            move |data: &[i16], _| {
                if !data.is_empty() {
                    let f: Vec<f32> = data.iter().map(|&s| s as f32 / i16::MAX as f32).collect();
                    let _ = tx.send(downmix(&f, channels));
                }
            },
            err_fn,
            None,
        )?,
        cpal::SampleFormat::U16 => device.build_input_stream(
            config,
            move |data: &[u16], _| {
                if !data.is_empty() {
                    let f: Vec<f32> = data
                        .iter()
                        .map(|&s| (s as f32 - 32768.0) / 32768.0)
                        .collect();
                    let _ = tx.send(downmix(&f, channels));
                }
            },
            err_fn,
            None,
        )?,
        other => {
            return Err(AppError::Message(format!("不支持的采样格式: {other:?}")));
        }
    };

    Ok(OpenedStream { stream, native_rate })
}

/// 多声道 → 单声道（均值下混）
fn downmix(data: &[f32], channels: u16) -> Vec<f32> {
    let ch = channels.max(1) as usize;
    if ch == 1 {
        return data.to_vec();
    }
    data.chunks(ch)
        .map(|frame| frame.iter().sum::<f32>() / frame.len() as f32)
        .collect()
}
