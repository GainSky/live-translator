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
    /// 句首预缓冲（毫秒）：VAD 确认语音前的音频并入句首，防止起始吞字
    pub pre_pad_ms: u64,
}

impl Default for VadParams {
    fn default() -> Self {
        Self {
            threshold: 0.4,
            min_silence_ms: 500,
            min_speech_ms: 150,
            max_speech_ms: 10_000,
            pre_pad_ms: 400,
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
    total_samples: u64,
    /// 滚动预缓冲：最近 pre_pad_ms 的音频（按窗口边界维护）
    pre_roll: Vec<f32>,
    pre_roll_cap: usize,
    /// 上一窗口的检测状态（false→true 跳变 = 新语段起始）
    prev_detected: bool,
    /// 起始瞬间快照的预缓冲（emit 时拼到句首）
    speech_pre_roll: Vec<f32>,
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
            total_samples: 0,
            pre_roll: Vec::new(),
            pre_roll_cap: params.pre_pad_ms as usize * SAMPLE_RATE as usize / 1000,
            prev_detected: false,
            speech_pre_roll: Vec::new(),
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

            // 语音起始（静音→语音跳变）：快照此前的预缓冲并入句首。
            // Silero 需数个窗口确认语音，确认期音频在段外——不补就吞起始字。
            let detected = self.vad.detected();
            if detected && !self.prev_detected {
                self.speech_started = true;
                self.speech_pre_roll = self.pre_roll.clone();
            }
            self.prev_detected = detected;

            // 本窗口进入滚动缓冲（快照在先，起始窗口不重复）
            self.pre_roll.extend_from_slice(&window);
            if self.pre_roll.len() > self.pre_roll_cap {
                let drop = self.pre_roll.len() - self.pre_roll_cap;
                self.pre_roll.drain(..drop);
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
        self.pre_roll.clear();
        self.speech_pre_roll.clear();
        self.prev_detected = false;
        out
    }

    fn drain_finished(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        while let Some(seg) = self.vad.front() {
            let samples = seg.samples().to_vec();
            self.vad.pop();
            let end = self.total_samples;
            // 句首拼入预缓冲（起始前的音频），时间轴同步回退
            let mut full = std::mem::take(&mut self.speech_pre_roll);
            full.extend_from_slice(&samples);
            let start = end.saturating_sub(full.len() as u64);
            out.push(Segment {
                start_ms: start * 1000 / SAMPLE_RATE as u64,
                end_ms: end * 1000 / SAMPLE_RATE as u64,
                samples: full,
            });
            self.speech_started = false;
        }
        out
    }
}
