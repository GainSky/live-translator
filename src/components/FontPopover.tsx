import { useEffect, useState } from "react";
import { FONT_CANDIDATES, useAppStore } from "@/stores/appStore";
import type { FontPref } from "@/types";

/**
 * 字体设置弹层（主窗转写区 / 悬浮窗各自独立记忆）。
 * 变更即时预览并持久化（updateAppearance → save_settings）。
 */
export function FontPopover(props: {
  value: FontPref;
  onChange: (f: FontPref) => void;
  label: string;
}) {
  const [open, setOpen] = useState(false);
  const [family, setFamily] = useState(props.value.family);
  const [size, setSize] = useState(props.value.size);

  useEffect(() => {
    setFamily(props.value.family);
    setSize(props.value.size);
  }, [props.value]);

  const apply = () => {
    props.onChange({ family: family.trim() || "system-ui", size: Math.min(72, Math.max(9, size)) });
    setOpen(false);
  };

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-accent"
        title="设置字体"
      >
        🔤 {props.label}：{props.value.family} · {props.value.size}px
      </button>

      {open && (
        <div className="absolute right-0 z-20 mt-2 w-72 space-y-3 rounded-lg border border-border bg-card p-4 shadow-xl">
          <div>
            <label className="mb-1 block text-xs text-muted-foreground">字体族</label>
            <select
              value={FONT_CANDIDATES.includes(family) ? family : ""}
              onChange={(e) => setFamily(e.target.value)}
              className="w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm"
            >
              <option value="">自定义…</option>
              {FONT_CANDIDATES.map((f) => (
                <option key={f} value={f}>{f}</option>
              ))}
            </select>
            {!FONT_CANDIDATES.includes(family) && (
              <input
                value={family}
                onChange={(e) => setFamily(e.target.value)}
                placeholder="输入字体族名称"
                className="mt-2 w-full rounded-md border border-border bg-background px-2 py-1.5 text-sm"
              />
            )}
          </div>

          <div>
            <label className="mb-1 block text-xs text-muted-foreground">
              字号：{size}px
            </label>
            <input
              type="range"
              min={9}
              max={72}
              value={size}
              onChange={(e) => setSize(Number(e.target.value))}
              className="w-full"
            />
          </div>

          <div
            className="rounded-md border border-border bg-background p-3"
            style={{ fontFamily: family, fontSize: `${size}px` }}
          >
            预览 Preview 123 转写文本示例
          </div>

          <button
            onClick={apply}
            className="w-full rounded-md bg-primary px-3 py-2 text-sm font-medium text-primary-foreground"
          >
            应用并保存
          </button>
        </div>
      )}
    </div>
  );
}

/** 便捷封装：直接读写 store 的 appearance.mainFont / overlayFont */
export function MainFontControl() {
  const mainFont = useAppStore((s) => s.settings.appearance.mainFont);
  const updateAppearance = useAppStore((s) => s.updateAppearance);
  return (
    <FontPopover
      label="主窗口"
      value={mainFont}
      onChange={(mainFont) => updateAppearance({ mainFont })}
    />
  );
}
