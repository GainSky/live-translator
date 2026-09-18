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

use serde::Serialize;

/// 采集/识别/翻译流水线（每路音频源一个独立线程；识别引擎全进程共享一份模型）
#[derive(Default)]
pub struct PipelineManager {
    /// 运行表：线程退出（含应用流消失导致的 Device disconnected）时自清理，
    /// 故用 Arc 共享给采集线程
    sources: Arc<Mutex<HashMap<String, SourceHandle>>>,
    engine: EngineHub,
    /// 当前会话（从空闲启动时创建；停止后保留供 UI/导出，下次启动替换）
    session: Mutex<Option<Arc<SessionStore>>>,
    /// 翻译队列（独立于转写运行：转写停止后队列继续消化，UI 可取消/清空）
    pub(crate) translate_queue: Arc<Mutex<std::collections::VecDeque<TranslateJob>>>,
    /// 内置翻译引擎 Hub
    pub(crate) translate_hub: crate::translate::local_engine::EngineHub,
    /// 翻译模型目录（worker 惰性使用）
    translate_model_root: Mutex<Option<PathBuf>>,
    /// 最近一次活动（开始/停止转写），空闲卸载计时用
    last_activity: Mutex<Option<std::time::Instant>>,
    /// 看门狗线程只启动一次
    watchdog_started: AtomicBool,
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

        // 翻译模型目录记录（worker 惰性读取；注意传目录本身而非 vad 文件路径）
        *self.translate_model_root.lock().unwrap() = Some(model_root.clone());

