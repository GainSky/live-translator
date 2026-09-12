//! 内置本地翻译引擎：candle 纯 Rust 进程内推理（Qwen2.5 GGUF 量化模型）。
//!
//! 设计要点（"本地离线反应要快"）：
//! - 模型常驻进程内存（加载一次，全局 Hub 共享），无每次请求的加载/HTTP 开销
//! - 贪婪解码（temperature=0 等价），输出上限 max_new tokens，短系统提示减少 prefill
//! - 每句独立生成，句间 clear_kv_cache
//!
//! 模型文件（模型管理器下载，见 models/manifest.json）：
//!   qwen2.5-3b-instruct-q4-k-m.gguf + qwen2.5-tokenizer.json

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use candle_core::quantized::gguf_file;
use candle_core::{D, Device, DType, Tensor};
use candle_transformers::models::quantized_qwen2::ModelWeights;
use tokenizers::Tokenizer;

use crate::error::{AppError, AppResult};
use crate::translate::prompts;

/// 单句生成上限（字幕句短，96 tokens 足够且限制最坏延迟）
const MAX_NEW_TOKENS: usize = 96;

/// 默认 GGUF 模型文件名（models/ 目录内）
pub const DEFAULT_MODEL: &str = "qwen2.5-3b-instruct-q4_k_m.gguf";
/// tokenizer 文件名
pub const DEFAULT_TOKENIZER: &str = "qwen2.5-3b-tokenizer.json";

pub struct LocalEngine {
    model: ModelWeights,
    tokenizer: Tokenizer,
    device: Device,
    eos_ids: Vec<u32>,
    /// 已加载的模型文件名（模型切换检测用）
    pub model_name: String,
}

impl LocalEngine {
    pub fn load(model_root: &Path, model_name: &str, tokenizer_name: &str) -> AppResult<Self> {
        let gguf_path = model_root.join(model_name);
        let tok_path = model_root.join(tokenizer_name);
        for f in [&gguf_path, &tok_path] {
            if !f.is_file() {
                return Err(AppError::EngineUnavailable(format!(
                    "缺少本地翻译模型文件: {}（请在设置页下载，或参考 readme §10.2 手动放置）",
                    f.display()
                )));
            }
        }

        tracing::info!("加载内置翻译引擎: {}", gguf_path.display());
        let t0 = Instant::now();
        let device = Device::Cpu;
        let mut file = std::fs::File::open(&gguf_path)
            .map_err(|e| AppError::Message(format!("打开 GGUF 失败: {e}")))?;
        let content = gguf_file::Content::read(&mut file)
            .map_err(|e| AppError::Message(format!("解析 GGUF 失败: {e}")))?;
        let model = ModelWeights::from_gguf(content, &mut file, &device)
            .map_err(|e| AppError::Message(format!("模型权重加载失败: {e}")))?;
        let tokenizer = Tokenizer::from_file(&tok_path)
            .map_err(|e| AppError::Message(format!("加载 tokenizer 失败: {e}")))?;
        // Qwen2.5 的 <|im_end|>（ChatML 结束符）；缺失时回退官方词表 id
        let eos_ids = vec![
            tokenizer.token_to_id("<|im_end|>").unwrap_or(151645),
            tokenizer.token_to_id("<|endoftext|>").unwrap_or(151643),
        ];
        tracing::info!("内置翻译引擎就绪（耗时 {:.1}s）", t0.elapsed().as_secs_f32());
        Ok(Self {
            model,
            tokenizer,
            device,
            eos_ids,
            model_name: model_name.to_string(),
        })
    }


    /// 翻译一句（贪婪解码，独立会话）
    pub fn translate(&mut self, text: &str, target_lang: &str) -> AppResult<String> {
        let target_name = prompts::lang_display_name(target_lang);
        let out = self.generate(text, target_name, MAX_NEW_TOKENS)?;
        Ok(post_process(out))
    }

