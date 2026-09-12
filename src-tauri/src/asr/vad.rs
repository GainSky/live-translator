use serde::{Deserialize, Serialize};
use sherpa_onnx::{SileroVadModelConfig, VadModelConfig, VoiceActivityDetector};
use std::path::Path;

use crate::error::{AppError, AppResult};

pub const SAMPLE_RATE: i32 = 16_000;
/// Silero VAD 喂入窗口（上游要求固定值，勿改）
pub const WINDOW_SIZE: usize = 512;

/// VAD 参数 —— 数值移植自参考实现（docs/porting-notes.md §1.1）
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct VadParams {
    pub threshold: f32,
    pub min_silence_ms: u64,
    pub min_speech_ms: u64,
    pub max_speech_ms: u64,
}

impl Default for VadParams {
    fn default() -> Self {
        Self {
            threshold: 0.4,
            min_silence_ms: 500,
            min_speech_ms: 150,
            max_speech_ms: 10_000,
        }
    }
}

/// 一个切出的语音句段（媒体时间轴，毫秒，相对会话起始）
#[derive(Debug, Clone)]
pub struct Segment {
    pub start_ms: u64,
    pub end_ms: u64,
    pub samples: Vec<f32>,
}

/// 每路音频源独立的 Silero VAD 切句器（多路互不干扰——参考实现经验，docs/porting-notes.md §4）
pub struct Segmenter {
    vad: VoiceActivityDetector,
    pending: Vec<f32>,
    speech_started: bool,
    speech_start_sample: u64,
    total_samples: u64,
}

impl Segmenter {
    pub fn new(params: &VadParams, silero_model_path: &Path) -> AppResult<Self> {
        let mut silero = SileroVadModelConfig::default();
        silero.model = Some(silero_model_path.display().to_string());
        silero.threshold = params.threshold;
        silero.min_silence_duration = params.min_silence_ms as f32 / 1000.0;
        silero.min_speech_duration = params.min_speech_ms as f32 / 1000.0;
        silero.max_speech_duration = params.max_speech_ms as f32 / 1000.0;
        silero.window_size = WINDOW_SIZE as _;

        let config = VadModelConfig {
            silero_vad: silero,
            sample_rate: SAMPLE_RATE,
            num_threads: 1,
            provider: Some("cpu".into()),
            debug: false,
            ..Default::default()
        };
        let vad = VoiceActivityDetector::create(&config, 30.0).ok_or_else(|| {
            AppError::Message("Silero VAD 初始化失败（onnx runtime 初始化错误）".into())
        })?;
        Ok(Self {
            vad,
            pending: Vec::new(),
            speech_started: false,
            speech_start_sample: 0,
            total_samples: 0,
        })
    }

    /// 喂入 16k 单声道样本；返回切出的完整句段
    pub fn feed(&mut self, input: &[f32]) -> Vec<Segment> {
        self.pending.extend_from_slice(input);
        let mut out = Vec::new();

        while self.pending.len() >= WINDOW_SIZE {
            let window: Vec<f32> = self.pending.drain(..WINDOW_SIZE).collect();
            self.total_samples += WINDOW_SIZE as u64;
            self.vad.accept_waveform(&window);

            if !self.speech_started && self.vad.detected() {
                self.speech_started = true;
                self.speech_start_sample = self.total_samples;
            }
            out.extend(self.drain_finished());
        }
        out
    }

    /// 流结束时冲出尚未收尾的语音段
    pub fn flush(&mut self) -> Vec<Segment> {
        self.vad.flush();
        let out = self.drain_finished();
        self.pending.clear();
        out
    }

    fn drain_finished(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Some(seg) = self.vad.front() {
            let samples = seg.samples().to_vec();
            self.vad.pop();
            let end = self.total_samples;
            let len = samples.len() as u64;
            let start = self.speech_start_sample.min(end.saturating_sub(len));
            out.push(Segment {
                start_ms: start * 1000 / SAMPLE_RATE as u64,
                end_ms: end * 1000 / SAMPLE_RATE as u64,
                samples,
            });
            self.speech_started = false;
        }
        out
    }
}
