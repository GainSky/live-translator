//! Windows 系统声音环回（WASAPI 设备级 LOOPBACK）。
//!
//! 原理：对 Render（输出）设备以 Capture 方向 + Shared 模式初始化时，
//! wasapi-rs 的 `initialize_client` 内部自动附加 `AUDCLNT_STREAMFLAGS_LOOPBACK`
//! —— 捕获该设备正在播放的全部系统声音。
//!
//! 加分项：EventsShared 模式的 `autoconvert` 让 WASAPI 直接输出
//! **16kHz 单声道 f32**（我们请求的格式），免重采样直进流水线。
//!
//! 按应用流捕获（进程级 LOOPBACK，对应 Linux 应用流功能）使用
//! `AudioClient::new_application_loopback_client(process_id, include_tree)`，
//! 留作后续增强（readme §10.5）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use wasapi::{DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat};

use super::{AudioDeviceDescriptor, DeviceKind, OpenedSource, SourceDevice};
use crate::error::{AppError, AppResult};

pub const ID_PREFIX: &str = "wasapi:";
/// 环回路直接让 WASAPI 输出 16k 单声道（autoconvert 完成 SRC），无需重采样
const CAPTURE_RATE: u32 = 16_000;

fn wasapi_err(context: &str, e: wasapi::WasapiError) -> AppError {
    AppError::Message(format!("{context}: {e}"))
}

fn make_desc(endpoint_id: &str, name: &str, is_default: bool) -> AudioDeviceDescriptor {
    AudioDeviceDescriptor {
        id: format!("{ID_PREFIX}{endpoint_id}"),
        name: name.to_string(),
        kind: DeviceKind::Loopback,
        // 运行时 autoconvert 输出固定 16k/1ch
        sample_rate: CAPTURE_RATE,
        channels: 1,
        is_default,
        signal: 0.0,
    }
}

pub fn enumerate() -> AppResult<Vec<AudioDeviceDescriptor>> {
    let _ = wasapi::initialize_mta();
    let enumerator =
        DeviceEnumerator::new().map_err(|e| wasapi_err("COM 枚举器创建失败", e))?;
    let default_id = enumerator
        .get_default_device(&Direction::Render)
        .and_then(|d| d.get_id())
        .ok();

    let mut out = vec![make_desc("default-loopback", "系统声音（默认输出）", true)];
    for (id, name) in enumerate_render_devices()? {
        out.push(make_desc(
            &id,
            &format!("{name}（系统声音）"),
            Some(&id) == default_id.as_deref(),
        ));
    }
    Ok(out)
}

fn enumerate_render_devices() -> AppResult<Vec<(String, String)>> {
    let _ = wasapi::initialize_mta();
    let enumerator =
        DeviceEnumerator::new().map_err(|e| wasapi_err("COM 枚举器创建失败", e))?;
    let coll = enumerator
        .get_device_collection(&Direction::Render)
        .map_err(|e| wasapi_err("输出设备枚举失败", e))?;
    let n = coll
        .get_nbr_devices()
        .map_err(|e| wasapi_err("设备数量获取失败", e))?;
    let mut out = Vec::new();
    for i in 0..n {
        let device = coll
            .get_device_at_index(i)
            .map_err(|e| wasapi_err("设备获取失败", e))?;
        let id = device.get_id().map_err(|e| wasapi_err("设备 id 获取失败", e))?;
        let name = device
            .get_friendlyname()
            .map_err(|e| wasapi_err("设备名获取失败", e))?;
        out.push((id, name));
    }
    Ok(out)
}

fn open_endpoint(endpoint_id: &str) -> AppResult<wasapi::Device> {
    let _ = wasapi::initialize_mta();
    let enumerator =
        DeviceEnumerator::new().map_err(|e| wasapi_err("COM 枚举器创建失败", e))?;
    if endpoint_id == "default-loopback" {
        return enumerator
            .get_default_device(&Direction::Render)
            .map_err(|e| wasapi_err("默认输出设备获取失败", e));
    }
    enumerator
        .get_device(endpoint_id)
        .map_err(|e| wasapi_err(format!("输出设备 {endpoint_id} 获取失败"), e))
}

pub fn open_default() -> AppResult<OpenedSource> {
    let device = open_endpoint("default-loopback")?;
    Ok(OpenedSource {
        desc: make_desc("default-loopback", "系统声音（默认输出）", true),
        device: SourceDevice::WasapiLoopback { endpoint_id: "default-loopback".into() },
    })
}

/// 按 endpoint id 打开环回源
pub fn open(id_suffix: &str) -> AppResult<OpenedSource> {
    let device = open_endpoint(id_suffix)?;
    let name = device
        .get_friendlyname()
        .unwrap_or_else(|_| id_suffix.to_string());
    Ok(OpenedSource {
        desc: make_desc(id_suffix, &format!("{name}（系统声音）"), false),
        device: SourceDevice::WasapiLoopback { endpoint_id: id_suffix.to_string() },
    })
}

