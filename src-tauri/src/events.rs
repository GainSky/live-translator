use serde::Serialize;

// 事件名（前端订阅见 src/lib/ipc.ts EVENTS）
pub const EV_TRANSCRIPT_NEW: &str = "transcript:new";
pub const EV_TRANSCRIPT_UPDATE: &str = "transcript:update";
pub const EV_ENGINE_STATE: &str = "engine:state";
pub const EV_TRANSLATE_STATE: &str = "translate:state";
pub const EV_AUDIO_LEVEL: &str = "audio:level";
pub const EV_PIPELINE_ERROR: &str = "pipeline:error";

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
