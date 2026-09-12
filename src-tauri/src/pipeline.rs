use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use chrono::Local;
use cpal::traits::StreamTrait;
use tauri::{AppHandle, Emitter, Manager};

use crate::asr::sense_voice::SenseVoiceEngine;
use crate::asr::vad::{Segmenter, VadParams};
use crate::asr::{EngineHub, ASREngine};
use crate::audio;
use crate::events::{
    AudioLevelPayload, EngineStatePayload, TranscriptPayload, EV_AUDIO_LEVEL, EV_ENGINE_STATE,
    EV_TRANSCRIPT_NEW,
};
use crate::error::{AppError, AppResult};
use crate::settings::Settings;
use crate::store::{SessionInfo, SessionStore};
use crate::SettingsState;

/// 采集/识别/翻译流水线（每路音频源一个独立线程；识别引擎全进程共享一份模型）
#[derive(Default)]
pub struct PipelineManager {
    /// 运行表：线程退出（含应用流消失导致的 Device disconnected）时自清理，
    /// 故用 Arc 共享给采集线程
    sources: Arc<Mutex<HashMap<String, SourceHandle>>>,
    engine: EngineHub,
    /// 当前会话（从空闲启动时创建；停止后保留供 UI/导出，下次启动替换）
    session: Mutex<Option<Arc<SessionStore>>>,
    /// 翻译队列 worker 的停止标志
    translate_worker: Mutex<Option<Arc<AtomicBool>>>,
    /// 翻译队列发送端（None = 未启用翻译）
    translate_tx: Mutex<Option<mpsc::Sender<TranslateJob>>>,
    /// 内置翻译引擎 Hub
    pub(crate) translate_hub: crate::translate::local_engine::EngineHub,
}

struct SourceHandle {
    stop: Arc<AtomicBool>,
}

impl PipelineManager {
    /// 启动选定音频源的转写流水线
    pub fn start(&self, app: &AppHandle, selected: Vec<String>, settings: &Settings) -> AppResult<()> {
        if selected.is_empty() {
            return Err(AppError::Message("未选择音频源".into()));
        }
        let model_root = crate::models::resolve_model_root(app)?;
        let vad_model_path = model_root.join("silero_vad.onnx");

        // 引擎按需加载（多路共享，状态上报 UI）
        let engine = self.engine.get_or_load(&model_root, &settings.asr.source_lang, |status, detail| {
            let _ = app.emit(
                EV_ENGINE_STATE,
                EngineStatePayload { engine: "sense-voice".into(), status, detail },
            );
        })?;

        let mut sources = self.sources.lock().unwrap();
        // 空闲启动 → 新会话（双写 SQLite + JSONL）
        if sources.is_empty() {
            let data_dir = app.path().app_data_dir()?;
            let store = Arc::new(SessionStore::new(&data_dir));
            tracing::info!("新会话: {}（记录于 {}）", store.session_id, data_dir.display());
            *self.session.lock().unwrap() = Some(store);
        }
        let session = self
            .session
            .lock()
            .unwrap()
            .clone()
            .expect("会话已在上文创建");

        // 翻译队列：启用时启动 worker（含内置引擎预热）
        // 注意：传模型目录 model_root，而非 vad_model_path（VAD 是 models/ 下的一个文件，
        // 曾误传导致内置引擎在 silero_vad.onnx 下找 GGUF → 预热失败）
        ensure_translate_worker(self, app, &model_root, settings.translation.enabled);

        for id in selected {
            if sources.contains_key(&id) {
                continue;
            }
            let opened = audio::resolve_source(&id)?;
            let source_name = opened.desc.name.clone();
            let stop_flag = Arc::new(AtomicBool::new(false));
            let stop_thread = stop_flag.clone();
            let app2 = app.clone();
            let engine2 = engine.clone();
            let vad_params = settings.asr.vad.clone();
            let vad_model = vad_model_path.clone();
            let session2 = session.clone();
            let sources_map = self.sources.clone();
            let thread_id = id.clone();
            let translate_tx = self.translate_tx.lock().unwrap().clone();
            std::thread::Builder::new()
                .name(format!("cap:{source_name}"))
                .spawn(move || {
                    run_source(
                        app2, opened, engine2, vad_params, vad_model, session2,
                        translate_tx, stop_thread, sources_map, thread_id,
                    )
                })
                .map_err(|e| AppError::Message(format!("采集线程启动失败: {e}")))?;
            sources.insert(id, SourceHandle { stop: stop_flag });
            tracing::info!("流水线已启动: {}", source_name);
        }
        Ok(())
    }

    /// 当前会话信息（UI 展示）
    pub fn current_session(&self) -> Option<SessionInfo> {
        self.session.lock().unwrap().as_ref().map(|s| s.info())
    }