    fn generate(&mut self, text: &str, target_name: &str, max_new: usize) -> AppResult<String> {
        let system = prompts::render_builtin_system(target_name);
        let prompt = format!(
            "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{text}<|im_end|>\n<|im_start|>assistant\n"
        );
        #[cfg(debug_assertions)]
        crate::translate::diag::log("builtin", "request", &format!("prompt={prompt}"));

        let prompt_ids: Vec<u32> = self
            .tokenizer
            .encode(prompt, true)
            .map_err(|e| AppError::Message(format!("分词失败: {e}")))?
            .get_ids()
            .to_vec();

        let mut all: Vec<u32> = prompt_ids.clone();
        let mut generated: Vec<u32> = Vec::new();
        let mut cur_pos: usize = 0; // 下一步输入首 token 的绝对位置
        let mut next: Option<u32> = None;

        for _ in 0..max_new {
            let (input_ids, index_pos) = match next {
                None => (all.clone(), 0usize), // prefill：整个 prompt
                Some(tok) => (vec![tok], cur_pos), // decode：上一 token @ cur_pos
            };
            let input = Tensor::new(&input_ids[..], &self.device)
                .map_err(|e| AppError::Message(format!("构建输入失败: {e}")))?
                .unsqueeze(0)
                .map_err(|e| AppError::Message(format!("构建输入失败: {e}")))?;
            let logits = self
                .model
                .forward(&input, index_pos)
                .map_err(|e| AppError::Message(format!("前向推理失败: {e}")))?
                .squeeze(0)
                .map_err(|e| AppError::Message(format!("前向推理失败: {e}")))?
                .to_dtype(DType::F32)
                .map_err(|e| AppError::Message(format!("前向推理失败: {e}")))?;
            cur_pos = index_pos + input_ids.len();

            let tok = logits
                .argmax(D::Minus1)
                .map_err(|e| AppError::Message(format!("采样失败: {e}")))?
                .to_scalar::<u32>()
                .map_err(|e| AppError::Message(format!("采样失败: {e}")))?;
            if self.eos_ids.contains(&tok) {
                break;
            }
            all.push(tok);
            generated.push(tok);
            next = Some(tok);
        }
        // KV cache 无需手动清理：每句 prefill 从 index_pos=0 开始时，
        // forward_attn 内部会用新 (k,v) 覆盖缓存（见 candle quantized_qwen2 实现）
        let text = self
            .tokenizer
            .decode(&generated, true)
            .map_err(|e| AppError::Message(format!("解码失败: {e}")))?;
        #[cfg(debug_assertions)]
        crate::translate::diag::log(
            "builtin",
            "response",
            &format!("tokens={} raw={text}", generated.len()),
        );
        Ok(text)
    }
}

/// 译文后处理：剥解释性前后缀、压平换行（移植参考实现思路）
fn post_process(text: String) -> String {
    let mut t = text.trim().to_string();
    for marker in ["译文：", "译文:", "翻译：", "翻译:"] {
        if let Some(idx) = t.find(marker) {
            t = t[idx + marker.len()..].trim().to_string();
        }
    }
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ===== 全局 Hub（worker / 测试命令 / smoke 共享，进程内只加载一份） =====

#[derive(Clone)]
pub struct EngineHub {
    inner: Arc<Mutex<HubInner>>,
}

struct HubInner {
    loaded: Option<(String, Arc<Mutex<LocalEngine>>)>,
}

impl Default for EngineHub {
    fn default() -> Self {
        Self { inner: Arc::new(Mutex::new(HubInner { loaded: None })) }
    }
}

impl EngineHub {
    /// 按需加载（模型名变更时自动重载）；返回共享句柄（Mutex 保护生成过程）
    pub fn get_or_load(
        &self,
        model_root: &Path,
        model_name: &str,
        tokenizer_name: &str,
        on_state: impl Fn(&str, Option<String>),
    ) -> AppResult<Arc<Mutex<LocalEngine>>> {
        let mut inner = self.inner.lock().unwrap();
        if let Some((name, engine)) = inner.loaded.as_ref() {
            if name == model_name {
                return Ok(engine.clone());
            }
            tracing::info!("翻译模型切换: {name} -> {model_name}");
            inner.loaded = None;
        }
        on_state("loading", None);
        let engine = Arc::new(Mutex::new(LocalEngine::load(
            model_root,
            model_name,
            tokenizer_name,
        )?));
        on_state("ready", None);
        inner.loaded = Some((model_name.to_string(), engine.clone()));
        Ok(engine)
    }

    /// 卸载内置引擎（空闲释放）
    pub fn unload(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.loaded.take().is_some() {
                tracing::info!("内置翻译引擎已卸载");
            }
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.inner.lock().unwrap().loaded.is_some()
    }

}
