//! 文件转写页：选择音频/视频文件 → 解码（symphonia/ffmpeg）→ VAD 切句 →
//! SenseVoice 识别（文件时间轴）→ 可选翻译 → 导出（SRT 即视频字幕）。

import { useEffect, useRef, useState } from "react";
import { useAppStore } from "@/stores/appStore";
import { MainFontControl } from "@/components/FontPopover";
import {
  cancelFileTranscription,
  exportMediaTranscripts,
  onFileProgress,
  onFileTranscript,
  transcribeFile,
} from "@/lib/ipc";
import { open, save } from "@tauri-apps/plugin-dialog";
import type { FileProgress, TranscriptItem } from "@/types";

const SOURCE_LANGS: { value: string; label: string }[] = [
  { value: "auto", label: "自动判定" },
  { value: "zh", label: "中文" },
  { value: "en", label: "英语" },
  { value: "ja", label: "日语" },
  { value: "ko", label: "韩语" },
  { value: "yue", label: "粤语" },
];

const EXPORT_FORMATS = ["srt", "txt", "md", "csv", "json"] as const;

function fmtSecs(s: number): string {
  const sec = Math.floor(s);
  return `${String(Math.floor(sec / 3600)).padStart(2, "0")}:${String(
    Math.floor((sec % 3600) / 60),
  ).padStart(2, "0")}:${String(sec % 60).padStart(2, "0")}`;
}

