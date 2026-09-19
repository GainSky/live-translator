import { useEffect, useState } from "react";
import { FONT_CANDIDATES, useAppStore } from "@/stores/appStore";
import type { FontPref } from "@/types";

/**
 * 字体设置弹层（受控开关：父层管理打开状态，支持互斥与外部点击关闭）。
 *
 * - 编辑过程仅本地预览（不污染草稿/不误触 dirty）
 * - 「保存并应用」→ onChange(f)（父层同步视图）+ onApply?.(f)（父层字段级持久化）
 * - 点击弹层外空白 → 关闭（透明遮罩层，可预期）
 */
export function FontPopover(props: {
  value: FontPref;
  onChange: (f: FontPref) => void;
  /** 「保存并应用」点击后回调（可做字段级即时持久化） */
  onApply?: (f: FontPref) => void;
  /** 受控开关；未提供时组件内部自管 */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  label: string;
}) {
  const internalOpen = useState(false);
  const open = props.open ?? internalOpen[0];
  const setOpen = (v: boolean) =>
    props.onOpenChange ? props.onOpenChange(v) : internalOpen[1](v);

  const [family, setFamily] = useState(props.value.family);
  const [size, setSize] = useState(props.value.size);

  useEffect(() => {
    if (open) {
      setFamily(props.value.family);
      setSize(props.value.size);
    }
  }, [props.value, open]);

  const apply = () => {
    const f: FontPref = {
      family: family.trim() || "system-ui",
      size: Math.min(72, Math.max(9, size)),
    };
    props.onChange(f);
    props.onApply?.(f);
    setOpen(false);
  };

  return (
    <div className="relative">
      <button
        onClick={() => setOpen(!open)}
        className="rounded-md border border-border px-3 py-1.5 text-xs hover:bg-accent"
        title="设置字体"
      >
        🔤 {props.label}：{props.value.family} · {props.value.size}px
      </button>

      {open && (
        <>
          {/* 透明遮罩：点击弹层外空白关闭 */}
          <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
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
              保存并应用
            </button>
          </div>
        </>
      )}
    </div>
  );
}

/** 便捷封装：主窗转写区字体（应用即经 updateAppearance 持久化） */
export function MainFontControl() {
  const mainFont = useAppStore((s) => s.settings.appearance.mainFont);
  const updateAppearance = useAppStore((s) => s.updateAppearance);
  const [open, setOpen] = useState(false);
  return (
    <FontPopover
      label="主窗口"
      value={mainFont}
      open={open}
      onOpenChange={setOpen}
      onChange={(mainFont) => updateAppearance({ mainFont })}
    />
  );
}