    /// 停止全部流水线（线程退出时释放采集流与 VAD；模型保留供下次快速启动）
    pub fn stop_all(&self) {
        let mut sources = self.sources.lock().unwrap();
        for (id, h) in sources.drain() {
            h.stop.store(true, Ordering::SeqCst);
            tracing::info!("流水线停止: {id}");
        }
        drop(sources);
        // 停止翻译 worker
        if let Some(flag) = self.translate_worker.lock().unwrap().take() {
            flag.store(true, Ordering::SeqCst);
        }
        self.translate_tx.lock().unwrap().take();
    }

    pub fn running_sources(&self) -> Vec<String> {
        self.sources.lock().unwrap().keys().cloned().collect()
    }

    pub fn engine_loaded(&self) -> bool {
        self.engine.is_loaded()
    }

    /// 空闲卸载（M2 接入定时器；docs/porting-notes.md §4）
    pub fn unload_engine(&self) {
        self.engine.unload();
    }
}

/// 翻译队列任务（附带所属会话，回填时写入正确会话）
pub struct TranslateJob {
    pub session: Arc<SessionStore>,
    pub payload: TranscriptPayload,
}

/// 启动/停止翻译队列 worker（设置禁用时确保停止）
fn ensure_translate_worker(
    manager: &PipelineManager,
    app: &AppHandle,
    model_root: &PathBuf,
    enabled: bool,
) {
    // 停旧 worker
    if let Some(flag) = manager.translate_worker.lock().unwrap().take() {
        flag.store(true, Ordering::SeqCst);
    }
    manager.translate_tx.lock().unwrap().take(); // 断开通道 → worker 退出
    if !enabled {
        return;
    }
    let (tx, rx) = mpsc::channel::<TranslateJob>();
    let stop_worker = Arc::new(AtomicBool::new(false));
    let stop_thread = stop_worker.clone();
    let app2 = app.clone();
    let model_root2 = model_root.clone();
    let hub = manager.translate_hub.clone();
    if let Ok(h) = std::thread::Builder::new().name("translate".into()).spawn(move || {
        translation_worker(app2, rx, stop_thread, model_root2, hub);
    }) {
        *manager.translate_worker.lock().unwrap() = Some(stop_worker);
        *manager.translate_tx.lock().unwrap() = Some(tx);
        tracing::info!("翻译队列 worker 已启动");
    }
}

