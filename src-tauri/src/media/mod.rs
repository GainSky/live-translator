//! 媒体文件转写：解码（symphonia/ffmpeg）→ VAD 切句 → SenseVoice → 翻译队列。
//! 句段时间轴 = 文件内时间（SRT 导出即成视频字幕）。

pub mod decode;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::asr::vad::{Segmenter, VadParams};
use crate::asr::ASREngine;
use crate::error::{AppError, AppResult};
use crate::events::{
    FileProgressPayload, TranscriptPayload, EV_FILE_PROGRESS, EV_FILE_TRANSCRIPT,
};
use crate::pipeline::{PipelineManager, TranslateJob};
use crate::SettingsState;
use crate::store::SessionStore;

/// 媒体文件转写会话状态（同一时间一个文件任务；会话保留供导出）
#[derive(Default)]
pub struct MediaState {
    stop: Mutex<Option<Arc<AtomicBool>>>,
    session: Mutex<Option<Arc<SessionStore>>>,
}

impl MediaState {
    fn begin(&self, session: Arc<SessionStore>, stop: Arc<AtomicBool>) -> AppResult<()> {
        let mut s = self.session.lock().unwrap();
        let mut f = self.stop.lock().unwrap();
        if f.is_some() {
            return Err(AppError::Message(
                "已有文件转写任务在运行，请先取消或等待完成".into(),
            ));
        }
        *s = Some(session);
        *f = Some(stop);
        Ok(())
    }

    /// 当前文件会话（导出用；任务结束后保留）
    pub fn current(&self) -> Option<Arc<SessionStore>> {
        self.session.lock().unwrap().clone()
    }

    pub fn cancel(&self) {
        if let Some(f) = self.stop.lock().unwrap().take() {
            f.store(true, Ordering::SeqCst);
            tracing::info!("文件转写已请求取消");
        }
    }

    fn finish(&self) {
        *self.stop.lock().unwrap() = None;
    }
}

/// 文件转写会话信息（前端展示）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSessionInfo {
    pub path: String,
    pub session_id: String,
    pub segments: u64,
}

/// 启动文件转写（command transcribe_file 的实现体）
pub fn transcribe_file(
    app: &AppHandle,
    media: &MediaState,
    path: String,
) -> AppResult<MediaSessionInfo> {
    if !Path::new(&path).is_file() {
        return Err(AppError::Message(format!("文件不存在: {path}")));
    }
    let stop = Arc::new(AtomicBool::new(false));
    let data_dir = app.path().app_data_dir()?;
    let session = Arc::new(SessionStore::new(&data_dir));
    media.begin(session.clone(), stop.clone())?;

    // 快照设置（源语言 + VAD 参数）
    let settings = app.state::<SettingsState>();
    let (source_lang, vad_params) = {
        let s = settings.0.lock().unwrap();
        (s.asr.source_lang.clone(), s.asr.vad.clone())
    };
    let file_name = Path::new(&path)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| path.clone());

    let app2 = app.clone();
    let worker_path = path.clone();
    let worker_session = session.clone();
    std::thread::Builder::new()
        .name("file-transcribe".into())
        .spawn(move || {
            media_worker(
                app2, worker_path, file_name, worker_session, stop, source_lang, vad_params,
            );
        })
        .map_err(|e| AppError::Message(format!("转写线程启动失败: {e}")))?;

    Ok(MediaSessionInfo {
        path,
        session_id: session.session_id.clone(),
        segments: 0,
    })
}

fn emit_progress(
    app: &AppHandle,
    path: &str,
    phase: &str,
    decoded_secs: f64,
    total_secs: Option<f64>,
    segments: u64,
    message: Option<String>,
) {
    let _ = app.emit(
        EV_FILE_PROGRESS,
        FileProgressPayload {
            path: path.to_string(),
            phase: phase.to_string(),
            decoded_secs,
            total_secs,
            segments,
            message,
        },
    );
}

/// 媒体时间 HH:MM:SS
fn fmt_media_time(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:02}:{:02}:{:02}", s / 3600, (s % 3600) / 60, s % 60)
}

fn media_worker(
    app: AppHandle,
    path: String,
    file_name: String,
    session: Arc<SessionStore>,
    stop: Arc<AtomicBool>,
    source_lang: String,
    vad_params: VadParams,
) {
    let finish = |app: &AppHandle, phase: &str, msg: Option<String>| {
        emit_progress(app, &path, phase, 0.0, None, session.items().len() as u64, msg);
    };

    let result = run_media(
        &app, &path, &file_name, &session, &stop, &source_lang, &vad_params,
    );

    match result {
        Ok(()) => {
            if stop.load(Ordering::SeqCst) {
                tracing::info!("文件转写已取消: {path}");
                finish(&app, "canceled", Some("已取消".into()));
            } else {
                tracing::info!("文件转写完成: {path}");
                finish(&app, "done", None);
            }
        }
        Err(e) => {
            tracing::error!("文件转写失败: {path}: {e}");
            finish(&app, "error", Some(e.to_string()));
        }
    }
    // 清除运行标记（会话保留供导出）
    app.state::<MediaState>().finish();
}

