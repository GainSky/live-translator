import { create } from "zustand";
import { saveSettings } from "@/lib/ipc";
import {
  DEFAULT_SETTINGS,
  type AudioDeviceInfo,
  type EngineStatus,
  type Settings,
  type TranscriptItem,
} from "@/types";

/** 运行态 + 设置的单一事实源（设置与后端 settings.json 同步） */
interface AppState {
  // ---- 设置 ----
  settings: Settings;
  setSettings: (s: Settings) => void;
  /** 外观即时变更：更新内存态并持久化（主题/字体/译文开关等） */
  updateAppearance: (
    patch: Partial<Settings["appearance"]>,
  ) => void;

  // ---- 音源 ----
  devices: AudioDeviceInfo[];
  setDevices: (d: AudioDeviceInfo[]) => void;
  selectedIds: string[];
  toggleSelected: (id: string) => void;
  running: boolean;
  runningIds: string[];
  setRunning: (running: boolean, ids?: string[]) => void;
  levels: Record<string, number>;
  setLevel: (id: string, peak: number) => void;
  decayLevels: () => void;

  // ---- 转写 ----
  transcripts: TranscriptItem[];
  addTranscript: (t: TranscriptItem) => void;
  /** 翻译完成后按 id 回填译文 */
  updateTranscript: (
    id: string,
    translatedText: string,
    provider: string,
  ) => void;
  clearTranscripts: () => void;

  // ---- 会话/状态 ----
  session: { sessionId: string; startClock: string } | null;
  setSession: (s: { sessionId: string; startClock: string } | null) => void;
  engineState: { status: EngineStatus; detail?: string } | null;
  setEngineState: (s: { status: EngineStatus; detail?: string } | null) => void;
  translateState: { status: string; detail?: string } | null;
  setTranslateState: (s: { status: string; detail?: string } | null) => void;
  lastError: string | null;
  setLastError: (e: string | null) => void;
}

export const useAppStore = create<AppState>((set, get) => ({
  settings: DEFAULT_SETTINGS,
  setSettings: (settings) => set({ settings }),

  updateAppearance: (patch) => {
    const cur = get().settings;
    const next: Settings = {
      ...cur,
      appearance: { ...cur.appearance, ...patch },
    };
    set({ settings: next });
    saveSettings(next).catch((e) => console.warn("设置保存失败:", e));
  },

  devices: [],
  setDevices: (devices) => set({ devices }),
  selectedIds: [],
  toggleSelected: (id) =>
    set((s) => ({
      selectedIds: s.selectedIds.includes(id)
        ? s.selectedIds.filter((x) => x !== id)
        : [...s.selectedIds, id],
    })),
  running: false,
  runningIds: [],
  setRunning: (running, ids) =>
    set({ running, runningIds: ids ?? (running ? get().selectedIds : []) }),
  levels: {},
  setLevel: (id, peak) =>
    set((s) => ({ levels: { ...s.levels, [id]: peak } })),
  decayLevels: () =>
    set((s) => {
      const next: Record<string, number> = {};
      for (const [k, v] of Object.entries(s.levels)) {
        const d = v * 0.8;
        if (d > 0.01) next[k] = d;
      }
      return { levels: next };
    }),

  transcripts: [],
  addTranscript: (t) => set((s) => ({ transcripts: [...s.transcripts, t] })),
  updateTranscript: (id, translatedText, provider) =>
    set((s) => ({
      transcripts: s.transcripts.map((t) =>
        t.id === id ? { ...t, translatedText, provider } : t,
      ),
    })),
  clearTranscripts: () => set({ transcripts: [] }),

  session: null,
  setSession: (session) => set({ session }),
  engineState: null,
  setEngineState: (engineState) => set({ engineState }),
  translateState: null,
  setTranslateState: (translateState) => set({ translateState }),
  lastError: null,
  setLastError: (lastError) => set({ lastError }),
}));

/** 常见字体候选（含 CJK；Linux/Windows 通用 + 自定义输入兜底） */
export const FONT_CANDIDATES = [
  "system-ui",
  "Noto Sans CJK SC",
  "Source Han Sans SC",
  "WenQuanYi Micro Hei",
  "Microsoft YaHei",
  "Segoe UI",
  "Liberation Sans",
  "DejaVu Sans",
  "Noto Sans Mono CJK SC",
  "monospace",
];
