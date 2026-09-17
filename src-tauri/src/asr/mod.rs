pub mod sense_voice;
pub mod vad;

use serde::Serialize;
use std::path::Path;
use std::sync::{Arc, Mutex};

use crate::error::AppResult;
use sense_voice::SenseVoiceEngine;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EngineStatus {
    Unloaded,
    Loading,
    Ready,
    /// GPU 初始化失败自动降级 CPU（移植自参考实现策略，docs/porting-notes.md §1.2）
    FallbackCpu,
    Error,
}

/// 一句识别结果（清洗后的原文 + 检出语言）
pub struct TranscriptCandidate {
    pub text: String,
    pub lang: Option<String>,
}

/// ASR 引擎抽象（预留：sense-voice / whisper / streaming-zipformer）
/// decode 为 &self，支持多路共享同一份已加载模型
pub trait ASREngine: Send + Sync {
    fn engine_name(&self) -> &'static str;
    /// 输入 16kHz 单声道 f32（一个完整 VAD 句段），返回识别文本
    fn transcribe(&self, samples_16k: &[f32]) -> AppResult<TranscriptCandidate>;
}

/// 引擎共享器：全进程只加载一份模型，多路音频源复用（省内存）。
/// 记录加载时的源语言——设置变更后下次转写自动重载（语言 token 在模型加载时注入）
pub struct EngineHub {
    inner: Mutex<Option<LoadedEngine>>,
}

struct LoadedEngine {
    source_lang: String,
    engine: Arc<SenseVoiceEngine>,
}

impl Default for EngineHub {
    fn default() -> Self {
        Self { inner: Mutex::new(None) }
    }
}

impl EngineHub {
    /// 按需加载；源语言与已加载不一致时自动重载（约 3s，日志可见）
    pub fn get_or_load(
        &self,
        model_root: &Path,
        source_lang: &str,
        on_state: impl Fn(EngineStatus, Option<String>),
    ) -> AppResult<Arc<SenseVoiceEngine>> {
        let mut guard = self.inner.lock().unwrap();
        if let Some(loaded) = guard.as_ref() {
            if loaded.source_lang == source_lang {
                return Ok(loaded.engine.clone());
            }
            tracing::info!(
                "源语言变更: {} -> {}，重载识别引擎",
                loaded.source_lang,
                source_lang
            );
            guard.take();
        }
        on_state(EngineStatus::Loading, None);
        match SenseVoiceEngine::load(model_root, source_lang) {
            Ok(engine) => {
                let arc = Arc::new(engine);
                *guard = Some(LoadedEngine {
                    source_lang: source_lang.to_string(),
                    engine: arc.clone(),
                });
                on_state(EngineStatus::Ready, None);
                Ok(arc)
            }
            Err(e) => {
                on_state(EngineStatus::Error, Some(e.to_string()));
                Err(e)
            }
        }
    }

    /// 释放模型（空闲卸载策略，docs/porting-notes.md §4）
    pub fn unload(&self) {
        let mut guard = self.inner.lock().unwrap();
        if guard.take().is_some() {
            tracing::info!("ASR 模型已卸载");
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }
}

/// 兼容旧调用点的便捷加载（测试/CLI 用）
pub fn load_engine(model_root: &Path, source_lang: &str) -> AppResult<SenseVoiceEngine> {
    SenseVoiceEngine::load(model_root, source_lang)
}
