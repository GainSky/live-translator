import { useEffect, useState } from "react";
import { onTranscript } from "@/lib/ipc";
import type { OverlayMode, TranscriptItem } from "@/types";

// TODO(M4): 拖动定位（data-tauri-drag-region）、锁定点击穿透、三显示模式切换控制、
//           位置记忆、字体/颜色/描边设置、上一句渐隐动画
// 拖动：给根容器加 data-tauri-drag-region 属性（需 core:window:allow-start-dragging 权限）
export function OverlayApp() {
  const [mode, setMode] = useState<OverlayMode>("both");
  const [latest, setLatest] = useState<TranscriptItem | null>(null);
  const [prev, setPrev] = useState<TranscriptItem | null>(null);

  useEffect(() => {
    const un = onTranscript((t) => {
      setPrev((p) => p ?? null);
      setLatest(t);
    });
    return () => {
      un.then((f) => f()).catch(() => {});
    };
  }, []);

  const showRaw = mode === "both" || mode === "raw";
  const showTranslated = mode === "both" || mode === "translated";

  return (
    <div
      data-tauri-drag-region
      className="flex h-screen w-screen flex-col justify-center gap-1 px-6 py-3"
    >
      {prev && (
        <div className="truncate text-sm opacity-40">
          <OverlayText item={prev} mode={mode} dim />
        </div>
      )}
      {latest ? (
        <div className="animate-fade-in-up">
          <OverlayText item={latest} mode={mode} />
        </div>
      ) : (
        <div className="text-center text-sm opacity-40" data-tauri-drag-region>
          LiveTranslator 悬浮字幕（M4 实现完整交互）
        </div>
      )}
      {/* 模式临时切换按钮（M4 移入设置/快捷键） */}
      <div className="mt-1 flex justify-center gap-2 text-[10px] opacity-50">
        {(["both", "raw", "translated"] as const).map((m) => (
          <button key={m} onClick={() => setMode(m)}>
            {m === "both" ? "原文+译文" : m === "raw" ? "仅原文" : "仅译文"}
          </button>
        ))}
      </div>
      {showRaw && null}
      {showTranslated && null}
    </div>
  );
}

function OverlayText(props: {
  item: TranscriptItem;
  mode: OverlayMode;
  dim?: boolean;
}) {
  const { item, mode, dim } = props;
  return (
    <span className={dim ? "opacity-70" : undefined}>
      {(mode === "both" || mode === "raw") && (
        <span className="font-semibold" style={{ textShadow: "0 1px 4px rgb(0 0 0 / 0.8)" }}>
          {item.rawText}
        </span>
      )}
      {(mode === "both" || mode === "translated") && item.translatedText && (
        <span
          className="ml-3 text-muted-foreground"
          style={{ textShadow: "0 1px 4px rgb(0 0 0 / 0.8)" }}
        >
          {item.translatedText}
        </span>
      )}
    </span>
  );
}
