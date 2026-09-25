import { useEffect, useState, type ReactNode } from "react";
import { useAppStore } from "@/stores/appStore";
import { useThemeEffect } from "@/hooks/useTheme";
import { TranscriptPage } from "@/pages/TranscriptPage";
import { FileTranscribePage } from "@/pages/FileTranscribePage";
import { SourcesPage } from "@/pages/SourcesPage";
import { SettingsPage } from "@/pages/SettingsPage";
import { Sidebar } from "@/components/Sidebar";
import {
  getSettings,
  onAudioLevel,
  onEngineState,
  onPipelineError,
  onTranscript,
  onTranscriptUpdate,
  onTranslateState,
} from "@/lib/ipc";

export type PageKey = "transcript" | "files" | "sources" | "settings";

const PAGES: Record<PageKey, { title: string; el: ReactNode }> = {
  transcript: { title: "转写", el: <TranscriptPage /> },
  files: { title: "文件转写", el: <FileTranscribePage /> },
  sources: { title: "音频源", el: <SourcesPage /> },
  settings: { title: "设置", el: <SettingsPage /> },
};

export default function App() {
  const theme = useAppStore((s) => s.settings.appearance.theme);
  const setSettings = useAppStore((s) => s.setSettings);
  const setLevel = useAppStore((s) => s.setLevel);
  const decayLevels = useAppStore((s) => s.decayLevels);
  const setEngineState = useAppStore((s) => s.setEngineState);
  const setLastError = useAppStore((s) => s.setLastError);
  const addTranscript = useAppStore((s) => s.addTranscript);
  const updateTranscript = useAppStore((s) => s.updateTranscript);
  const setTranslateState = useAppStore((s) => s.setTranslateState);
  const [page, setPage] = useState<PageKey>("transcript");

  useThemeEffect(theme);

  // 启动：载入后端设置
  useEffect(() => {
    getSettings()
      .then(setSettings)
      .catch((e) => console.warn("载入设置失败:", e));
  }, [setSettings]);

  // 全局事件总线（readme §5.1）
  useEffect(() => {
    const subs = [
      onTranscript(addTranscript),
      onTranscriptUpdate((u) => updateTranscript(u.id, u.translatedText, u.provider)),
      onAudioLevel((l) => setLevel(l.sourceId, l.peak)),
      onEngineState((s) => setEngineState(s)),
      onTranslateState((s) => setTranslateState(s)),
      onPipelineError((e) => setLastError(e.message)),
    ];
    return () => {
      subs.forEach((p) => p.then((f) => f()).catch(() => {}));
    };
  }, [addTranscript, updateTranscript, setLevel, setEngineState, setTranslateState, setLastError]);

  // 电平表衰减动画
  useEffect(() => {
    const t = setInterval(decayLevels, 150);
    return () => clearInterval(t);
  }, [decayLevels]);

  return (
    <div className="flex h-screen overflow-hidden">
      <Sidebar current={page} onNavigate={setPage} />
      <main className="flex min-w-0 flex-1 flex-col">
        <header className="flex items-center justify-between border-b border-border px-6 py-3">
          <h1 className="text-[1.9rem] font-semibold">{PAGES[page].title}</h1>
        </header>
        <div className="min-h-0 flex-1 overflow-y-auto">{PAGES[page].el}</div>
      </main>
    </div>
  );
}
