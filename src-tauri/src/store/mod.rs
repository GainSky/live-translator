pub mod db;
pub mod export;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::Local;
use rusqlite::Connection;
use serde::Serialize;

use crate::events::TranscriptPayload;

/// 会话信息（UI 展示用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: String,
    pub start_clock: String,
}

/// 会话记录：SQLite（结构化检索）+ JSONL（容灾），双写（readme §5.4）
/// 线程安全：多路采集线程并发 append（Connection 互斥锁保护）
pub struct SessionStore {
    pub session_id: String,
    pub session_start: chrono::DateTime<chrono::Local>,
    conn: Mutex<Option<Connection>>,
    jsonl_path: Option<PathBuf>,
    /// 会话内全部转写（内存副本，导出用；与 SQLite/JSONL 同步追加）
    items: Mutex<Vec<TranscriptPayload>>,
}

impl SessionStore {
    pub fn new(data_dir: &Path) -> Self {
        let session_id = format!("s-{}", Local::now().format("%Y%m%d-%H%M%S"));
        let conn = db::init(&data_dir.join("live-translator.db")).ok();
        if conn.is_none() {
            tracing::warn!("SQLite 初始化失败，本次会话仅 JSONL 记录");
        }
        let jsonl_path = data_dir.join(format!("transcript_{session_id}.jsonl"));
        Self {
            session_id,
            session_start: Local::now(),
            conn: Mutex::new(conn),
            jsonl_path: Some(jsonl_path),
            items: Mutex::new(Vec::new()),
        }
    }

    /// 当前会话全部转写记录（内存副本，导出用）
    pub fn items(&self) -> Vec<TranscriptPayload> {
        self.items.lock().unwrap().clone()
    }

    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.session_id.clone(),
            start_clock: self.session_start.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    /// 追加一条转写记录（双写；单条失败仅记录日志，不影响流水线）
    pub fn append(&self, item: &TranscriptPayload) {
        self.items.lock().unwrap().push(item.clone());
        {
            let mut guard = self.conn.lock().unwrap();
            if let Some(conn) = guard.as_mut() {
                let r = conn.execute(
                    "INSERT INTO transcripts (id, session_id, source_id, source_name, start_ms, end_ms, clock_time, raw_text, lang, translated_text, provider, asr_engine)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
                    rusqlite::params![
                        item.id,
                        item.session_id,
                        item.source_id,
                        item.source_name,
                        item.start_ms as i64,
                        item.end_ms as i64,
                        item.clock_time,
                        item.raw_text,
                        item.lang,
                        item.translated_text,
                        item.provider,
                        item.asr_engine,
                    ],
                );
                if let Err(e) = r {
                    tracing::warn!("SQLite 写入失败: {e}");
                }
            }
        }
        if let Some(path) = &self.jsonl_path {
            let line = serde_json::to_string(item).unwrap_or_default();
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
                if writeln!(f, "{line}").is_err() {
                    tracing::warn!("JSONL 写入失败: {}", path.display());
                }
            }
        }
    }

    /// 回填翻译结果（M3 翻译队列完成后调用）
    pub fn update_translation(&self, id: &str, translated: &str, provider: &str) {
        // 先更新内存副本——导出与前端直读都以 items 为数据源；
        // 此前只写 DB，导致日志有翻译但导出/页面全是 null
        if let Ok(mut items) = self.items.lock() {
            if let Some(it) = items.iter_mut().find(|i| i.id == id) {
                it.translated_text = Some(translated.to_string());
                it.provider = Some(provider.to_string());
            }
        }
        // 再写 SQLite（与 append 相同的锁顺序：items → conn，防死锁）
        let mut guard = self.conn.lock().unwrap();
        if let Some(conn) = guard.as_mut() {
            let _ = conn.execute(
                "UPDATE transcripts SET translated_text = ?1, provider = ?2 WHERE id = ?3",
                rusqlite::params![translated, provider, id],
            );
        }
    }
}