        // 空闲卸载看门狗（进程内仅启动一次）
        *self.last_activity.lock().unwrap() = Some(std::time::Instant::now());
        if !self.watchdog_started.swap(true, Ordering::SeqCst) {
            let app2 = app.clone();
            std::thread::Builder::new()
                .name("idle-watchdog".into())
                .spawn(move || idle_watchdog(app2))
                .ok();
        }

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
            let translate_queue = self.translate_queue.clone();
            std::thread::Builder::new()
                .name(format!("cap:{source_name}"))
                .spawn(move || {
                    run_source(
                        app2, opened, engine2, vad_params, vad_model, session2,
                        translate_queue, stop_thread, sources_map, thread_id,
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

    /// 翻译队列快照（UI 展示用）
    pub fn queue_list(&self) -> Vec<QueueItemInfo> {
        self.translate_queue
            .lock()
            .unwrap()
            .iter()
            .map(|j| QueueItemInfo {
                id: j.payload.id.clone(),
                session_id: j.payload.session_id.clone(),
                source_name: j.payload.source_name.clone(),
                raw_preview: j.payload.raw_text.chars().take(48).collect(),
            })
            .collect()
    }

    /// 取消队列中指定任务；返回是否移除
    pub fn queue_cancel(&self, id: &str) -> bool {
        let mut q = self.translate_queue.lock().unwrap();
        let before = q.len();
        q.retain(|j| j.payload.id != id);
        let removed = q.len() < before;
        if removed {
            tracing::info!("翻译任务已取消: {id}");
        }
        removed
    }

    /// 清空整个翻译队列；返回移除数量
    pub fn queue_clear(&self) -> usize {
        let mut q = self.translate_queue.lock().unwrap();
        let n = q.len();
        q.clear();
        if n > 0 {
            tracing::info!("翻译队列已清空: {n} 项");
        }
        n
    }

    /// 会话导出数据（记录列表 + 会话 id + 开始时间）
    pub fn session_export_data(
        &self,
    ) -> Option<(Vec<crate::events::TranscriptPayload>, String, String)> {
        self.session.lock().unwrap().as_ref().map(|s| {
            (
                s.items(),
                s.session_id.clone(),
                s.session_start.format("%Y-%m-%d %H:%M:%S").to_string(),
            )
        })
    }

    /// 停止全部流水线（线程退出时释放采集流与 VAD；模型保留供下次快速启动）
    pub fn stop_all(&self) {
        *self.last_activity.lock().unwrap() = Some(std::time::Instant::now());
        let mut sources = self.sources.lock().unwrap();
        for (id, h) in sources.drain() {
            h.stop.store(true, Ordering::SeqCst);
            tracing::info!("流水线停止: {id}");
        }
        drop(sources);
        // 翻译队列独立运行：转写停止后队列中剩余任务继续翻译（readme §10.5 需求）
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

/// 队列任务快照（UI 展示/取消用）
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QueueItemInfo {
    pub id: String,
    pub session_id: String,
    pub source_name: String,
    pub raw_preview: String,
}

/// 翻译 worker：常驻串行消费队列（独立于转写生命周期）。
/// 转写停止后队列中剩余任务继续翻译；翻译禁用时挂起不消费；UI 可取消/清空。
pub(crate) fn translation_worker(
    app: AppHandle,
    queue: Arc<Mutex<std::collections::VecDeque<TranslateJob>>>,
    model_root_getter: impl Fn() -> Option<PathBuf> + Send + 'static,
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
    emit_state("ok", None); // 通道就绪

    loop {
        // 轮询队列（250ms；翻译延迟相对 LLM 推理可忽略，CPU 占用极低）
        std::thread::sleep(Duration::from_millis(250));
        let cfg = app.state::<SettingsState>().0.lock().unwrap().translation.clone();
        if !cfg.enabled {
            continue; // 翻译禁用 → 挂起不消费（队列保留，重新启用后继续）
        }

        // 取队首任务（跳过已被 UI 取消的会话残留）
        let job = {
            let mut q = queue.lock().unwrap();
            q.pop_front()
        };
        let Some(job) = job else {
            continue;
        };

        if cfg.provider == "builtin" && !hub.is_loaded() {
            emit_state("loading", Some("首次使用需加载翻译模型（约 8s）".into()));
        }

        let Some(model_root) = model_root_getter() else {
            emit_state("error", Some("模型目录不可用".into()));
            continue;
        };
        let t0 = std::time::Instant::now();
        match tauri::async_runtime::block_on(crate::translate::translate_with_fallback(
            &job.payload.raw_text,
            job.payload.lang.as_deref(),
            &cfg,
            &mut state,
            &model_root,
            &hub,
        )) {
            Ok(outcome) if outcome.provider != "none" => {
                tracing::info!(
                    "翻译完成（{}，{:.1}s）: {}",
                    outcome.provider,
                    t0.elapsed().as_secs_f32(),
                    job.payload.id
                );
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
    translate_queue: Arc<Mutex<std::collections::VecDeque<TranslateJob>>>,
    stop: Arc<AtomicBool>,
    sources_map: Arc<Mutex<HashMap<String, SourceHandle>>>,
    source_id: String,
) {
    let (tx, rx) = mpsc::channel::<Vec<f32>>();
    // 双后端采集：cpal（麦克风/环回）或 WASAPI 环回（Windows，直出 16k mono）
    let (native_rate, cpal_stream) = match &source.device {
        audio::SourceDevice::Cpal(dev) => {
            let s = match audio::capture::build_input_stream(dev, tx, stop.clone()) {
                Ok(s) => s,
                Err(e) => {
                    emit_error(&app, Some(&source.desc.id), e.to_string());
                    return;
                }
            };
            if let Err(e) = s.stream.play() {
                emit_error(&app, Some(&source.desc.id), e.to_string());
                return;
            }
            (s.native_rate, Some(s))
        }
        #[cfg(target_os = "windows")]
        audio::SourceDevice::WasapiLoopback { endpoint_id } => {
            match crate::audio::wasapi_loopback::spawn_capture(
                endpoint_id.clone(),
                tx,
                stop.clone(),
            ) {
                Ok((rate, _handle)) => (rate, None), // 16k 直出，无需重采样
                Err(e) => {
                    emit_error(&app, Some(&source.desc.id), e.to_string());
                    return;
                }
            }
        }
    };
    tracing::info!("采集启动: {} ({}Hz, {}ch)", source.desc.name, native_rate, source.desc.channels);

    let mut resampler = audio::resample::Resampler::new(native_rate, 16_000);
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
                    handle_segment(&app, &source.desc, &engine, &seg, &session, &translate_queue);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    for seg in segmenter.flush() {
        handle_segment(&app, &source.desc, &engine, &seg, &session, &translate_queue);
    }
    drop(cpal_stream); // cpal 采集流随线程退出释放（WASAPI 线程自持）
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
    translate_queue: &Arc<Mutex<std::collections::VecDeque<TranslateJob>>>,
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
                translate_queue.lock().unwrap().push_back(TranslateJob {
                    session: Arc::clone(session),
                    payload,
                });
            }
        }
        Ok(_) => {} // 空句（如纯噪音）
        Err(e) => tracing::warn!("识别失败（{}）: {e}", desc.name),
    }
}

/// 空闲卸载看门狗：每 60s 检查一次，无活动源且超过设定分钟数时
/// 卸载 ASR 与内置翻译引擎（docs/porting-notes.md §4）
fn idle_watchdog(app: AppHandle) {
    use tauri::Manager;
    loop {
        std::thread::sleep(Duration::from_secs(60));
        let Some(man) = app.try_state::<PipelineManager>() else {
            return;
        };
        if !man.sources.lock().unwrap().is_empty() {
            continue; // 转写进行中
        }
        let idle_minutes = app
            .state::<SettingsState>()
            .0
            .lock()
            .unwrap()
            .advanced
            .idle_unload_minutes;
        let Some(last) = *man.last_activity.lock().unwrap() else {
            continue;
        };
        if last.elapsed() >= Duration::from_secs(idle_minutes * 60) {
            man.engine.unload();
            man.translate_hub.unload();
            *man.last_activity.lock().unwrap() = None; // 已卸载，待下次活动重新计时
            tracing::info!("空闲超过 {idle_minutes} 分钟，模型已卸载");
        }
    }
}
