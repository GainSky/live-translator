use serde::Serialize;

// 事件名（前端订阅见 src/lib/ipc.ts EVENTS）
pub const EV_TRANSCRIPT_NEW: &str = "transcript:new";
pub const EV_TRANSCRIPT_UPDATE: &str = "transcript:update";
pub const EV_ENGINE_STATE: &str = "engine:state";
pub const EV_TRANSLATE_STATE: &str = "translate:state";
pub const EV_AUDIO_LEVEL: &str = "audio:level";
pub const EV_PIPELINE_ERROR: &str = "pipeline:error";
pub const EV_MODEL_PROGRESS: &str = "model:progress";
pub const EV_SETTINGS_CHANGED: &str = "settings:changed";
pub const EV_FILE_PROGRESS: &str = "file:progress";
pub const EV_FILE_TRANSCRIPT: &str = "file:transcript";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptPayload {
    pub id: String,
    pub session_id: String,
    pub source_id: String,
    pub source_name: String,
    /// 相对会话起始的毫秒时间戳（VAD 切句起止，导出 SRT 用）
    pub start_ms: u64,
    pub end_ms: u64,
    /// 墙钟时间 HH:MM:SS
    pub clock_time: String,
    pub raw_text: String,
    pub lang: Option<String>,
    pub translated_text: Option<String>,
    pub provider: Option<String>,
    pub asr_engine: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineStatePayload {
    pub engine: String,
    pub status: crate::asr::EngineStatus,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioLevelPayload {
    pub source_id: String,
    /// 0.0 ~ 1.0 峰值电平
    pub peak: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PipelineErrorPayload {
    pub source_id: Option<String>,
    pub message: String,
}

/// 模型下载进度（state: downloading/verifying/extracting/done/error）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelProgressPayload {
    pub id: String,
    pub state: String,
    pub downloaded: u64,
    pub total: u64,
    pub error: Option<String>,
}

/// 媒体文件转写进度（phase: preparing/decoding/done/canceled/error）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileProgressPayload {
    pub path: String,
    pub phase: String,
    pub decoded_secs: f64,
    pub total_secs: Option<f64>,
    pub segments: u64,
    pub message: Option<String>,
}

/// 设置变更广播（任一窗口保存后，所有窗口收敛到同一份配置）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsChangedPayload {
    pub settings: crate::settings::Settings,
}

/// 译文回填事件（翻译队列完成后推送）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptUpdatePayload {
    pub id: String,
    pub translated_text: String,
    pub provider: String,
}

/// 翻译通道状态（ok / loading / offline / cooldown / error）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateStatePayload {
    pub status: String,
    pub detail: Option<String>,
}
