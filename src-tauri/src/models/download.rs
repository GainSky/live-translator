//! 模型下载器：断点续传 + 进度事件 + sha256 校验 + tar.bz2 解压。
//!
//! - 每个模型同一时刻只允许一个下载任务（ACTIVE 去重）
//! - `.part` 半成品文件 + HTTP Range 续传；完成后原子改名
//! - 目录型模型（dest 以 / 结尾，如 SenseVoice 的 tar.bz2）下载后解压到 models/
//! - 进度经 `model:progress` 事件上报（200ms 节流）

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter};

use super::ModelEntry;
use crate::events::{ModelProgressPayload, EV_MODEL_PROGRESS};
use crate::error::{AppError, AppResult};

/// 同时只能有一个同名模型下载任务
fn active() -> &'static Mutex<HashSet<String>> {
    static ACTIVE: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    ACTIVE.get_or_init(|| Mutex::new(HashSet::new()))
}

/// 正在下载的模型 id（UI 状态查询用）
pub fn active_ids() -> HashSet<String> {
    active().lock().unwrap().clone()
}

fn emit_progress(app: &AppHandle, entry: &ModelEntry, state: &str, downloaded: u64, total: u64, error: Option<String>) {
    let _ = app.emit(
        EV_MODEL_PROGRESS,
        crate::events::ModelProgressPayload {
            id: entry.id.clone(),
            state: state.to_string(),
            downloaded,
            total,
            error,
        },
    );
}

/// 后台下载入口（在独立线程调用；命令立即返回）
pub fn download_in_background(app: AppHandle, models_dir: PathBuf, entry: ModelEntry) {
    let id = entry.id.clone();
    {
        let mut guard = active().lock().unwrap();
        if !guard.insert(id.clone()) {
            let _ = app.emit(
                EV_MODEL_PROGRESS,
                ModelProgressPayload {
                    id,
                    state: "error".into(),
                    downloaded: 0,
                    total: 0,
                    error: Some("该模型已在下载中".into()),
                },
            );
            return;
        }
    }
    let app2 = app.clone();
    std::thread::Builder::new()
        .name(format!("download:{}", entry.id))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_download(&app2, &models_dir, &entry)
            }));
            match result {
                Ok(Ok(())) => {
                    emit_progress(&app2, &entry, "done", 0, 0, None);
                    tracing::info!("模型下载完成: {}", entry.id);
                }
                Ok(Err(e)) => {
                    tracing::error!("模型下载失败（{}）: {e}", entry.id);
                    emit_progress(&app2, &entry, "error", 0, 0, Some(e.to_string()));
                }
                Err(_) => {
                    emit_progress(&app2, &entry, "error", 0, 0, Some("下载线程异常".into()));
                }
            }
            active().lock().unwrap().remove(&id);
        })
        .ok();
}

fn run_download(app: &AppHandle, models_dir: &Path, entry: &ModelEntry) -> AppResult<()> {
    let is_archive = entry.dest.ends_with('/');
    let url = entry
        .urls
        .first()
        .ok_or_else(|| AppError::Message("模型清单缺少下载地址".into()))?;
    let archive_name = url
        .split('?')
        .next()
        .and_then(|u| u.rsplit('/').next())
        .unwrap_or("model.bin");
    let final_path = if is_archive {
        models_dir.join(archive_name)
    } else {
        models_dir.join(&entry.dest)
    };
    let part_path = PathBuf::from(format!("{}.part", final_path.display()));

    emit_progress(app, entry, "downloading", 0, entry.size_bytes.unwrap_or(0), None);

    // 断点续传：.part 已有长度 → Range 请求
    let start = part_path.metadata().map(|m| m.len()).unwrap_or(0);
    let client = reqwest::blocking::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .build()?;
    let mut req = client.get(url);
    if start > 0 {
        tracing::info!("续传: {} 从 {} 字节继续", archive_name, start);
        req = req.header("Range", format!("bytes={start}-"));
    }
    let resp = req.send().map_err(|e| AppError::Message(format!("下载请求失败: {e}")))?;
    let status = resp.status();
    let mut downloaded = start;
    let file = if status.as_u16() == 206 {
        // 续传成功
        Some(std::fs::OpenOptions::new().append(true).open(&part_path)?)
    } else if status.as_u16() == 200 {
        // 服务端不支持 Range 或无半成品：从头下载
        downloaded = 0;
        Some(std::fs::File::create(&part_path)?)
    } else {
        return Err(AppError::Message(format!("下载失败: HTTP {status}")));
    };
    let mut file = file.ok_or_else(|| AppError::Message("下载文件打开失败".into()))?;

    let content_len = resp.content_length().unwrap_or(0);
    let total = if content_len > 0 { downloaded + content_len } else { entry.size_bytes.unwrap_or(0) };

    let mut last_emit = Instant::now() - Duration::from_secs(1);
    let mut buffer = [0u8; 64 * 1024];
    let mut reader = resp;
    loop {
        if stopping_requested(app) {
            return Err(AppError::Message("下载已取消".into()));
        }
        // blocking Response 实现 std::io::Read
        let n = std::io::Read::read(&mut reader, &mut buffer)
            .map_err(|e| AppError::Message(format!("下载中断: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buffer[..n])?;
        downloaded += n as u64;
        if last_emit.elapsed() >= Duration::from_millis(200) {
            last_emit = Instant::now();
            emit_progress(app, entry, "downloading", downloaded, total, None);
        }
    }
    file.flush()?;
    drop(file);

    emit_progress(app, entry, "verifying", downloaded, total, None);

    // sha256 校验（清单提供哈希时）
    if !entry.sha256.is_empty() {
        let ok = super::verify_sha256(&part_path, &entry.sha256)?;
        if !ok {
            let _ = std::fs::remove_file(&part_path);
            return Err(AppError::Message("sha256 校验失败，文件已删除，请重新下载".into()));
        }
    }

    // 归档：解压到 models 根目录（内部路径与代码引用一致）
    if is_archive {
        emit_progress(app, entry, "extracting", total, total, None);
        let f = std::fs::File::open(&part_path)?;
        let decoder = bzip2::read::BzDecoder::new(f);
        let mut archive = tar::Archive::new(decoder);
        archive
            .unpack(models_dir)
            .map_err(|e| AppError::Message(format!("解压失败: {e}")))?;
        let _ = std::fs::remove_file(&part_path);
    } else {
        std::fs::rename(&part_path, &final_path)?;
    }

    Ok(())
}

// 取消机制占位：当前无取消命令，恒为 false（取消后续版本提供）
fn stopping_requested(_app: &AppHandle) -> bool {
    false
}

