import { useEffect, useRef, useState } from "react";
import { useAppStore } from "@/stores/appStore";
import { MainFontControl } from "@/components/FontPopover";
import {
  exportTranscripts,
  hideOverlay,
  setOverlayLock,
  showOverlay,
  translateQueueCancel,
  translateQueueClear,
  translateQueueList,
} from "@/lib/ipc";
import { save } from "@tauri-apps/plugin-dialog";
import type { QueueItemInfo } from "@/types";

const TRANSLATE_LABEL: Record<string, string> = {
  loading: "内置引擎加载中…",
  offline: "本地引擎不可用",
  cooldown: "远程翻译冷却中",
  error: "翻译出错",
  idle: "翻译待机",
};

/** 仅异常状态显示徽章；ok/idle 不打扰 */
const TRANSLATE_BADGE_STATES = ["loading", "offline", "cooldown", "error"];

export function TranscriptPage() {
  // zustand v5：逐字段 selector（对象 selector 会因引用不稳定导致多余渲染）
  const transcripts = useAppStore((s) => s.transcripts);
  const showTranslated = useAppStore((s) => s.settings.appearance.showTranslated);
  const mainFont = useAppStore((s) => s.settings.appearance.mainFont);
  const updateAppearance = useAppStore((s) => s.updateAppearance);
  const lastError = useAppStore((s) => s.lastError);
  const engineState = useAppStore((s) => s.engineState);
  const translateState = useAppStore((s) => s.translateState);
  const session = useAppStore((s) => s.session);
  const running = useAppStore((s) => s.running);
  const overlayVisible = useAppStore((s) => s.overlayVisible);
  const overlayLocked = useAppStore((s) => s.overlayLocked);
  const setOverlayVisible = useAppStore((s) => s.setOverlayVisible);
  const setOverlayLocked = useAppStore((s) => s.setOverlayLocked);
  const clearTranscripts = useAppStore((s) => s.clearTranscripts);
  const setLastError = useAppStore((s) => s.setLastError);

  const bottomRef = useRef<HTMLDivElement>(null);

  const [queueOpen, setQueueOpen] = useState(false);
  const [queueItems, setQueueItems] = useState<QueueItemInfo[]>([]);

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [transcripts.length]);

  // 翻译队列面板打开时轮询刷新
  useEffect(() => {
    if (!queueOpen) return;
    const t = setInterval(
      () => translateQueueList().then(setQueueItems).catch(() => {}),
      1000,
    );
    return () => clearInterval(t);
  }, [queueOpen]);

  const [exportMsg, setExportMsg] = useState<string | null>(null);
  const onExport = async (fmt: "txt" | "md" | "srt" | "csv" | "json") => {
    try {
      const path = await save({
        defaultPath: `live-translator_${session?.sessionId ?? "session"}.${fmt}`,
        filters: [{ name: fmt.toUpperCase(), extensions: [fmt] }],
      });
      if (!path) return;
      const written = await exportTranscripts(fmt, path);
      setExportMsg(`已导出: ${written}`);
    } catch (e) {
      setExportMsg(`导出失败: ${e}`);
    }
  };

  return (
    <div className="flex h-full flex-col p-6">
      {/* 工具栏（已在滚动区外，不随转写内容滚动） */}
      <div className="mb-3 flex flex-wrap items-center gap-3">
        <label className="flex items-center gap-2 text-[1.05rem]">
          <input
            type="checkbox"
            className="h-5 w-5 accent-[hsl(var(--primary))]"
            checked={showTranslated}
            onChange={(e) => updateAppearance({ showTranslated: e.target.checked })}
          />
          显示译文
        </label>
        {translateState && TRANSLATE_BADGE_STATES.includes(translateState.status) && (
          <span
            className={
              "rounded-md border px-2.5 py-1 text-[0.95rem] " +
              (translateState.status === "error" || translateState.status === "offline"
                ? "border-destructive/40 bg-destructive/10 text-destructive"
                : "border-border bg-muted text-muted-foreground")
            }
            title={translateState.detail}
          >
            翻译：{TRANSLATE_LABEL[translateState.status] ?? translateState.status}
          </span>
        )}
        <MainFontControl />
        <button
          onClick={() => {
            const next = !queueOpen;
            setQueueOpen(next);
            if (next) translateQueueList().then(setQueueItems).catch(() => {});
          }}
          className={
            "rounded-md border px-4 py-2 text-[1.15rem] " +
            (queueOpen
              ? "border-primary/60 bg-primary/15 text-primary"
              : "border-border hover:bg-accent")
          }
          title="查看/管理待翻译任务（独立于转写：转写停止后队列继续消化）"
        >
          翻译队列{queueItems.length > 0 ? `（${queueItems.length}）` : ""}
        </button>
        <button
          onClick={() => {
            if (overlayVisible) {
              hideOverlay()
                .then(() => {
                  setOverlayVisible(false);
                  setOverlayLocked(false);
                })
                .catch((e) => setLastError(`悬浮窗操作失败: ${e}`));
            } else {
              showOverlay()
                .then(() => setOverlayVisible(true))
                .catch((e) => setLastError(`悬浮窗操作失败: ${e}`));
            }
          }}
          className={
            "rounded-md border px-4 py-2 text-[1.15rem] " +
            (overlayVisible
              ? "border-primary/60 bg-primary/15 text-primary"
              : "border-border hover:bg-accent")
          }
          title="桌面歌词模式悬浮窗（可拖动到任意位置）"
        >
          {overlayVisible ? "悬浮窗开" : "悬浮窗关"}
        </button>
        {overlayVisible && (
          <button
            onClick={() => {
              const next = !overlayLocked;
              setOverlayLock(next)
                .then(() => setOverlayLocked(next))
                .catch((e) => setLastError(`锁定失败: ${e}`));
            }}
            className={
              "rounded-md border px-4 py-2 text-[1.15rem] " +
              (overlayLocked
                ? "border-amber-500/60 bg-amber-500/10 text-amber-500"
                : "border-border hover:bg-accent")
            }
            title={overlayLocked ? "已锁定：悬浮窗点击穿透，鼠标操作落到下层窗口" : "锁定悬浮窗（点击穿透到下层窗口）"}
          >
            {overlayLocked ? "🔒 已锁定" : "🔓 未锁定"}
          </button>
        )}
        <div className="ml-auto flex flex-wrap gap-2">
          {(["txt", "md", "srt", "csv", "json"] as const).map((f) => (
            <button
              key={f}
              onClick={() => onExport(f)}
              className="rounded-md border border-border px-3.5 py-2 text-[1.05rem] uppercase hover:bg-accent"
              title="导出（M5 里程碑实现）"
            >
              {f}
            </button>
          ))}
          <button
            onClick={clearTranscripts}
            className="rounded-md border border-destructive px-3.5 py-2 text-[1.05rem] text-destructive hover:bg-destructive/10"
          >
            清空
          </button>
        </div>
      </div>

      {queueOpen && (
        <div className="mb-3 space-y-2 rounded-lg border border-border bg-card p-4">
          <div className="flex items-center justify-between">
            <span className="text-[1.15rem] font-medium">
              待翻译任务（{queueItems.length}）——独立于转写运行，转写停止后继续消化
            </span>
            <button
              onClick={() =>
                translateQueueClear()
                  .then(() => translateQueueList().then(setQueueItems))
                  .catch((e) => setLastError(`清空失败: ${e}`))
              }
              disabled={queueItems.length === 0}
              className="rounded-md border border-destructive px-3 py-1.5 text-[1rem] text-destructive hover:bg-destructive/10 disabled:opacity-50"
            >
              清空队列
            </button>
          </div>
          {queueItems.length === 0 ? (
            <p className="text-[1rem] text-muted-foreground">队列为空。</p>
          ) : (
            <ul className="space-y-1.5">
              {queueItems.map((q, i) => (
                <li
                  key={q.id}
                  className="flex items-center gap-3 rounded-md bg-background px-3 py-2"
                >
                  <span className="w-6 text-muted-foreground">{i + 1}.</span>
                  <span className="w-28 truncate text-[0.95rem] text-muted-foreground">
                    {q.sourceName}
                  </span>
                  <span className="min-w-0 flex-1 truncate">{q.rawPreview}</span>
                  <button
                    onClick={() =>
                      translateQueueCancel(q.id)
                        .then(() => translateQueueList().then(setQueueItems))
                        .catch((e) => setLastError(`取消失败: ${e}`))
                    }
                    className="rounded border border-destructive/50 px-2 py-0.5 text-[0.9rem] text-destructive hover:bg-destructive/10"
                  >
                    取消
                  </button>
                </li>
              ))}
            </ul>
          )}
        </div>
      )}

      {exportMsg && (
        <div className="mb-3 rounded-md border border-border bg-muted/50 px-3 py-2 text-[1.05rem] text-muted-foreground">
          {exportMsg}
        </div>
      )}

      {/* 状态条 */}
      {(lastError || (running && engineState?.status === "loading")) && (
        <div className="mb-3 rounded-md border border-border bg-muted/50 px-3 py-2 text-[1.05rem] text-muted-foreground">
          {lastError ?? "正在加载识别模型，首次可能需要数十秒…"}
        </div>
      )}

      {/* 转写列表（字体设置生效区域） */}
      <div
        className="min-h-0 flex-1 space-y-3 overflow-y-auto"
        style={{ fontFamily: mainFont.family, fontSize: `${mainFont.size}px` }}
      >
        {transcripts.length === 0 && (
          <div className="flex h-full flex-col items-center justify-center gap-2 text-muted-foreground">
            <span className="text-4xl">🎧</span>
            <p className="text-sm">
              {running
                ? "正在聆听… 说话内容将实时显示在这里"
                : "前往「音频源」选择设备并开始转写"}
            </p>
            {session && (
              <p className="text-xs opacity-70">当前会话 {session.sessionId}</p>
            )}
          </div>
        )}

        {transcripts.map((t) => (
          <article
            key={t.id}
            className="animate-fade-in-up rounded-lg border border-border bg-card p-4"
          >
            <header className="mb-1 flex items-center gap-2 text-xs text-muted-foreground">
              <time>{t.clockTime}</time>
              <span className="rounded bg-muted px-1.5 py-0.5">{t.sourceName}</span>
              {t.lang && <span className="uppercase">{t.lang}</span>}
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
