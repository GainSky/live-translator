//! 媒体文件解码：产出 16k 单声道 f32 样本流。
//!
//! - 优先纯 Rust（symphonia）：wav/mp3/flac/ogg/m4a/mkv（含 mp4 内音频轨）
//! - 失败回退 ffmpeg 子进程（视频与其余编解码：ac3/dts/wmv…），
//!   `ffmpeg -vn -f f32le -ar 16000 -ac 1 pipe:1` 直出 16k

use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{DecoderOptions, CODEC_TYPE_NULL};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use crate::audio::resample::Resampler;
use crate::error::{AppError, AppResult};

/// 打开解码器（symphonia 优先，失败回退 ffmpeg）
/// 返回 (解码器, 总时长秒[可得时])
pub fn open(path: &Path) -> AppResult<(MediaDecoder, Option<f64>)> {
    match open_symphonia(path) {
        Ok(d) => Ok((d, None)), // 总时长由内部 n_samples 提供
        Err(e) => {
            tracing::info!("symphonia 解码不可用（{}），回退 ffmpeg: {e}", path.display());
            open_ffmpeg(path)
                .map(|d| (d, None))
                .map_err(|fe| {
                    AppError::Message(format!(
                        "解码失败：symphonia（{e}）/ ffmpeg（{fe}）。\
                         视频文件需要安装 ffmpeg（Windows: winget install Gyan.FFmpeg；Linux: pacman -S ffmpeg）"
                    ))
                })
        }
    }
}

/// 解码器：调用方循环 `next_chunk`（约 1 秒一批 16k 单声道样本）
/// 返回 true 表示到末尾
pub enum MediaDecoder {
    Symphonia(SymphoniaDecoder),
    Ffmpeg(FfmpegDecoder),
}

impl MediaDecoder {
    pub fn total_secs(&self) -> Option<f64> {
        match self {
            MediaDecoder::Symphonia(d) => d.total_secs,
            MediaDecoder::Ffmpeg(d) => d.total_secs,
        }
    }

    /// 读下一批 16k 单声道样本；返回 true = 已到末尾
    pub fn next_chunk(&mut self, out: &mut Vec<f32>) -> AppResult<bool> {
        match self {
            MediaDecoder::Symphonia(d) => d.next_chunk(out),
            MediaDecoder::Ffmpeg(d) => d.next_chunk(out),
        }
    }

    pub fn decoded_secs(&self) -> f64 {
        match self {
            MediaDecoder::Symphonia(d) => d.decoded_in as f64 / 16_000.0,
            MediaDecoder::Ffmpeg(d) => d.decoded_secs,
        }
    }
}

// ---------- symphonia 路径 ----------

pub struct SymphoniaDecoder {
    format: Box<dyn symphonia::core::formats::FormatReader>,
    decoder: Box<dyn symphonia::core::codecs::Decoder>,
    track_id: u32,
    resampler: Resampler,
    out_pending: Vec<f32>,
    total_secs: Option<f64>,
    decoded_in: u64,
}

fn open_symphonia(path: &Path) -> AppResult<MediaDecoder> {
    let file = File::open(path).map_err(|e| AppError::Message(format!("打开文件失败: {e}")))?;
    let mss = MediaSourceStream::new(
        Box::new(file),
        symphonia::core::io::MediaSourceStreamOptions::default(),
    );
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    let probed = symphonia::default::get_probe()
        .format(&hint, mss, &FormatOptions::default(), &MetadataOptions::default())
        .map_err(|e| AppError::Message(format!("媒体容器识别失败: {e}")))?;
    let format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or_else(|| AppError::Message("文件中没有可解码的音轨".into()))?;
    let track_id = track.id;
    let rate = track
        .codec_params
        .sample_rate
        .ok_or_else(|| AppError::Message("音轨缺少采样率信息".into()))?;
    // 时长：n_frames（帧数，音频=采样数）× time_base；缺失则进度条不定长
    let total_secs = track.codec_params.n_frames.map(|n| {
        let tb = track.codec_params.time_base.unwrap_or_default();
        (n as f64) * tb.numer as f64 / tb.denom as f64
    });

    let decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| AppError::Message(format!("音频解码器初始化失败: {e}")))?;

    Ok(MediaDecoder::Symphonia(SymphoniaDecoder {
        format,
        decoder,
        track_id,
        resampler: Resampler::new(rate, 16_000),
        out_pending: Vec::new(),
        total_secs,
        decoded_in: 0,
    }))
}