/// 启动环回采集线程（WASAPI autoconvert 直接输出 16k 单声道 f32），
/// 返回 (有效采样率, 线程句柄)。停止置位后线程在 ≤200ms 内退出。
pub fn spawn_capture(
    endpoint_id: String,
    tx: Sender<Vec<f32>>,
    stopping: Arc<AtomicBool>,
) -> AppResult<(u32, JoinHandle<()>)> {
    let handle = std::thread::Builder::new()
        .name("wasapi-loopback".into())
        .spawn(move || {
            if let Err(e) = capture_loop(&endpoint_id, tx, stopping) {
                tracing::error!("WASAPI 环回采集失败: {e}");
            }
        })
        .map_err(|e| AppError::Message(format!("环回采集线程启动失败: {e}")))?;
    Ok((CAPTURE_RATE, handle))
}

fn capture_loop(
    endpoint_id: &str,
    tx: Sender<Vec<f32>>,
    stopping: Arc<AtomicBool>,
) -> AppResult<()> {
    let _ = wasapi::initialize_mta();
    let device = open_endpoint(endpoint_id)?;
    let mut audio_client = device
        .get_iaudioclient()
        .map_err(|e| wasapi_err("AudioClient 获取失败", e))?;

    // 直接请求 16k 单声道 f32；EventsShared + autoconvert 由 WASAPI 完成 SRC/下混，
    // Render 设备 + Capture 方向 → 内部自动 LOOPBACK 标志
    let format = WaveFormat::new(32, 32, &SampleType::Float, CAPTURE_RATE as i32, 1, None);
    let (def_time, _min) = audio_client
        .get_device_period()
        .map_err(|e| wasapi_err("设备周期获取失败", e))?;
    audio_client
        .initialize_client(
            &format,
            &Direction::Capture,
            &StreamMode::EventsShared {
                autoconvert: true,
                buffer_duration_hns: def_time,
            },
        )
        .map_err(|e| wasapi_err("环回客户端初始化失败", e))?;

    let h_event = audio_client
        .set_get_eventhandle()
        .map_err(|e| wasapi_err("事件句柄获取失败", e))?;
    let capture_client = audio_client
        .get_audiocaptureclient()
        .map_err(|e| wasapi_err("捕获客户端获取失败", e))?;
    audio_client
        .start_stream()
        .map_err(|e| wasapi_err("采集流启动失败", e))?;
    tracing::info!("WASAPI 环回采集启动: {} (16kHz mono)", endpoint_id);

    let mut queue: std::collections::VecDeque<u8> = std::collections::VecDeque::new();
    let mut sample_buf: Vec<f32> = Vec::with_capacity(1024);

    while !stopping.load(Ordering::SeqCst) {
        // 有事件才读（无数据时 read 无意义）
        if h_event.wait_for_event(200).is_err() {
            continue; // 超时：回查 stopping
        }
        let frames = capture_client
            .get_next_packet_size()
            .map_err(|e| wasapi_err("包大小获取失败", e))?
            .unwrap_or(0);
        if frames > 0 {
            capture_client
                .read_from_device_to_deque(&mut queue)
                .map_err(|e| wasapi_err("环回读取失败", e))?;
            // 每帧 4 字节 f32 LE（16k mono）
            while queue.len() >= 4 {
                let b0 = queue.pop_front().unwrap();
                let b1 = queue.pop_front().unwrap();
                let b2 = queue.pop_front().unwrap();
                let b3 = queue.pop_front().unwrap();
                sample_buf.push(f32::from_le_bytes([b0, b1, b2, b3]));
                if sample_buf.len() >= 512 {
                    let _ = tx.send(std::mem::take(&mut sample_buf));
                }
            }
        }
    }

    let _ = audio_client.stop_stream();
    tracing::info!("WASAPI 环回采集停止: {endpoint_id}");
    Ok(())
}

/// 信号探测：临时起环回采集 700ms 测峰值（Windows 环回源专用）
pub fn probe_peak(endpoint_id: &str) -> f32 {
    let (tx, rx) = std::sync::mpsc::channel::<Vec<f32>>();
    let stopping = Arc::new(AtomicBool::new(true)); // 探测期错误回调静默
    let handle = match std::thread::Builder::new()
        .name("probe-loopback".into())
        .spawn(move || {
            if let Err(e) = capture_loop(endpoint_id, tx, stopping.clone()) {
                tracing::debug!("环回探测失败: {e}");
            }
        }) {
        Ok(h) => h,
        Err(_) => return 0.0,
    };
    let deadline = Instant::now() + Duration::from_millis(700);
    let mut peak = 0f32;
    while Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(chunk) => {
                let p = chunk.iter().fold(0f32, |m, &s| m.max(s.abs()));
                peak = peak.max(p);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => break,
        }
    }
    stopping.store(true, Ordering::SeqCst);
    let _ = handle.join();
    peak.min(1.0)
}
