//! SenseVoice 引擎（官方 sherpa-onnx Rust 绑定，OfflineRecognizer）
//!
//! 模型：sherpa-onnx-sense-voice-zh-en-ja-ko-yue int8（model.int8.onnx + tokens.txt）
//! 参数移植自参考实现：use_itn=true、num_threads=4、CUDA→CPU 降级（M7 接 CUDA）。

use std::path::Path;

use sherpa_onnx::OfflineRecognizerConfig;

use crate::asr::{TranscriptCandidate, ASREngine};
use crate::error::{AppError, AppResult};

pub const MODEL_DIR_NAME: &str = "sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17";
pub const MODEL_FILE: &str = "model.int8.onnx";
pub const TOKENS_FILE: &str = "tokens.txt";

pub struct SenseVoiceEngine {
    recognizer: sherpa_onnx::OfflineRecognizer,
}

impl SenseVoiceEngine {
    /// 加载模型（模型文件缺失时报错并给出下载/放置指引）
    pub fn load(model_root: &Path, source_lang: &str) -> AppResult<Self> {
        let dir = model_root.join(MODEL_DIR_NAME);
        let model = dir.join(MODEL_FILE);
        let tokens = dir.join(TOKENS_FILE);
        for f in [&model, &tokens] {
            if !f.is_file() {
                return Err(AppError::Message(format!(
                    "缺少模型文件: {}（请通过设置页下载模型，或放置到模型目录）",
                    f.display()
                )));
            }
        }

        let mut config = OfflineRecognizerConfig::default();
        config.model_config.sense_voice.model = Some(model.display().to_string());
        config.model_config.sense_voice.language = Some(map_source_lang(source_lang));
        config.model_config.sense_voice.use_itn = true; // 移植参考实现
        config.model_config.tokens = Some(tokens.display().to_string());
        config.model_config.num_threads = 4; // 移植参考实现
        config.model_config.debug = false;

        tracing::info!("加载 SenseVoice 模型: {}", dir.display());
        let recognizer = sherpa_onnx::OfflineRecognizer::create(&config).ok_or_else(|| {
            AppError::Message("SenseVoice 加载失败（onnx runtime 初始化错误）".into())
        })?;
        tracing::info!("SenseVoice 模型加载完成");
        Ok(Self { recognizer })
    }
}

impl ASREngine for SenseVoiceEngine {
    fn engine_name(&self) -> &'static str {
        "sense-voice"
    }

    fn transcribe(&self, samples_16k: &[f32]) -> AppResult<TranscriptCandidate> {
        let stream = self.recognizer.create_stream();
        stream.accept_waveform(crate::asr::vad::SAMPLE_RATE, samples_16k);
        self.recognizer.decode(&stream);
        let result = stream
            .get_result()
            .ok_or_else(|| AppError::Message("识别结果为空".into()))?;
        let raw = result.text.clone();
        let lang = detect_lang_tag(&raw).map(str::to_string);
        Ok(TranscriptCandidate { text: clean_sense_voice_text(&raw), lang })
    }
}

/// 源语言设置 → SenseVoice language 参数（auto 或语言代码）
fn map_source_lang(lang: &str) -> String {
    match lang {
        "" | "auto" => "auto".into(),
        "zh-TW" | "zh-CN" => "zh".into(),
        other => other.into(),
    }
}

/// 剥离 SenseVoice 输出中的特有标签（<|zh|>、<|NEUTRAL|>、<|speech|> 等）
/// —— 逐字移植参考实现 `clean_sense_voice_text`（docs/porting-notes.md §1.2）。
pub fn clean_sense_voice_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_tag = false;
    for ch in text.chars() {
        match ch {
            '<' => in_tag = true,
            '>' if in_tag => in_tag = false,
            c if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.trim().to_string()
}

/// 从原始输出（含标签）检出语言代码 —— 移植参考实现（docs/porting-notes.md §1.3）
pub fn detect_lang_tag(raw_text: &str) -> Option<&'static str> {
    if raw_text.contains("<|zh|>") || raw_text.contains("<|yue|>") {
        Some("zh")
    } else if raw_text.contains("<|en|>") {
        Some("en")
    } else if raw_text.contains("<|ja|>") {
        Some("ja")
    } else if raw_text.contains("<|ko|>") {
        Some("ko")
    } else {
        None
    }
}

/// Whisper 语言代码映射（后续引擎用）：auto→""，zh-TW/zh-CN→zh
pub fn map_whisper_lang(lang: &str) -> String {
    match lang {
        "auto" => String::new(),
        "zh-TW" | "zh-CN" => "zh".into(),
        other => other.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_tags_and_detects_lang() {
        let raw = "<|zh|><|NEUTRAL|><|speech|>你好世界<|withitn|>";
        assert_eq!(clean_sense_voice_text(raw), "你好世界");
        assert_eq!(detect_lang_tag(raw), Some("zh"));
        assert_eq!(detect_lang_tag("<|en|>hello"), Some("en"));
        assert_eq!(detect_lang_tag("no tags"), None);
    }

    #[test]
    fn whisper_lang_map() {
        assert_eq!(map_whisper_lang("auto"), "");
        assert_eq!(map_whisper_lang("zh-TW"), "zh");
        assert_eq!(map_whisper_lang("ja"), "ja");
    }

    #[test]
    fn source_lang_map() {
        assert_eq!(map_source_lang("auto"), "auto");
        assert_eq!(map_source_lang("zh-TW"), "zh");
        assert_eq!(map_source_lang("en"), "en");
    }
}
