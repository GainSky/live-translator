// 与 Rust 侧 serde 结构一一对应（Rust 端统一 #[serde(rename_all = "camelCase")]）

export type AudioDeviceKind =
  | "microphone"
  | "loopback"
  | "monitor"
  | "application"
  | "virtual"
  | "unknown";

export interface AudioDeviceInfo {
  id: string;
  name: string;
  kind: AudioDeviceKind;
  sampleRate: number;
  channels: number;
  isDefault: boolean;
  /** 信号测试峰值（0~1；list 命令并行探测后填充，阈值 0.01） */
  signal: number;
}

/** 与 Rust audio::probe::SIGNAL_THRESHOLD 一致 */
export const SIGNAL_THRESHOLD = 0.01;

export type EngineStatus =
  | "unloaded"
  | "loading"
  | "ready"
  | "fallbackCpu"
  | "error";

export interface TranscriptItem {
  id: string;
  sessionId: string;
  sourceId: string;
  sourceName: string;
  /** 相对会话起始的毫秒时间戳（VAD 切句起止，导出 SRT 用） */
  startMs: number;
  endMs: number;
  /** 墙钟时间 HH:MM:SS */
  clockTime: string;
  rawText: string;
  /** SenseVoice/Whisper 检出的语言代码（zh/en/ja/ko…） */
  lang?: string;
  translatedText?: string;
  /** 翻译来源：openai-compatible / local-llm / opencc */
  provider?: string;
  asrEngine?: string;
}

export type OverlayMode = "both" | "raw" | "translated";

export type ThemePref = "system" | "light" | "dark";

export interface FontPref {
  family: string;
  /** px */
  size: number;
}

export interface VadParams {
  threshold: number;
  minSilenceMs: number;
  minSpeechMs: number;
  maxSpeechMs: number;
}

export interface TranslationConfig {
  enabled: boolean;
  targetLang: string;
  /** builtin（内置 candle 引擎·离线）| openai-compatible（远程）| local-http（Ollama 等） */
  provider: "builtin" | "openai-compatible" | "local-http";
  builtin: {
    /** models/ 目录下的 GGUF 文件名 */
    model: string;
  };
  openai: {
    baseUrl: string;
    apiKey: string;
    model: string;
    temperature: number;
  };
  local: {
    baseUrl: string;
    model: string;
  };
  googleFallback: boolean;
}

export interface Settings {
  asr: {
    engine: "sense-voice";
    sourceLang: string;
    vad: VadParams;
  };
  translation: TranslationConfig;
  appearance: {
    theme: ThemePref;
    showTranslated: boolean;
    mainFont: FontPref;
    overlayFont: FontPref;
    overlayMode: OverlayMode;
  };
  advanced: {
    /** 空闲 N 分钟后自动卸载模型（移植自参考实现 600s 策略） */
    idleUnloadMinutes: number;
  };
}

/** 与 Rust 侧 Default 值一致（src-tauri/src/settings.rs） */
export const DEFAULT_SETTINGS: Settings = {
  asr: {
    engine: "sense-voice",
    sourceLang: "auto",
    vad: {
      threshold: 0.4,
      minSilenceMs: 500,
      minSpeechMs: 150,
      maxSpeechMs: 10000,
    },
  },
  translation: {
    enabled: false,
    targetLang: "zh-CN",
    provider: "builtin",
    builtin: { model: "qwen2.5-3b-instruct-q4-k-m.gguf" },
    openai: {
      baseUrl: "https://api.deepseek.com/v1",
      apiKey: "",
      model: "deepseek-chat",
      temperature: 0.3,
    },
    local: {
      baseUrl: "http://127.0.0.1:11434/v1",
      model: "qwen2.5:7b-instruct",
    },
    googleFallback: false,
  },
  appearance: {
    theme: "system",
    showTranslated: true,
    mainFont: { family: "system-ui", size: 15 },
    overlayFont: { family: "system-ui", size: 22 },
    overlayMode: "both",
  },
  advanced: {
    idleUnloadMinutes: 10,
  },
};

/** 当前会话信息（Rust store::SessionInfo） */
export interface SessionInfo {
  sessionId: string;
  startClock: string;
}
