use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

use crate::error::{AppError, AppResult};

/// 模型清单条目（readme §5.5：首启下载 + 断点续传 + sha256 校验 + 便携模式）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: String,
    /// 多镜像（GitHub Releases / HF / 自建），按序尝试
    pub urls: Vec<String>,
    pub sha256: String,
    pub size_bytes: Option<u64>,
    /// 相对模型根目录的目标路径（tar.bz2 需解压后取指定文件，见 extract 规则）
    pub dest: String,
    pub required: bool,
}

pub fn load_manifest(models_dir: &Path) -> AppResult<Vec<ModelEntry>> {
    let path = models_dir.join("manifest.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| {
        AppError::Message(format!("读取模型清单失败（{}）: {e}", path.display()))
    })?;
    Ok(serde_json::from_str(&raw)?)
}

/// sha256 校验（首启下载完成后执行）
pub fn verify_sha256(path: &Path, expected_hex: &str) -> AppResult<bool> {
    let bytes = std::fs::read(path)?;
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let actual: String = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(actual.eq_ignore_ascii_case(expected_hex))
}

/// 解析模型根目录：环境变量覆盖 → 便携模式（exe 同目录 models/）→ 用户数据目录
pub fn resolve_model_root(app: &tauri::AppHandle) -> AppResult<std::path::PathBuf> {
    if let Ok(p) = std::env::var("LIVE_TRANSLATOR_MODELS_DIR") {
        return Ok(std::path::PathBuf::from(p));
    }
    if let Ok(exe) = std::env::current_exe() {
        let portable = exe.parent().unwrap_or(exe.as_path()).join("models");
        if portable.is_dir() {
            return Ok(portable);
        }
    }
    use tauri::Manager;
    Ok(app.path().app_data_dir()?.join("models"))
}

/// 下载模型（M5 实现：多镜像 + 断点续传 + 进度事件上报）
pub fn download(_entry: &ModelEntry, _models_dir: &Path) -> AppResult<PathBuf> {
    Err(AppError::NotImplemented("M5: 模型下载器".into()))
}
