use std::collections::HashMap;
use std::path::Path;

use tauri::{AppHandle, Manager, State};
use tauri::Emitter;

use crate::error::{AppError, AppResult};
use crate::pipeline::PipelineManager;
use crate::settings::Settings;
use crate::SettingsState;

/// 枚举音频源并做一次信号测试（每源并行探测 ~0.7s）：
/// 检测到信号的源带 signal 峰值并排序在前（readme §5.2 音源页"有声标识"）
#[tauri::command]
pub async fn list_audio_devices() -> AppResult<Vec<crate::audio::AudioDeviceDescriptor>> {
    tokio::task::spawn_blocking(|| {
        let mut devs = crate::audio::enumerate_devices()?;
        let ids: Vec<String> = devs.iter().map(|d| d.id.clone()).collect();
        let peaks: HashMap<String, f32> =
            crate::audio::probe::probe_parallel(&ids).into_iter().collect();
        for d in devs.iter_mut() {
            d.signal = peaks.get(&d.id).copied().unwrap_or(0.0);
        }
        // 有信号的排前（峰值降序），无信号的保持原顺序（稳定排序）
        let th = crate::audio::probe::SIGNAL_THRESHOLD;
        devs.sort_by(|a, b| {
            let a_on = a.signal >= th;
            let b_on = b.signal >= th;
            b_on.cmp(&a_on).then(
                b.signal
                    .partial_cmp(&a.signal)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
        });
        Ok(devs)
    })
    .await
    .map_err(|e| AppError::Message(format!("信号测试任务失败: {e}")))?
}

#[tauri::command]
pub fn start_pipeline(
    app: AppHandle,
    state: State<'_, PipelineManager>,
    settings: State<'_, SettingsState>,
    source_ids: Vec<String>,
) -> AppResult<()> {
    let s = settings.0.lock().unwrap().clone();
    state.start(&app, source_ids, &s)
}

#[tauri::command]
pub fn stop_pipeline(state: State<'_, PipelineManager>) -> AppResult<()> {
    state.stop_all();
    Ok(())
}

#[tauri::command]
pub fn current_session(state: State<'_, PipelineManager>) -> Option<crate::store::SessionInfo> {
    state.current_session()
}

#[tauri::command]
pub fn translate_queue_list(
    state: State<'_, PipelineManager>,
) -> Vec<crate::pipeline::QueueItemInfo> {
    state.queue_list()
}

#[tauri::command]
pub fn translate_queue_cancel(state: State<'_, PipelineManager>, id: String) -> bool {
    state.queue_cancel(&id)
}

#[tauri::command]
pub fn translate_queue_clear(state: State<'_, PipelineManager>) -> usize {
    state.queue_clear()
}

#[tauri::command]
pub fn get_settings(state: State<'_, SettingsState>) -> Settings {
    state.0.lock().unwrap().clone()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, SettingsState>,
    settings: Settings,
) -> AppResult<()> {
    // Windows: exe 同目录 | Linux/macOS: ~/.config/{identifier}
    let dir = crate::settings::settings_dir(&app)?;
    crate::settings::save(&dir, &settings)?;
    *state.0.lock().unwrap() = settings;
    tracing::info!("设置已保存至 {}", dir.join("settings.json").display());
    Ok(())
}

#[tauri::command]
pub async fn test_translation(
    app: AppHandle,
    pipeline: State<'_, PipelineManager>,
    state: State<'_, SettingsState>,
    text: String,
    config: Option<crate::settings::TranslationSettings>,
) -> AppResult<String> {
    // 前端可传入界面草稿配置（未保存也能测）；否则用已保存配置
    let mut cfg = config.unwrap_or_else(|| state.0.lock().unwrap().translation.clone());
    // 显式测试：不受「启用翻译」开关限制
    cfg.enabled = true;
    let model_root = crate::models::resolve_model_root(&app)?;
    let mut state_deg = crate::translate::DegradationState::default();
    let outcome = crate::translate::translate_with_fallback(
        &text,
        None,
        &cfg,
        &mut state_deg,
        &model_root,
        &pipeline.translate_hub,
    )
    .await?;
    Ok(format!("[{}] {}", outcome.provider, outcome.text))
}

#[tauri::command]
pub fn list_models(app: AppHandle) -> AppResult<crate::models::ModelsPage> {
    let dir = crate::models::resolve_model_root(&app)?;
    crate::models::list_models(&dir)
}

#[tauri::command]
pub fn resolve_models_dir(app: AppHandle) -> AppResult<String> {
    Ok(crate::models::resolve_model_root(&app)?.display().to_string())
}

#[tauri::command]
pub fn download_model(app: AppHandle, id: String) -> AppResult<()> {
    let dir = crate::models::resolve_model_root(&app)?;
    crate::models::download_in_background(app, &dir, &id)
}

#[tauri::command]
pub fn export_transcripts(
    state: State<'_, PipelineManager>,
    format: String,
    path: String,
) -> AppResult<String> {
    let fmt = crate::store::export::ExportFormat::parse(&format)?;
    let (items, session_id, session_start) = state
        .session_export_data()
        .ok_or_else(|| AppError::Message("当前没有转写会话，请先开始一次转写".into()))?;
    let written = crate::store::export::export(
        &items,
        fmt,
        Path::new(&path),
        &session_id,
        &session_start,
    )?;
    Ok(written.display().to_string())
}

#[tauri::command]
pub fn show_overlay(app: AppHandle, state: State<'_, SettingsState>) -> AppResult<()> {
    let window = app
        .get_webview_window("overlay")
        .ok_or_else(|| AppError::Message("悬浮窗未初始化".into()))?;
    // 恢复上次位置 + 重置锁定（穿透）
    if let Some(pos) = state.0.lock().unwrap().appearance.overlay_pos {
        window.set_position(tauri::PhysicalPosition::new(pos.x, pos.y))?;
        window.set_ignore_cursor_events(false)?;
    }
    window.show()?;
    Ok(())
}

/// 重置悬浮窗位置（清除记忆位置并移动到默认位置）
#[tauri::command]
pub fn reset_overlay_pos(app: AppHandle, state: State<'_, SettingsState>) -> AppResult<()> {
    let window = app
        .get_webview_window("overlay")
        .ok_or_else(|| AppError::Message("悬浮窗未初始化".into()))?;
    state.0.lock().unwrap().appearance.overlay_pos = None;
    window.set_position(tauri::LogicalPosition::new(120.0, 120.0))?;
    tracing::info!("悬浮窗位置已重置");
    Ok(())
}

/// 悬浮窗锁定（点击穿透）：锁定后鼠标事件穿透到下层窗口，
/// 解锁请用主窗转写页的「解锁悬浮窗」按钮
#[tauri::command]
pub fn set_overlay_lock(app: AppHandle, locked: bool) -> AppResult<()> {
    let window = app
        .get_webview_window("overlay")
        .ok_or_else(|| AppError::Message("悬浮窗未初始化".into()))?;
    window.set_ignore_cursor_events(locked)?;
    tracing::info!("悬浮窗锁定: {locked}");
    Ok(())
}

#[tauri::command]
pub fn hide_overlay(app: AppHandle) -> AppResult<()> {
    let window = app
        .get_webview_window("overlay")
        .ok_or_else(|| AppError::Message("悬浮窗未初始化".into()))?;
    window.hide()?;
    Ok(())
}

// 事件上报辅助（M1 起流水线使用）：统一从此处 emit，避免散落
pub fn emit_transcript(app: &AppHandle, payload: crate::events::TranscriptPayload) {
    let _ = app.emit(crate::events::EV_TRANSCRIPT_NEW, payload);
}

pub fn emit_engine_state(app: &AppHandle, payload: crate::events::EngineStatePayload) {
    let _ = app.emit(crate::events::EV_ENGINE_STATE, payload);
}
