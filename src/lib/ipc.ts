// Tauri IPC 封装：事件名与命令名与 Rust 侧保持一致
// 事件常量见 src-tauri/src/events.rs，命令见 src-tauri/src/commands.rs

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AudioDeviceInfo,
  QueueItemInfo,
  ModelInfo,
  ModelProgress,
  ModelsPage,
  TranslationConfig,
  EngineStatus,
  SessionInfo,
  Settings,
  TranscriptItem,
} from "@/types";

// ---- 事件名 ----
export const EVENTS = {
  transcriptNew: "transcript:new",
  transcriptUpdate: "transcript:update",
  engineState: "engine:state",
  translateState: "translate:state",
  audioLevel: "audio:level",
  pipelineError: "pipeline:error",
  modelProgress: "model:progress",
} as const;

// ---- 命令 ----
export function listAudioDevices(): Promise<AudioDeviceInfo[]> {
  return invoke("list_audio_devices");
}

export function startPipeline(sourceIds: string[]): Promise<void> {
  return invoke("start_pipeline", { sourceIds });
}

export function stopPipeline(): Promise<void> {
  return invoke("stop_pipeline");
}

export function currentSession(): Promise<SessionInfo | null> {
  return invoke("current_session");
}

export function getSettings(): Promise<Settings> {
  return invoke("get_settings");
}

export function saveSettings(settings: Settings): Promise<void> {
  return invoke("save_settings", { settings });
}

export function listModels(): Promise<ModelsPage> {
  return invoke("list_models");
}

export function resolveModelsDir(): Promise<string> {
  return invoke("resolve_models_dir");
}

export function downloadModel(id: string): Promise<void> {
  return invoke("download_model", { id });
}

export function testTranslation(
  text: string,
  config?: TranslationConfig,
): Promise<string> {
  return invoke("test_translation", { text, config: config ?? null });
}

export function translateQueueList(): Promise<QueueItemInfo[]> {
  return invoke("translate_queue_list");
}

export function translateQueueCancel(id: string): Promise<boolean> {
  return invoke("translate_queue_cancel", { id });
}

export function translateQueueClear(): Promise<number> {
  return invoke("translate_queue_clear");
}

export function exportTranscripts(
  format: "txt" | "md" | "srt" | "csv" | "json",
  path: string,
): Promise<string> {
  return invoke("export_transcripts", { format, path });
}

export function showOverlay(): Promise<void> {
  return invoke("show_overlay");
}

export function hideOverlay(): Promise<void> {
  return invoke("hide_overlay");
}

export function setOverlayLock(locked: boolean): Promise<void> {
  return invoke("set_overlay_lock", { locked });
}

export function resetOverlayPos(): Promise<void> {
  return invoke("reset_overlay_pos");
}

// ---- 事件订阅 ----
export function onTranscript(
  cb: (item: TranscriptItem) => void,
): Promise<UnlistenFn> {
  return listen<TranscriptItem>(EVENTS.transcriptNew, (e) => cb(e.payload));
}

export function onTranscriptUpdate(
  cb: (u: { id: string; translatedText: string; provider: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ id: string; translatedText: string; provider: string }>(
    EVENTS.transcriptUpdate,
    (e) => cb(e.payload),
  );
}

export function onModelProgress(
  cb: (p: ModelProgress) => void,
): Promise<UnlistenFn> {
  return listen<ModelProgress>(EVENTS.modelProgress, (e) => cb(e.payload));
}

export function onTranslateState(
  cb: (s: { status: string; detail?: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ status: string; detail?: string }>(
    EVENTS.translateState,
    (e) => cb(e.payload),
  );
}

export function onEngineState(
  cb: (status: { engine: string; status: EngineStatus; detail?: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ engine: string; status: EngineStatus; detail?: string }>(
    EVENTS.engineState,
    (e) => cb(e.payload),
  );
}

export function onAudioLevel(
  cb: (level: { sourceId: string; peak: number }) => void,
): Promise<UnlistenFn> {
  return listen<{ sourceId: string; peak: number }>(EVENTS.audioLevel, (e) =>
    cb(e.payload),
  );
}

export function onPipelineError(
  cb: (err: { sourceId?: string; message: string }) => void,
): Promise<UnlistenFn> {
  return listen<{ sourceId?: string; message: string }>(EVENTS.pipelineError, (e) =>
    cb(e.payload),
  );
}
