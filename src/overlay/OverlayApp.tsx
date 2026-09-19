//! 悬浮窗（桌面歌词模式）v2.3
//!
//! - 窗口自动增高：ResizeObserver 测量内容高度 → setSize（宽度保持，位置左上角不动）
//! - 前句淡出：新句到达时旧句以 CSS 动画淡出，播完自动卸载（文字不再残留）
//! - 显示逻辑（v2.3 修正）：
//!     双语模式：原文行 + 译文行（译文未到时先显示原文行，回填后原地替换）
//!     仅原文：单行原文
//!     仅译文：**不显示原文**（不可见占位保持高度，避免短暂重叠），译文到达后显示
//! - 原文行/译文行字体独立（overlayRawFont / overlayTranslatedFont）
//! - 悬浮窗本体上可直接切换显示模式与锁定（穿透）；模式切换即持久化
//! - 拖动：onMouseDown + startDragging（子元素点击不触发 drag region 的规避）

import { useEffect, useRef, useState, type CSSProperties } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import { invoke } from "@tauri-apps/api/core";
import { getSettings, onTranscript, onTranscriptUpdate, onSettingsChanged, setOverlayDisplay } from "@/lib/ipc";
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

  // 独立 WebView 实例：自行拉取设置（字体/模式/颜色）
  useEffect(() => {
    getSettings().then(setSettings).catch(() => {});
    const unsubs = [
      // 任一窗口保存设置 → 收敛到同一份配置（字体/颜色/模式实时跟随）
      onSettingsChanged((s) => setSettings(s)),
      onTranscript((t) => {
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
  const rawFont = settings.appearance.overlayRawFont;
  const trFont = settings.appearance.overlayTranslatedFont;

  const setMode = (m: OverlayMode) => {
    // 专用命令：只更新悬浮窗显示偏好，不触碰其他设置（避免跨窗口整份覆盖）
    setOverlayDisplay({ mode: m }).catch((e) =>
      console.warn("模式保存失败:", e),
    );
    setSettings({
      ...settings,
      appearance: { ...settings.appearance, overlayMode: m },
    });
  };

  const toggleLock = () => {
    const next = !locked;
    setLocked(next);
    invoke("set_overlay_lock", { locked: next }).catch(console.error);
  };

  const startDrag = (e: React.MouseEvent) => {
    if (e.button !== 0) return;
    if ((e.target as HTMLElement).closest("button")) return;
    getCurrentWindow().startDragging().catch(() => {});
  };

  const rawStyle: CSSProperties = {
    fontFamily: rawFont.family,
    fontSize: `${rawFont.size}px`,
    color: settings.appearance.overlayRawColor,
    textShadow: "0 1px 4px rgb(0 0 0 / 0.85), 0 0 2px rgb(0 0 0 / 0.9)",
  };
  const trStyle: CSSProperties = {
    fontFamily: trFont.family,
    fontSize: `${trFont.size}px`,
    color: settings.appearance.overlayTranslatedColor,
    textShadow: "0 1px 4px rgb(0 0 0 / 0.85), 0 0 2px rgb(0 0 0 / 0.9)",
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
          className="overlay-fadeout text-center leading-tight"
        >
          <OverlayText item={prev} mode={mode} rawStyle={rawStyle} trStyle={trStyle} />
        </div>
      )}

      {/* 最新句 */}
      <div className="text-center leading-tight">
        {latest ? (
          <OverlayText item={latest} mode={mode} rawStyle={rawStyle} trStyle={trStyle} />
        ) : (
          <span className="opacity-40" style={rawStyle}>
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

/** 按显示模式产出内容（长句自动换行，窗口随内容自动增高）：
 *  - raw：原文（可多行）
 *  - translated：译文未到时**不显示原文**（不可见占位保持行高，避免短暂重叠），
 *    译文到达后显示
 *  - both：原文行 + 译文行（译文未到时先显示原文行，回填后原地替换） */
function OverlayText(props: {
  item: TranscriptItem;
  mode: OverlayMode;
  rawStyle: CSSProperties;
  trStyle: CSSProperties;
}) {
  const { item, mode, rawStyle, trStyle } = props;
  const hasT = !!item.translatedText;

  if (mode === "translated") {
    if (!hasT) {
      // 不可见占位：保持行高，避免窗口高度跳动
      return <span className="block break-words opacity-0">{item.rawText}</span>;
    }
    return (
      <span className="block break-words" style={trStyle}>
        {item.translatedText}
      </span>
    );
  }
  if (mode === "both") {
    return (
      <span className="block">
        <span className="block break-words font-semibold" style={rawStyle}>
          {item.rawText}
        </span>
        {hasT && (
          <span className="mt-0.5 block break-words" style={trStyle}>
            {item.translatedText}
          </span>
        )}
      </span>
    );
  }
  return (
    <span className="block break-words font-semibold" style={rawStyle}>
      {item.rawText}
    </span>
  );
}
