pub mod download;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

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

/// 单个模型的状态（设置页模型管理用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub required: bool,
    pub exists: bool,
    pub size_bytes: Option<u64>,
    pub size_on_disk: Option<u64>,
    pub dest: String,
    pub downloading: bool,
}

/// 列出清单模型及其磁盘状态
pub fn list_models(models_dir: &Path) -> AppResult<Vec<ModelInfo>> {
    let entries = load_manifest(models_dir)?;
    let active = download::active_ids();
    let mut out = Vec::new();
    for e in &entries {
        let dest_path = models_dir.join(&e.dest);
        let (exists, size_on_disk) = if e.dest.ends_with('/') {
            (dest_path.is_dir(), None)
        } else {
            let m = std::fs::metadata(&dest_path).ok();
            (m.is_some(), m.map(|m| m.len()))
        };
        out.push(ModelInfo {
            id: e.id.clone(),
            name: e.name.clone(),
            required: e.required,
            exists,
            size_bytes: e.size_bytes,
            size_on_disk,
            dest: e.dest.clone(),
            downloading: active.contains(&e.id),
        });
    }
    Ok(out)
}

/// 下载模型（后台线程执行，进度经 model:progress 事件上报）
pub fn download_in_background(app: tauri::AppHandle, models_dir: &Path, id: &str) -> AppResult<()> {
    let entries = load_manifest(models_dir)?;
    let entry = entries
        .into_iter()
        .find(|e| e.id == id)
        .ok_or_else(|| AppError::Message(format!("清单中不存在模型: {id}")))?;
    download::download_in_background(app, models_dir.to_path_buf(), entry);
    Ok(())
}
