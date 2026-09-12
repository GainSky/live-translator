import { useAppStore } from "@/stores/appStore";
import type { PageKey } from "@/App";

const NAV: { key: PageKey; label: string; icon: string }[] = [
  { key: "transcript", label: "转写", icon: "📄" },
  { key: "sources", label: "音频源", icon: "🎙️" },
  { key: "settings", label: "设置", icon: "⚙️" },
];

const THEME_CYCLE = { system: "dark", dark: "light", light: "system" } as const;
const THEME_ICON = { dark: "🌙", light: "☀️", system: "🖥️" } as const;

export function Sidebar(props: {
  current: PageKey;
  onNavigate: (k: PageKey) => void;
}) {
  const theme = useAppStore((s) => s.settings.appearance.theme);
  const running = useAppStore((s) => s.running);
  const updateAppearance = useAppStore((s) => s.updateAppearance);

  return (
    <aside className="relative flex w-20 flex-col items-center gap-3 border-r border-border bg-card py-6">
      {NAV.map((n) => (
        <button
          key={n.key}
          title={n.label}
          onClick={() => props.onNavigate(n.key)}
          className={
            "flex h-[4.4rem] w-[4.4rem] items-center justify-center rounded-xl text-[2.25rem] transition-colors " +
            (props.current === n.key
              ? "bg-primary text-primary-foreground"
              : "text-muted-foreground hover:bg-accent")
          }
        >
          {n.icon}
        </button>
      ))}

      <div className="mt-auto flex flex-col items-center gap-2.5">
        {running && (
          <span
            title="转写进行中"
            className="flex h-9 w-9 items-center justify-center"
          >
            <span className="relative flex h-5 w-5">
              <span className="absolute inline-flex h-full w-full animate-ping rounded-full bg-primary opacity-60" />
              <span className="relative inline-flex h-5 w-5 rounded-full bg-primary" />
            </span>
          </span>
        )}
        <button
          title={`主题：${theme}（点击切换）`}
          onClick={() => updateAppearance({ theme: THEME_CYCLE[theme] })}
          className="flex h-[4.4rem] w-[4.4rem] items-center justify-center rounded-xl text-[2.25rem] text-muted-foreground hover:bg-accent"
        >
          {THEME_ICON[theme]}
        </button>
      </div>
    </aside>
  );
}
