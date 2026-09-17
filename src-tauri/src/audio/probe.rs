//! 音频源信号测试：短时打开采集流测峰值（音源页"有声标识"排序用）
//!
//! 每设备一线程并行探测（设备句柄不可跨线程，故线程内自解析），
//! 总耗时 ≈ 单设备探测窗口（~0.7s），与设备数量无关。

use std::sync::atomic::AtomicBool;
use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use cpal::traits::StreamTrait;
use cpal::Device;

/// 信号判定阈值（峰值 0~1；Firefox 实测播放 0.05~0.5，静音 ~0）
pub const SIGNAL_THRESHOLD: f32 = 0.01;
/// 单设备探测时长
pub const PROBE_DURATION: Duration = Duration::from_millis(700);

/// 打开设备采集 ~PROBE_DURATION，返回窗口内峰值（0~1）。打开失败返回 0。
pub fn probe_one(device: &Device) -> f32 {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    // 探测期的错误回调静默（流销毁触发的 Device disconnected 等）
    let stopping = Arc::new(AtomicBool::new(true));
    let stream = match super::capture::build_input_stream(device, tx, stopping) {
        Ok(s) => s,
        Err(_) => return 0.0,
    };
    if stream.stream.play().is_err() {
        return 0.0;
    }
    let deadline = Instant::now() + PROBE_DURATION;
    let mut peak = 0f32;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                let p = chunk.iter().fold(0f32, |m, &s| m.max(s.abs()));
                peak = peak.max(p);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    drop(stream);
    peak.min(1.0)
}

/// 并行探测多台设备（每设备一线程），返回 (id, peak) 列表
pub fn probe_parallel(ids: &[String]) -> Vec<(String, f32)> {
    let mut handles = Vec::new();
    for id in ids {
        let id = id.clone();
        if let Ok(h) = std::thread::Builder::new()
            .name(format!("probe:{id}"))
            .spawn(move || {
                let peak = super::resolve_source(&id)
                    .map(|src| probe_source(&src))
                    .unwrap_or(0.0);
                (id, peak)
            })
        {
            handles.push(h);
        }
    }
    handles
        .into_iter()
        .filter_map(|h| h.join().ok())
        .collect()
}

/// 按后端分发探测：cpal 设备 / Windows WASAPI 环回
pub fn probe_source(source: &super::OpenedSource) -> f32 {
    match &source.device {
        super::SourceDevice::Cpal(device) => probe_one(device),
        #[cfg(target_os = "windows")]
        super::SourceDevice::WasapiLoopback { endpoint_id } => {
            super::wasapi_loopback::probe_peak(endpoint_id)
        }
    }
}
