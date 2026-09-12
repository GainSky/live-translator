import { useEffect, useState } from "react";
import { useAppStore } from "@/stores/appStore";
import {
  currentSession,
  listAudioDevices,
  startPipeline,
  stopPipeline,
} from "@/lib/ipc";
import { SIGNAL_THRESHOLD, type AudioDeviceInfo, type EngineStatus } from "@/types";

const KIND_LABEL: Record<string, string> = {
  microphone: "麦克风",
  loopback: "系统声音",
  monitor: "系统声音",
  application: "应用音频",
  virtual: "虚拟设备",
  unknown: "其他",
};

const ENGINE_LABEL: Record<EngineStatus, string> = {
  unloaded: "模型未加载",
  loading: "模型加载中…",
  ready: "模型就绪",
  fallbackCpu: "CPU 模式",
  error: "模型错误",
};

export function SourcesPage() {
  const {
    devices, setDevices,
    selectedIds, toggleSelected,
    running, runningIds, setRunning,
    levels,
    engineState, setEngineState,
    session, setSession,
    setLastError,
  } = useAppStore();

  const [probing, setProbing] = useState(false);

  const refresh = () => {
    setProbing(true);
    listAudioDevices()
      .then(setDevices)
      .catch((e) => setLastError(`设备枚举失败: ${e}`))
      .finally(() => setProbing(false));
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const onStart = () => {
    startPipeline(selectedIds)
      .then(() => {
        setRunning(true, selectedIds);
        setLastError(null);
        currentSession().then(setSession).catch(() => {});
      })
      .catch((e) => setLastError(`启动失败: ${e}`));
  };

  const onStop = () => {
    stopPipeline()
      .then(() => {
        setRunning(false, []);
        setEngineState(null);
      })
      .catch((e) => setLastError(`停止失败: ${e}`));
  };

  return (
    <div className="flex h-full flex-col">
      {/* 工具栏：固定顶栏，不随列表滚动；底色与列表区明确区分 */}
      <div className="flex flex-wrap items-center gap-3 border-b border-border bg-muted/40 px-6 py-3.5 shadow-sm">
        {engineState && (
          <span
            className={
              "rounded-md border border-border px-2.5 py-1.5 text-[1.2rem] " +
              (engineState.status === "error"
                ? "border-destructive/40 bg-destructive/15 text-destructive"
                : "bg-card text-muted-foreground")
            }
          >
            {ENGINE_LABEL[engineState.status]}
            {engineState.detail ? `（${engineState.detail}）` : ""}
          </span>
        )}
        {session && (
          <span className="text-[1.2rem] text-muted-foreground">
            会话 {session.sessionId} · {session.startClock}
          </span>
        )}
        <button
          onClick={refresh}
          disabled={probing}
          className="ml-auto rounded-md border border-border bg-card px-4 py-2 text-[1.2rem] hover:bg-accent disabled:opacity-50"
        >
          {probing ? "信号检测中…" : "刷新设备"}
        </button>
        {running ? (
          <button
            onClick={onStop}
            className="rounded-md bg-destructive px-5 py-2.5 text-[1.35rem] font-medium text-destructive-foreground"
          >
            停止转写（{runningIds.length} 路）
          </button>
        ) : (
          <button
            onClick={onStart}
            disabled={selectedIds.length === 0}
            className="rounded-md bg-primary px-5 py-2.5 text-[1.35rem] font-medium text-primary-foreground disabled:opacity-50"
          >
            开始转写（{selectedIds.length} 路）
          </button>
        )}
      </div>

      {/* 设备列表：独立滚动区 */}
      <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
        {devices.length === 0 && (
          <p className="text-[1.2rem] text-muted-foreground">
            未发现音频设备。Linux 需确认 PipeWire 运行中；点击上方「刷新设备」重试。
          </p>
        )}
        <ul className="space-y-2.5">
          {devices.map((d) => (
            <DeviceRow
              key={d.id}
              device={d}
              checked={selectedIds.includes(d.id)}
              disabled={running}
              level={levels[d.id] ?? 0}
              onToggle={() => toggleSelected(d.id)}
            />
          ))}
        </ul>
      </div>

      {/* 底部提示：固定，不随列表滚动 */}
      <p className="border-t border-border px-6 py-3 text-[1.15rem] leading-relaxed text-muted-foreground">
        提示：「系统声音」为系统输出环回（Windows WASAPI Loopback / Linux PipeWire
        STREAM_CAPTURE_SINK）；「应用音频」为单个应用的播放流（如浏览器的视频声音，
        动态出现——打开新播放源后请点「刷新设备」）。可同时勾选多路。
        {running && " 转写进行中，停止后可修改选择。"}
      </p>
    </div>
  );
}

function DeviceRow(props: {
  device: AudioDeviceInfo;
  checked: boolean;
  disabled: boolean;
  level: number;
  onToggle: () => void;
}) {
  const { device: d, checked, disabled, level, onToggle } = props;
  // 声音检测条：电平三色分级（<33% 主色 / <66% 琥珀 / ≥66% 红近削波），随输入信号跳动
  const pct = Math.round(Math.min(1, level) * 100);
  const barColor =
    level >= 0.66 ? "bg-red-500" : level >= 0.33 ? "bg-amber-500" : "bg-primary";
  return (
    <li>
      <label
        className={
          "flex cursor-pointer items-center gap-4 rounded-lg border border-border bg-card p-4 " +
          (disabled ? "opacity-60" : "hover:bg-accent")
        }
      >
        <input
          type="checkbox"
          checked={checked}
          disabled={disabled}
          onChange={onToggle}
          className="h-6 w-6 accent-[hsl(var(--primary))]"
        />
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[1.45rem] font-medium">
            {d.name}
            {d.signal >= SIGNAL_THRESHOLD && (
              <span
                className="ml-2 inline-block translate-y-[-1px] rounded bg-primary/15 px-1.5 py-0.5 align-middle text-[0.85rem] font-normal text-primary"
                title={`信号测试峰值 ${(d.signal * 100).toFixed(0)}%`}
              >
                有声 🔊
              </span>
            )}
          </span>
          <span className="text-[1.2rem] text-muted-foreground">
            {KIND_LABEL[d.kind] ?? "音频源"}
            {d.isDefault && " · 默认"}
            {" · "}
            {d.sampleRate / 1000}kHz · {d.channels}ch
          </span>
        </span>
        <span className="flex items-center gap-2.5" title="输入电平（转写运行时跳动）">
          <span className="h-7 w-52 overflow-hidden rounded-full border border-border/60 bg-muted">
            <span
              className={"block h-full rounded-full transition-[width] duration-75 " + barColor}
              style={{ width: `${pct}%` }}
            />
          </span>
          <span
            className={
              "w-12 text-right text-[1rem] tabular-nums " +
              (pct > 0 ? "text-muted-foreground" : "text-muted-foreground/40")
            }
          >
            {pct}%
          </span>
        </span>
      </label>
    </li>
  );
}