export function FileTranscribePage() {
  const showTranslated = useAppStore((s) => s.settings.appearance.showTranslated);
  const targetLang = useAppStore((s) => s.settings.translation.targetLang);
  const setLastError = useAppStore((s) => s.setLastError);
  const bottomRef = useRef<HTMLDivElement>(null);

  const [filePath, setFilePath] = useState<string | null>(null);
  const [sourceLang, setSourceLang] = useState("auto");
  const [running, setRunning] = useState(false);
  const [progress, setProgress] = useState<FileProgress | null>(null);
  const [items, setItems] = useState<TranscriptItem[]>([]);
  const [exportMsg, setExportMsg] = useState<string | null>(null);

  useEffect(() => {
    const unsubs = [
      onFileProgress((p) => setProgress(p)),
      onFileTranscript((t) => setItems((cur) => [...cur, t])),
    ];
    return () => unsubs.forEach((p) => p.then((f) => f()).catch(() => {}));
  }, []);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [items.length]);

  const pickFile = async () => {
    const picked = await open({
      multiple: false,
      filters: [
        {
          name: "媒体文件",
          extensions: [
            "wav", "mp3", "flac", "ogg", "m4a", "aac", "opus",
            "mp4", "mkv", "webm", "mov", "avi", "flv", "ts",
          ],
        },
      ],
    });
    if (typeof picked === "string") setFilePath(picked);
  };

  const start = async () => {
    if (!filePath) return;
    setItems([]);
    setProgress(null);
    setExportMsg(null);
    setRunning(true);
    try {
      await transcribeFile(filePath, sourceLang);
    } catch (e) {
      setRunning(false);
      setLastError(`文件转写启动失败: ${e}`);
    }
  };

  const cancel = async () => {
    try {
      await cancelFileTranscription();
    } catch (e) {
      setLastError(`取消失败: ${e}`);
    }
  };

  // 任务结束（done/canceled/error）→ 解除运行态
  useEffect(() => {
    if (progress && ["done", "canceled", "error"].includes(progress.phase)) {
      setRunning(false);
      if (progress.phase === "error") {
        setLastError(`文件转写失败: ${progress.message ?? "未知错误"}`);
      }
    }
  }, [progress?.phase, progress]);

  const doExport = async (fmt: (typeof EXPORT_FORMATS)[number]) => {
    const path = await save({
      defaultPath: `transcript_${new Date().toISOString().slice(0, 10)}.${fmt}`,
      filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }],
    });
    if (!path) return;
    try {
      const written = await exportMediaTranscripts(fmt, path);
      setExportMsg(`已导出: ${written}`);
    } catch (e) {
      setExportMsg(`导出失败: ${e}`);
    }
  };

  const pct =
    progress && progress.totalSecs && progress.totalSecs > 0
      ? Math.min(100, (progress.decodedSecs / progress.totalSecs) * 100)
      : null;

  return (
    <div className="flex h-full flex-col p-6">
      {/* 工具栏 */}
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <button
          onClick={pickFile}
          disabled={running}
          className="rounded-md border border-border px-4 py-2 text-[1.15rem] hover:bg-accent disabled:opacity-50"
        >
          选择文件…
        </button>
        <select
          value={sourceLang}
          onChange={(e) => setSourceLang(e.target.value)}
          disabled={running}
          className="input w-auto"
          title="源语言（影响识别准确率）"
        >
          {SOURCE_LANGS.map((l) => (
            <option key={l.value} value={l.value}>{l.label}</option>
          ))}
        </select>
        {running ? (
          <button
            onClick={cancel}
            className="rounded-md border border-destructive px-4 py-2 text-[1.15rem] text-destructive hover:bg-destructive/10"
          >
            取消
          </button>
        ) : (
          <button
            onClick={start}
            disabled={!filePath}
            className="rounded-md bg-primary px-5 py-2 text-[1.15rem] font-medium text-primary-foreground disabled:opacity-50"
          >
            开始转写
          </button>
        )}
        <div className="flex items-center gap-2">
          {EXPORT_FORMATS.map((f) => (
            <button
              key={f}
              onClick={() => doExport(f)}
              disabled={items.length === 0}
              className="rounded-md border border-border px-3 py-1.5 text-[1rem] hover:bg-accent disabled:opacity-50"
            >
              导出 {f.toUpperCase()}
            </button>
          ))}
        </div>
        <MainFontControl />
      </div>

      {/* 文件与进度 */}
      <div className="mb-3 space-y-2 rounded-lg border border-border bg-card p-4">
        <p className="text-[1.1rem]">
          文件：{filePath ?? "（未选择）"}
          {running && <span className="ml-2 text-primary">转写中…</span>}
        </p>
        {progress && (
          <>
            {pct !== null && (
              <div className="h-2.5 w-full overflow-hidden rounded-full bg-muted">
                <div
                  className="h-full rounded-full bg-primary transition-all"
                  style={{ width: `${pct}%` }}
                />
              </div>
            )}
            <p className="text-[0.95rem] text-muted-foreground">
              阶段：{progress.phase}
              {progress.totalSecs
                ? ` · ${fmtSecs(progress.decodedSecs)} / ${fmtSecs(progress.totalSecs)}`
                : ` · 已解码 ${fmtSecs(progress.decodedSecs)}`}
              {" · "}已出 {progress.segments} 句
              {progress.message ? ` · ${progress.message}` : ""}
            </p>
          </>
        )}
        {exportMsg && <p className="text-[0.95rem] text-primary">{exportMsg}</p>}
      </div>

      {/* 转写列表 */}
      <div className="min-h-0 flex-1 space-y-2.5 overflow-y-auto">
        {items.length === 0 && (
          <p className="text-[1.05rem] text-muted-foreground">
            选择一个音频/视频文件开始转写。视频需要系统安装 ffmpeg；
            生成的时间轴可直接导出 SRT 字幕。目标语言跟随设置页（当前：
            {targetLang}）。
          </p>
        )}
        {items.map((t) => (
          <article
            key={t.id}
            className="animate-fade-in-up rounded-lg border border-border bg-card p-4"
          >
            <header className="mb-1 flex items-center gap-2 text-xs text-muted-foreground">
              <time>{fmtSecs(t.startMs / 1000)}</time>
              <span className="opacity-60">
                {(t.startMs / 1000).toFixed(1)}s–{(t.endMs / 1000).toFixed(1)}s
              </span>
            </header>
            <p className="leading-relaxed">{t.rawText}</p>
            {showTranslated && t.translatedText && (
              <p className="mt-1 leading-relaxed text-muted-foreground">
                {t.translatedText}
              </p>
            )}
          </article>
        ))}
        <div ref={bottomRef} />
      </div>
    </div>
  );
}

