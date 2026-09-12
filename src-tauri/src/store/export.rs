use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};
use crate::events::TranscriptPayload;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    Txt,
    Md,
    Srt,
    Csv,
    Json,
}

impl ExportFormat {
    pub fn parse(s: &str) -> AppResult<Self> {
        match s.to_ascii_lowercase().as_str() {
            "txt" => Ok(Self::Txt),
            "md" | "markdown" => Ok(Self::Md),
            "srt" => Ok(Self::Srt),
            "csv" => Ok(Self::Csv),
            "json" => Ok(Self::Json),
            other => Err(AppError::Message(format!("不支持的导出格式: {other}"))),
        }
    }
}

/// 导出转写记录（M5 实现，readme §5.4）。
///
/// SRT 时间轴 = VAD 切句起止（start_ms/end_ms），支持双语或单语字幕。
pub fn export(
    _items: &[TranscriptPayload],
    format: ExportFormat,
    out_dir: &Path,
) -> AppResult<PathBuf> {
    let _ = format;
    Err(AppError::NotImplemented(format!(
        "M5: 导出 {:?}（输出目录 {}）",
        format,
        out_dir.display()
    )))
}
