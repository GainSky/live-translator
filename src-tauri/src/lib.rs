pub mod asr;
pub mod audio;
pub mod commands;
pub mod error;
pub mod events;
pub mod models;
pub mod pipeline;
pub mod settings;
pub mod store;
pub mod translate;

use pipeline::PipelineManager;
use settings::Settings;
use std::sync::Mutex;

/// 全局设置状态（内存态，磁盘持久化见 commands::save_settings）
pub struct SettingsState(pub Mutex<Settings>);

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(PipelineManager::default())
        .manage(SettingsState(Mutex::new(Settings::default())))
        .setup(|app| {
            use tauri::Manager;
            use tracing_subscriber::{layer::SubscriberExt, layer::Layer as _, util::SubscriberInitExt, EnvFilter};

            // 日志初始化：双写（终端 + 每日滚动文件）。
            // Windows release 为 GUI 子系统无控制台 → 文件是唯一可靠日志出口，
            // 位置：<app_data_dir>/logs/live-translator.log.YYYY-MM-DD
            let data_dir = app.path().app_data_dir()?;
            let log_dir = data_dir.join("logs");
            std::fs::create_dir_all(&log_dir)?;
            let filter = EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info"));
            let file_appender = tracing_appender::rolling::daily(&log_dir, "live-translator.log");
            tracing_subscriber::registry()
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(std::io::stdout)
                        .with_ansi(false)
                        .with_filter(filter.clone()),
                )
                .with(
                    tracing_subscriber::fmt::layer()
                        .with_writer(file_appender)
                        .with_ansi(false)
                        .with_filter(filter),
                )
                .init();

            // 载入用户设置：
            //   Windows: exe 同目录 | Linux/macOS: ~/.config/{identifier}
            //   旧位置（app_data_dir）存在设置时自动迁移到新位置
            let data_dir = app.path().app_data_dir()?;
            let config_dir = settings::settings_dir(app.handle())?;
            let loaded = settings::load_with_migration(&config_dir, &data_dir);
            *app.state::<SettingsState>().0.lock().unwrap() = loaded;
            tracing::info!("设置目录: {}", config_dir.display());

            // 初始化 SQLite（会话转写记录）
            store::db::init(&data_dir.join("live-translator.db"))?;
            tracing::info!("数据库就绪: {}", data_dir.join("live-translator.db").display());

            // 悬浮窗默认隐藏（M4 由命令 show_overlay / hide_overlay 控制）
            if let Some(overlay) = app.get_webview_window("overlay") {
                overlay.hide()?;
            }

            // 翻译队列 worker：常驻（独立于转写生命周期；翻译禁用时挂起不消费）
            let manager = app.state::<PipelineManager>();
            let queue = manager.translate_queue.clone();
            let hub = manager.translate_hub.clone();
            let app_handle = app.handle().clone();
            let app_for_root = app.handle().clone();
            std::thread::Builder::new()
                .name("translate".into())
                .spawn(move || {
                    // 模型目录在每次取任务时惰性解析（env/便携/数据目录均兼容）
                    let model_root_getter = move || {
                        crate::models::resolve_model_root(&app_for_root).ok()
                    };
                    crate::pipeline::translation_worker(app_handle, queue, model_root_getter, hub);
                })
                .ok();

            tracing::info!("LiveTranslator 初始化完成");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_audio_devices,
            commands::start_pipeline,
            commands::stop_pipeline,
            commands::current_session,
            commands::translate_queue_list,
            commands::translate_queue_cancel,
            commands::translate_queue_clear,
            commands::get_settings,
            commands::save_settings,
            commands::test_translation,
            commands::export_transcripts,
            commands::list_models,
            commands::download_model,
            commands::show_overlay,
            commands::hide_overlay,
            commands::set_overlay_lock,
            commands::reset_overlay_pos,
        ])
        .on_window_event(|window, event| {
            use tauri::Manager;
            match event {
                // 主窗口关闭 → 停止流水线、保存设置（含悬浮窗位置）并退出进程
                // （悬浮窗常驻隐藏状态，否则关闭主窗后进程因 overlay 窗口存活而不退出）
                tauri::WindowEvent::Destroyed if window.label() == "main" => {
                    let app = window.app_handle();
                    tracing::info!("主窗口已关闭，停止流水线并退出");
                    app.state::<PipelineManager>().stop_all();
                    let s = app.state::<SettingsState>().0.lock().unwrap().clone();
                    if let Ok(dir) = crate::settings::settings_dir(app) {
                        let _ = crate::settings::save(&dir, &s);
                    }
                    app.exit(0);
                }
                // 悬浮窗拖动 → 记忆位置（仅内存，退出时统一落盘）
                tauri::WindowEvent::Moved(pos) if window.label() == "overlay" => {
                    let app = window.app_handle();
                    app.state::<SettingsState>().0.lock().unwrap().appearance.overlay_pos =
                        Some(crate::settings::OverlayPos { x: pos.x, y: pos.y });
                }
                _ => {}
            }
        })
        .run(tauri::generate_context!())
        .expect("LiveTranslator 启动失败");
}
