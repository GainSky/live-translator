use rusqlite::Connection;

use crate::error::AppResult;

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS transcripts (
    id              TEXT PRIMARY KEY,
    session_id      TEXT NOT NULL,
    source_id       TEXT NOT NULL,
    source_name     TEXT NOT NULL,
    start_ms        INTEGER NOT NULL,
    end_ms          INTEGER NOT NULL,
    clock_time      TEXT NOT NULL,
    raw_text        TEXT NOT NULL,
    lang            TEXT,
    translated_text TEXT,
    provider        TEXT,
    asr_engine      TEXT,
    created_at      TEXT NOT NULL DEFAULT (datetime('now'))
);
CREATE INDEX IF NOT EXISTS idx_transcripts_session ON transcripts(session_id);
"#;

/// 打开（或创建）会话数据库并确保 schema 存在
pub fn init(db_path: &std::path::Path) -> AppResult<Connection> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(db_path)?;
    conn.execute_batch(SCHEMA)?;
    Ok(conn)
}
