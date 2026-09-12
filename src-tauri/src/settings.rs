use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::AppResult;

/// 应用设置（持久化为 <app_data_dir>/settings.json）
/// 字段命名与前端 src/types/index.ts 一一对应（camelCase）
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub asr: AsrSettings,
    pub translation: TranslationSettings,
    pub appearance: AppearanceSettings,
    pub advanced: AdvancedSettings,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AsrSettings {
    pub engine: String,
    pub source_lang: String,
    pub vad: crate::asr::vad::VadParams,
}

impl Default for AsrSettings {
    fn default() -> Self {
        Self {
            engine: "sense-voice".into(),
            source_lang: "auto".into(),
            vad: Default::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TranslationSettings {
    pub enabled: bool,
    pub target_lang: String,
    /// "builtin"（内置 candle 引擎·离线）| "openai-compatible"（远程）| "local-http"（Ollama 等）
    pub provider: String,
    pub builtin: BuiltinConfig,
    pub openai: OpenAiConfig,
    pub local: LocalConfig,
    pub google_fallback: bool,
}

impl Default for TranslationSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            target_lang: "zh-CN".into(),
            provider: "builtin".into(),
            builtin: Default::default(),
            openai: Default::default(),
            local: Default::default(),
            google_fallback: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BuiltinConfig {
    /// models/ 目录下的 GGUF 文件名
    pub model: String,
    /// models/ 目录下的 tokenizer 文件名
    pub tokenizer: String,
}

impl Default for BuiltinConfig {
    fn default() -> Self {
        Self {
            model: crate::translate::local_engine::DEFAULT_MODEL.into(),
            tokenizer: crate::translate::local_engine::DEFAULT_TOKENIZER.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OpenAiConfig {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub temperature: f32,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        Self {
            base_url: "https://api.deepseek.com/v1".into(),
            api_key: String::new(),
            model: "deepseek-chat".into(),
            temperature: 0.3,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct LocalConfig {
    /// Ollama 的 OpenAI 兼容端点（llama.cpp server / LM Studio 同理）
    pub base_url: String,
    pub model: String,
}

impl Default for LocalConfig {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434/v1".into(),
            model: "qwen2.5:7b-instruct".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppearanceSettings {
    /// "system" | "light" | "dark"
    pub theme: String,
    pub show_translated: bool,
    pub main_font: FontPref,
    pub overlay_font: FontPref,
    /// "both" | "raw" | "translated"
    pub overlay_mode: String,
}

impl Default for AppearanceSettings {
    fn default() -> Self {
        Self {
            theme: "system".into(),
            show_translated: true,
            main_font: Default::default(),
            overlay_font: FontPref { family: "system-ui".into(), size: 22 },
            overlay_mode: "both".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct FontPref {
    pub family: String,
    /// px
    pub size: u32,
}

impl Default for FontPref {
    fn default() -> Self {
        Self { family: "system-ui".into(), size: 15 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AdvancedSettings {
    /// 空闲 N 分钟后自动卸载模型（移植自参考实现 600s 策略，见 docs/porting-notes.md §4）
    pub idle_unload_minutes: u64,
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self { idle_unload_minutes: 10 }
    }
}

/// 设置文件目录（平台差异，readme §10.2）：
/// - Windows：与 exe 同目录（便携模式，随单文件走）
/// - Linux/macOS：~/.config/{identifier}（XDG 约定）
pub fn settings_dir(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let _ = app; // 便携模式不依赖 app 上下文
        let exe = std::env::current_exe()?;
        let dir = exe
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| std::path::PathBuf::from("."));
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
    #[cfg(not(target_os = "windows"))]
    {
        use tauri::Manager;
        let dir = app.path().app_config_dir()?;
        std::fs::create_dir_all(&dir)?;
        Ok(dir)
    }
}

/// 载入设置；文件缺失或损坏时返回默认值。
/// 兼容迁移：新位置（config_dir）无文件时回读旧位置（data_dir）。
pub fn load_with_migration(config_dir: &Path, legacy_dir: &Path) -> Settings {
    let new_path = config_dir.join("settings.json");
    if !new_path.exists() {
        let legacy_path = legacy_dir.join("settings.json");
        if legacy_path.exists() {
            match std::fs::read_to_string(&legacy_path) {
                Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
                    Ok(s) => {
                        let _ = save(config_dir, &s);
                        tracing::info!("设置已从 {} 迁移至 {}", legacy_path.display(), new_path.display());
                        return s;
                    }
                    Err(_) => {}
                },
                Err(_) => {}
            }
        }
    }
    load(config_dir)
}

/// 载入设置；文件缺失或损坏时返回默认值
pub fn load(dir: &Path) -> Settings {
    let path = dir.join("settings.json");
    let mut s = match std::fs::read_to_string(&path) {
        Ok(raw) => match serde_json::from_str::<Settings>(&raw) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("settings.json 解析失败，使用默认设置: {e}");
                Settings::default()
            }
        },
        Err(_) => Settings::default(),
    };
    // 迁移：旧版 "local-llm"（Ollama HTTP）→ 新名 "local-http"
    if s.translation.provider == "local-llm" {
        s.translation.provider = "local-http".into();
    }
    s
}

pub fn save(dir: &Path, s: &Settings) -> AppResult<()> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join("settings.json");
    std::fs::write(path, serde_json::to_string_pretty(s)?)?;
    Ok(())
}