impl SymphoniaDecoder {
    fn next_chunk(&mut self, out: &mut Vec<f32>) -> AppResult<bool> {
        out.append(&mut self.out_pending);
        while out.len() < 16_000 {
            let packet = match self.format.next_packet() {
                Ok(p) => p,
                // 流结束
                Err(SymphoniaError::IoError(err))
                    if err.kind() == std::io::ErrorKind::UnexpectedEof =>
                {
                    return Ok(true)
                }
                Err(e) => return Err(AppError::Message(format!("读取媒体包失败: {e}"))),
            };
            if packet.track_id() != self.track_id {
                continue;
            }
            let decoded = match self.decoder.decode(&packet) {
                Ok(d) => d,
                // 单包坏帧：跳过继续
                Err(SymphoniaError::DecodeError(_)) => continue,
                Err(e) => return Err(AppError::Message(format!("音频解码失败: {e}"))),
            };
            let frames = decoded.frames() as u64;
            if frames == 0 {
                continue;
            }
            let spec = *decoded.spec();
            let mut sbuf = SampleBuffer::<f32>::new(frames, spec);
            sbuf.copy_interleaved_ref(decoded);
            let channels = spec.channels.count().max(1);
            let inter = &sbuf.samples()[..(frames as usize) * channels];
            let mono: Vec<f32> = if channels == 1 {
                inter.to_vec()
            } else {
                inter
                    .chunks_exact(channels)
                    .map(|c| c.iter().sum::<f32>() / channels as f32)
                    .collect()
            };
            self.decoded_in += frames;
            let out16 = self.resampler.process(&mono);
            self.out_pending.extend(out16);
            out.append(&mut self.out_pending);
        }
        Ok(false)
    }
}

// ---------- ffmpeg 路径 ----------

pub struct FfmpegDecoder {
    child: Child,
    stdout: Box<dyn Read>,
    stderr_tail: Arc<Mutex<String>>,
    remain: Vec<u8>,
    total_secs: Option<f64>,
    decoded_secs: f64,
    emitted: u64,
}

fn probe_duration_ffmpeg(path: &Path) -> Option<f64> {
    let out = Command::new("ffprobe")
        .args([
            "-v", "error", "-show_entries", "format=duration",
            "-of", "default=noprint_wrappers=1:nokey=1",
        ])
        .arg(path)
        .output()
        .ok()?;
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    s.parse::<f64>().ok()
}

fn open_ffmpeg(path: &Path) -> AppResult<MediaDecoder> {
    let total_secs = probe_duration_ffmpeg(path);
    let mut child = Command::new("ffmpeg")
        .args(["-nostdin", "-i"])
        .arg(path)
        .args(["-vn", "-f", "f32le", "-ar", "16000", "-ac", "1", "pipe:1"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            AppError::Message(format!(
                "启动 ffmpeg 失败（未安装或不在 PATH）: {e}"
            ))
        })?;
    let stdout = child.stdout.take().ok_or_else(|| {
        AppError::Message("ffmpeg stdout 不可读".into())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        AppError::Message("ffmpeg stderr 不可读".into())
    })?;
    // 后台持续排空 stderr（不排空会因管道写满而死锁），保留尾部用于报错
    let stderr_tail: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let tail = stderr_tail.clone();
    std::thread::spawn(move || {
        let mut s = String::new();
        let mut buf = [0u8; 4096];
        let mut reader = stderr;
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    s.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if s.len() > 8192 {
                        s.drain(..4096);
                    }
                }
            }
        }
        *tail.lock().unwrap() = s;
    });

    Ok(MediaDecoder::Ffmpeg(FfmpegDecoder {
        child,
        stdout: Box::new(stdout),
        stderr_tail,
        remain: Vec::new(),
        total_secs,
        decoded_secs: 0.0,
        emitted: 0,
    }))
}

impl FfmpegDecoder {
    fn next_chunk(&mut self, out: &mut Vec<f32>) -> AppResult<bool> {
        let mut bytes = [0u8; 64_000]; // 16000 f32 = 1s
        let n = self
            .stdout
            .read(&mut bytes)
            .map_err(|e| AppError::Message(format!("读取 ffmpeg 输出失败: {e}")))?;
        self.remain.extend_from_slice(&bytes[..n]);

        // 只消费完整 f32（4 字节），残字节留待下次
        let complete = self.remain.len() - self.remain.len() % 4;
        let ready: Vec<u8> = self.remain.drain(..complete).collect();
        let pushed = ready.len() / 4;
        for b in ready.chunks_exact(4) {
            out.push(f32::from_le_bytes([b[0], b[1], b[2], b[3]]));
        }
        self.emitted += pushed as u64;
        self.decoded_secs = self.emitted as f64 / 16_000.0;

        if n == 0 {
            // 流结束：校验退出码
            let status = self
                .child
                .wait()
                .map_err(|e| AppError::Message(format!("等待 ffmpeg 退出失败: {e}")))?;
            if !status.success() {
                let tail = self.stderr_tail.lock().unwrap().clone();
                return Err(AppError::Message(format!(
                    "ffmpeg 转码失败: {tail}"
                )));
            }
            return Ok(true);
        }
        Ok(false)
    }
}