fn run_media(
    app: &AppHandle,
    path: &str,
    file_name: &str,
    session: &Arc<SessionStore>,
    stop: &Arc<AtomicBool>,
    source_lang: &str,
    vad_params: &VadParams,
) -> AppResult<()> {
    emit_progress(app, path, "preparing", 0.0, None, 0, None);

    // 模型 + 切句器（与直播链路共享同一份已加载 SenseVoice）
    let model_root = crate::models::resolve_model_root(app)?;
    let engine = app
        .state::<PipelineManager>()
        .asr_engine()
        .get_or_load(&model_root, source_lang, |status, detail| {
            let _ = app.emit(
                crate::events::EV_ENGINE_STATE,
                crate::events::EngineStatePayload {
                    engine: "sense-voice".into(),
                    status,
                    detail,
                },
            );
        })?;
    let mut segmenter = Segmenter::new(vad_params, &model_root.join("silero_vad.onnx"))?;

    emit_progress(app, path, "decoding", 0.0, None, 0, None);
    let (mut decoder, total_secs) = decode::open(Path::new(path))?;
    emit_progress(
        app, path, "decoding", 0.0, total_secs, session.items().len() as u64, None,
    );

    let translate_queue = app.state::<PipelineManager>().translate_queue.clone();
    let mut decoded_samples: u64 = 0;
    let mut chunk: Vec<f32> = Vec::new();
    let mut last_emit = std::time::Instant::now();

    loop {
        if stop.load(Ordering::SeqCst) {
            return Ok(());
        }
        chunk.clear();
        let eof = decoder.next_chunk(&mut chunk)?;

        decoded_samples += chunk.len() as u64;
        for seg in segmenter.feed(&chunk) {
            if stop.load(Ordering::SeqCst) {
                return Ok(());
            }
            handle_file_segment(app, file_name, engine.as_ref(), &seg, session, &translate_queue);
        }

        // 进度节流（200ms）
        if last_emit.elapsed().as_millis() >= 200 {
            last_emit = std::time::Instant::now();
            emit_progress(
                app, path, "decoding",
                decoded_samples as f64 / 16_000.0,
                total_secs,
                session.items().len() as u64,
                None,
            );
        }
        if eof {
            break;
        }
    }
    for seg in segmenter.flush() {
        handle_file_segment(app, file_name, engine.as_ref(), &seg, session, &translate_queue);
    }
    emit_progress(
        app, path, "decoding",
        decoded_samples as f64 / 16_000.0,
        total_secs,
        session.items().len() as u64,
        None,
    );
    Ok(())
}

/// 单句：识别 → 中文简繁快路径 → 入会话/事件/翻译队列（镜像直播 handle_segment）
fn handle_file_segment(
    app: &AppHandle,
    file_name: &str,
    engine: &dyn ASREngine,
    seg: &crate::asr::vad::Segment,
    session: &Arc<SessionStore>,
    translate_queue: &Arc<Mutex<std::collections::VecDeque<TranslateJob>>>,
) {
    match engine.transcribe(&seg.samples) {
        Ok(cand) if !cand.text.is_empty() => {
            let cfg = app
                .state::<SettingsState>()
                .0
                .lock()
                .unwrap()
                .translation
                .clone();

            let mut translated_text = None;
            let mut provider = None;
            if cfg.enabled {
                if let Some(t) =
                    crate::translate::try_opencc_inline(&cand.text, cand.lang.as_deref(), &cfg)
                {
                    translated_text = Some(t);
                    provider = Some("opencc".into());
                }
            }

            let payload = TranscriptPayload {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: session.session_id.clone(),
                source_id: "file".into(),
                source_name: file_name.to_string(),
                start_ms: seg.start_ms,
                end_ms: seg.end_ms,
                clock_time: fmt_media_time(seg.start_ms),
                raw_text: cand.text.clone(),
                lang: cand.lang.clone(),
                translated_text: translated_text.clone(),
                provider: provider.clone(),
                asr_engine: Some("sense-voice".into()),
            };
            let _ = app.emit(EV_FILE_TRANSCRIPT, &payload);
            session.append(&payload);

            if cfg.enabled && translated_text.is_none() {
                translate_queue.lock().unwrap().push_back(TranslateJob {
                    session: Arc::clone(session),
                    payload,
                });
            }
        }
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("文件句段识别失败: {e}");
        }
    }
}
