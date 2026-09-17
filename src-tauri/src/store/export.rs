//! 会话记录导出：TXT / Markdown / SRT（双语字幕）/ CSV / JSON。
//!
//! SRT 时间轴 = VAD 切句起止（start_ms/end_ms，媒体时间）；
//! 双语导出：译文存在时作为第二行/第二列。

use std::io::Write;
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

    pub fn ext(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Srt => "srt",
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

fn ms_srt(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    let mmm = ms % 1000;
    format!("{h:02}:{m:02}:{s:02},{mmm:03}")
}

fn ms_clock(ms: u64) -> String {
    let h = ms / 3_600_000;
    let m = (ms % 3_600_000) / 60_000;
    let s = (ms % 60_000) / 1000;
    format!("{h:02}:{m:02}:{s:02}")
}

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

/// 导出转写记录到 out_path，返回实际写入的文件路径
pub fn export(
    items: &[TranscriptPayload],
    format: ExportFormat,
    out_path: &Path,
    session_id: &str,
    session_start: &str,
) -> AppResult<PathBuf> {
    if items.is_empty() {
        return Err(AppError::Message("当前会话没有可导出的转写记录".into()));
    }
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = match format {
        ExportFormat::Txt => render_txt(items, session_id, session_start),
        ExportFormat::Md => render_md(items, session_id, session_start),
        ExportFormat::Srt => render_srt(items),
        ExportFormat::Csv => render_csv(items),
        ExportFormat::Json => serde_json::to_string_pretty(items)?,
    };
    std::fs::write(out_path, body)?;
    Ok(out_path.to_path_buf())
}

fn render_txt(items: &[TranscriptPayload], session_id: &str, session_start: &str) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# LiveTranslator 转写记录\n会话: {session_id}  开始: {session_start}\n\n---\n\n"
    ));
    for t in items {
        out.push_str(&format!(
            "[{}] ({}) {}\n",
            t.clock_time, t.source_name, t.raw_text
        ));
        if let Some(tr) = &t.translated_text {
            out.push_str(&format!("    译文: {tr}\n"));
        }
    }
    out
}

fn render_md(items: &[TranscriptPayload], session_id: &str, session_start: &str) -> String {
    let mut out = format!(
        "# LiveTranslator 转写记录\n\n*   **会话**：{session_id}\n*   **开始时间**：{session_start}\n\n---\n\n"
    );
    for t in items {
        out.push_str(&format!(
            "### 🕒 [{} | {}]（{}）\n\n*   **原文**：{}\n",
            t.clock_time,
            ms_clock(t.start_ms),
            t.source_name,
            t.raw_text
        ));
        if let Some(tr) = &t.translated_text {
            out.push_str(&format!("*   **译文**：{tr}\n"));
        }
        out.push('\n');
    }
    out
}

fn render_srt(items: &[TranscriptPayload]) -> String {
    let mut out = String::new();
    for (i, t) in items.iter().enumerate() {
        out.push_str(&format!(
            "{}\n{} --> {}\n{}\n\n",
            i + 1,
            ms_srt(t.start_ms),
            ms_srt(t.end_ms),
            // 双语字幕：译文存在时作为第二行
            match &t.translated_text {
                Some(tr) if !tr.is_empty() => format!("{}\n{}", t.raw_text, tr),
                _ => t.raw_text.clone(),
            }
        ));
    }
    out
}

fn render_csv(items: &[TranscriptPayload]) -> String {
    let mut out = String::from(
        "clock_time,source,lang,start_ms,end_ms,raw_text,translated_text,provider,asr_engine\n",
    );
    for t in items {
        out.push_str(&format!(
            "{},{},{},{},{},{},{},{},{}\n",
            csv_field(&t.clock_time),
            csv_field(&t.source_name),
            csv_field(t.lang.as_deref().unwrap_or("")),
            t.start_ms,
            t.end_ms,
            csv_field(&t.raw_text),
            csv_field(t.translated_text.as_deref().unwrap_or("")),
            csv_field(t.provider.as_deref().unwrap_or("")),
            csv_field(t.asr_engine.as_deref().unwrap_or("")),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> TranscriptPayload {
        TranscriptPayload {
            id: "1".into(),
            session_id: "s-1".into(),
            source_id: "mic".into(),
            source_name: "麦克风".into(),
            start_ms: 29_200,
            end_ms: 36_600,
            clock_time: "12:00:01".into(),
            raw_text: "你好, 世界".into(),
            lang: Some("zh".into()),
            translated_text: Some("你好, 世界（译）".into()),
            provider: Some("openai-compatible".into()),
            asr_engine: Some("sense-voice".into()),
        }
    }

    #[test]
    fn srt_timeline_format() {
        let s = render_srt(&[sample()]);
        assert!(s.contains("1\n00:00:29,200 --> 00:00:36,600\n"));
        assert!(s.contains("你好, 世界\n你好, 世界（译）"));
    }

    #[test]
    fn csv_escapes_commas_and_quotes() {
        let s = render_csv(&[sample()]);
        assert!(s.contains("\"你好, 世界\"")); // 含逗号 → 整个字段加引号
    }

    #[test]
    fn parse_formats() {
        assert!(matches!(ExportFormat::parse("md"), Ok(ExportFormat::Md)));
        assert!(ExportFormat::parse("exe").is_err());
    }
}
