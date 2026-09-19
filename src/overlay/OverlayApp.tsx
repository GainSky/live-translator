//! 悬浮窗（桌面歌词模式）v2.2
//!
//! - 窗口自动增高：ResizeObserver 测量内容高度 → setSize（宽度保持，位置左上角不动）
//! - 前句淡出：新句到达时旧句以 CSS 动画淡出，播完自动卸载（文字不再残留）
//! - 行数与显示模式：有译文按模式（双语两行/仅原文/仅译文）；无译文回落单行原文
//! - 悬浮窗本体上切换模式 + 锁定（穿透）；模式切换即持久化
//! - 按钮固定尺寸 + 填充式高亮，杜绝回流导致的错位

import { useEffect, useRef, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { getSettings, onTranscript, onTranscriptUpdate } from "@/lib/ipc";
import { DEFAULT_SETTINGS, type OverlayMode, type Settings, type TranscriptItem } from "@/types";

const MODE_LABEL: Record<OverlayMode, string> = {
  both: "双语",
  raw: "原文",
  translated: "译文",
};

export function OverlayApp() {
  const [settings, setSettings] = useState<Settings>(DEFAULT_SETTINGS);
  const [latest, setLatest] = useState<TranscriptItem | null>(null);
  const [prev, setPrev] = useState<TranscriptItem | null>(null);
  const [locked, setLocked] = useState(false);
  const latestRef = useRef<TranscriptItem | null>(null);
  const contentRef = useRef<HTMLDivElement>(null);
  const lastSizeRef = useRef({ w: 0, h: 0 });

  // 独立 WebView 实例：自行拉取设置（字体/模式）
  useEffect(() => {
    getSettings().then(setSettings).catch(() => {});
    const unsubs = [
      onTranscript((t) => {
        // 旧句转入淡出槽；无旧句则直接显示新句
        setPrev(latestRef.current);
        latestRef.current = t;
        setLatest(t);
      }),
      onTranscriptUpdate((u) => {
        const apply = (item: TranscriptItem | null) =>
          item && item.id === u.id
            ? { ...item, translatedText: u.translatedText, provider: u.provider }
            : item;
        latestRef.current = apply(latestRef.current);
        setLatest((cur) => apply(cur));
        setPrev((cur) => apply(cur));
      }),
    ];
    return () => unsubs.forEach((p) => p.then((f) => f()).catch(() => {}));
  }, []);

  // 窗口自动增高：内容高度变化 → setSize（宽度保持窗口当前宽）
  useEffect(() => {
    const el = contentRef.current;
    if (!el) return;
    const ro = new ResizeObserver(() => {
      const rect = el.getBoundingClientRect();
      const w = Math.ceil(rect.width);
      const h = Math.ceil(rect.height);
      const last = lastSizeRef.current;
      if (Math.abs(w - last.w) < 2 && Math.abs(h - last.h) < 2) return;
      lastSizeRef.current = { w, h };
      getCurrentWindow()
        .setSize(new LogicalSize(w, h))
        .catch(console.error);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const mode: OverlayMode = settings.appearance.overlayMode as OverlayMode;
  const font = settings.appearance.overlayFont;

  const setMode = (m: OverlayMode) => {
    const next: Settings = {
      ...settings,
      appearance: { ...settings.appearance, overlayMode: m },
    };
    setSettings(next);
    invoke("save_settings", { settings: next }).catch((e) =>
      console.warn("模式保存失败:", e),
    );
  };

  const toggleLock = () => {
    const next = !locked;
    setLocked(next);
    invoke("set_overlay_lock", { locked: next }).catch(console.error);
  };

  const startDrag = (e: React.MouseEvent) => {
    // 左键且未点到按钮/控制条 → 开始拖动窗口
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    getCurrentWindow().startDragging().catch(() => {});
  };

  return (
    <div
      ref={contentRef}
      onMouseDown={startDrag}
      className={"flex flex-col gap-1 px-5 py-2 " + (locked ? "cursor-default" : "cursor-move")}
    >
      {/* 前句：新句到达时进入淡出动画，播完自动卸载（onAnimationEnd） */}
      {prev && (
        <div
          key={prev.id}
          onAnimationEnd={() => setPrev(null)}
          className="overlay-fadeout truncate text-center leading-tight"
          style={{ fontFamily: font.family, fontSize: `${font.size}px` }}
        >
          <OverlayText item={prev} mode={mode} />
        </div>
      )}

      {/* 最新句 */}
      <div
        className="truncate text-center leading-tight"
        style={{ fontFamily: font.family, fontSize: `${font.size}px` }}
      >
        {latest ? (
          <OverlayText item={latest} mode={mode} />
        ) : (
          <span className="opacity-40">
            等待转写内容…
          </span>
        )}
      </div>

      {/* 控制条：固定尺寸按钮，高亮即按钮本身 */}
      <div className="flex shrink-0 items-center justify-center gap-1.5 pt-0.5">
        {(Object.keys(MODE_LABEL) as OverlayMode[]).map((m) => (
          <button
            key={m}
            onClick={() => setMode(m)}
            className={
              "h-7 w-[4.5rem] rounded-md border text-[0.95rem] leading-none transition-colors " +
              (mode === m
                ? "border-primary/80 bg-primary/70 font-medium text-white"
                : "border-white/25 bg-black/40 text-white/85 hover:bg-black/70")
            }
            title={`显示模式：${MODE_LABEL[m]}`}
          >
            {MODE_LABEL[m]}
          </button>
        ))}
        <button
          onClick={toggleLock}
          className="h-7 w-10 rounded-md border border-white/25 bg-black/40 text-[0.95rem] text-white/85 hover:bg-black/70"
          title={locked ? "已锁定（鼠标穿透）——解锁请用主窗转写页按钮" : "锁定悬浮窗（点击穿透）"}
        >
          {locked ? "🔒" : "🔓"}
        </button>
      </div>
    </div>
  );
}

/** 按显示模式产出内容（行截断由外层 truncate 保证）：
 *  - raw：单行原文
 *  - translated：单行译文（无译文回落原文）
 *  - both：两行（原文行 + 译文行）；无译文 → 单行原文 */
function OverlayText(props: { item: TranscriptItem; mode: OverlayMode }) {
  const { item, mode } = props;
  const hasT = !!item.translatedText;
  const shadow = { textShadow: "0 1px 4px rgb(0 0 0 / 0.85), 0 0 2px rgb(0 0 0 / 0.9)" };

  if (mode === "translated" && hasT) {
    return (
      <span className="block truncate" style={shadow}>
        {item.translatedText}
      </span>
    );
  }
  if (mode === "both" && hasT) {
    return (
      <span className="block">
        <span className="block truncate font-semibold" style={shadow}>
          {item.rawText}
        </span>
        <span
          className="mt-0.5 block truncate text-[0.75em] text-amber-200/90"
          style={shadow}
        >
          {item.translatedText}
        </span>
      </span>
    );
  }
  return (
    <span className="block truncate font-semibold" style={shadow}>
      {item.rawText}
    </span>
  );
}