/// 翻译 worker：串行消费队列（LLM 一次一句），空闲时保活内置引擎
fn translation_worker(
    app: AppHandle,
    rx: mpsc::Receiver<TranslateJob>,
    stop: Arc<AtomicBool>,
    model_root: PathBuf,
    hub: crate::translate::local_engine::EngineHub,
) {
    use crate::events::{TranslateStatePayload, EV_TRANSLATE_STATE};
    use tauri::Manager;

    let mut state = crate::translate::DegradationState::default();
    let emit_state = |status: &str, detail: Option<String>| {
        let _ = app.emit(EV_TRANSLATE_STATE, TranslateStatePayload {
            status: status.to_string(),
            detail,
        });
    };

    // 启动即就绪：内置引擎在首句翻译时才加载（~8s，届时上报 loading→ok）
    emit_state("ok", None);

    loop {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        match rx.recv_timeout(Duration::from_secs(45)) {
            Ok(job) => {
                let cfg = app.state::<SettingsState>().0.lock().unwrap().translation.clone();
                if !cfg.enabled {
                    continue; // 运行中被关闭
                }
                if cfg.provider == "builtin" && !hub.is_loaded() {
                    emit_state("loading", Some("首次使用需加载翻译模型（约 8s）".into()));
                }
                match tauri::async_runtime::block_on(crate::translate::translate_with_fallback(
                    &job.payload.raw_text,
                    job.payload.lang.as_deref(),
                    &cfg,
                    &mut state,
                    &model_root,
                    &hub,
                )) {
                    Ok(outcome) if outcome.provider != "none" => {
                        emit_state("ok", Some(format!("来源: {}", outcome.provider)));
                        let _ = app.emit(
                            crate::events::EV_TRANSCRIPT_UPDATE,
                            crate::events::TranscriptUpdatePayload {
                                id: job.payload.id.clone(),
                                translated_text: outcome.text.clone(),
                                provider: outcome.provider.clone(),
                            },
                        );
                        job.session.update_translation(
                            &job.payload.id,
                            &outcome.text,
                            &outcome.provider,
                        );
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!("翻译失败: {e}");
                        emit_state("error", Some(e.to_string()));
                    }
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                // 保活：本地 HTTP 服务 45s 无任务时发 1-token 请求防卸载
                let cfg = app.state::<SettingsState>().0.lock().unwrap().translation.clone();
                if cfg.enabled && cfg.provider == "local-http" && !state.local_offline {
                    let system = crate::translate::prompts::render_local_system(&cfg.target_lang);
                    let base = cfg.local.base_url.clone();
                    let model = cfg.local.model.clone();
                    let app2 = app.clone();
                    let _ = tauri::async_runtime::spawn(async move {
                        let _ = crate::translate::openai_compat::chat(
                            &base, "", &model, 0.2, 1, 20, &system, "1", "keep-alive",
                        )
                        .await;
                        let _ = app2; // 保持句柄
                    });
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    // worker 退出不清空状态：下次启动会重新上报；避免前端残留「待机」误导
    tracing::info!("翻译队列 worker 退出");
}

fn emit_error(app: &AppHandle, source_id: Option<&str>, message: String) {
    tracing::error!("流水线错误: {message}");
    let _ = app.emit(
        crate::events::EV_PIPELINE_ERROR,
        crate::events::PipelineErrorPayload { source_id: source_id.map(str::to_string), message },
    );
}

fn run_source(
    app: AppHandle,
    source: audio::OpenedSource,
    engine: Arc<SenseVoiceEngine>,
    vad_params: VadParams,
    vad_model_path: PathBuf,
    session: Arc<SessionStore>,
    translate_tx: Option<mpsc::Sender<TranslateJob>>,
    stop: Arc<AtomicBool>,
    sources_map: Arc<Mutex<HashMap<String, SourceHandle>>>,
    source_id: String,
) {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    let stream = match audio::capture::build_input_stream(&source.device, tx, stop.clone()) {
        Ok(s) => s,
        Err(e) => {
            emit_error(&app, Some(&source.desc.id), e.to_string());
            return;
        }
    };
    if let Err(e) = stream.stream.play() {
        emit_error(&app, Some(&source.desc.id), e.to_string());
        return;
    }
    tracing::info!("采集启动: {} ({}Hz, {}ch)", source.desc.name, stream.native_rate, source.desc.channels);

    let mut resampler = audio::resample::Resampler::new(stream.native_rate, 16_000);
    let mut segmenter = match Segmenter::new(&vad_params, &vad_model_path) {
        Ok(s) => s,
        Err(e) => {
            emit_error(&app, Some(&source.desc.id), e.to_string());
            return;
        }
    };
    let mut level_tick = 0u32;

    while !stop.load(Ordering::SeqCst) {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(chunk) => {
                let mono16k = resampler.process(&chunk);
                level_tick += 1;
                if level_tick % 3 == 0 {
                    let peak = chunk.iter().fold(0.0f32, |m, &s| m.max(s.abs()));
                    let _ = app.emit(
                        EV_AUDIO_LEVEL,
                        AudioLevelPayload { source_id: source.desc.id.clone(), peak: peak.min(1.0) },
                    );
                }
                for seg in segmenter.feed(&mono16k) {
                    handle_segment(&app, &source.desc, &engine, &seg, &session, &translate_tx);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    for seg in segmenter.flush() {
        handle_segment(&app, &source.desc, &engine, &seg, &session, &translate_tx);
    }
    drop(stream); // 采集流随线程退出释放
    // 运行表自清理（应用流消失导致的自然退出同样适用，便于该源可再次启动）
    sources_map.lock().unwrap().remove(&source_id);
    tracing::info!("采集停止: {}", source.desc.name);
}

fn handle_segment(
    app: &AppHandle,
    desc: &audio::AudioDeviceDescriptor,
    engine: &SenseVoiceEngine,
    seg: &crate::asr::vad::Segment,
    session: &Arc<SessionStore>,
    translate_tx: &Option<mpsc::Sender<TranslateJob>>,
) {
    match engine.transcribe(&seg.samples) {
        Ok(cand) if !cand.text.is_empty() => {
            // 读取实时翻译设置（设置变更对下一句即时生效）
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
                // 快路径：中文↔简繁 → OpenCC 即时转换（零 LLM 消耗、亚毫秒）
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
                source_id: desc.id.clone(),
                source_name: desc.name.clone(),
                start_ms: seg.start_ms,
                end_ms: seg.end_ms,
                clock_time: Local::now().format("%H:%M:%S").to_string(),
                raw_text: cand.text.clone(),
                lang: cand.lang.clone(),
                translated_text: translated_text.clone(),
                provider: provider.clone(),
                asr_engine: Some("sense-voice".into()),
            };
            let _ = app.emit(EV_TRANSCRIPT_NEW, &payload);
            session.append(&payload); // SQLite + JSONL 双写（含已得译文）

            // 异步入队：LLM/远程翻译完成后 transcript:update 回填
            if cfg.enabled && translated_text.is_none() {
                if let Some(tx) = translate_tx {
                    let _ = tx.send(TranslateJob {
                        session: Arc::clone(session),
                        payload,
                    });
                }
            }
        }
        Ok(_) => {} // 空句（如纯噪音）
        Err(e) => tracing::warn!("识别失败（{}）: {e}", desc.name),
    }
}
