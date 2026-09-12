import { useEffect, useState } from "react";
import { getSettings, saveSettings, testTranslation } from "@/lib/ipc";
import { useAppStore } from "@/stores/appStore";
import { FontPopover } from "@/components/FontPopover";
import type { Settings } from "@/types";

const SOURCE_LANGS = [
  ["auto", "自动检测"],
  ["zh", "中文"],
  ["en", "英语"],
  ["ja", "日语"],
  ["ko", "韩语"],
  ["yue", "粤语"],
];
const TARGET_LANGS = [
  ["zh-CN", "简体中文"],
  ["en", "英语"],
  ["ja", "日语"],
  ["ko", "韩语"],
  ["es", "西班牙语"],
  ["fr", "法语"],
  ["de", "德语"],
  ["ru", "俄语"],
  ["zh-TW", "繁体中文"],
];

export function SettingsPage() {
  const setSettings = useAppStore((s) => s.setSettings);
  const [draft, setDraft] = useState<Settings | null>(null);
  const [dirty, setDirty] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveErr, setSaveErr] = useState<string | null>(null);
  // 翻译测试
  const [testText, setTestText] = useState("今天天气真不错，适合出去走走。");
  const [testResult, setTestResult] = useState<string | null>(null);
  const [testing, setTesting] = useState(false);

  useEffect(() => {
    getSettings()
      .then((s) => setDraft(s))
      .catch((e) => console.warn("载入设置失败:", e));
  }, []);

  if (!draft) {
    return <div className="p-6 text-sm text-muted-foreground">载入设置中…</div>;
  }

  /** 不可变更新辅助：深拷贝后修改再提交 */
  const edit = (fn: (d: Settings) => void) => {
    const d = structuredClone(draft);
    fn(d);
    setDraft(d);
    setDirty(true);
    setSaved(false);
  };

  const onSave = () => {
    if (!draft) return;
    saveSettings(draft)
      .then(() => {
        setSettings(draft); // 同步全局（主题/字体即时生效）
        setDirty(false);
        setSaved(true);
        setSaveErr(null);
      })
      .catch((e) => setSaveErr(`保存失败: ${e}`));
  };

  const onTest = () => {
    setTesting(true);
    setTestResult(null);
    testTranslation(testText, draft.translation)
      .then((r) => setTestResult(`✅ ${r}`))
      .catch((e) => setTestResult(`❌ ${e}`))
      .finally(() => setTesting(false));
  };

  return (
    <div className="mx-auto max-w-3xl space-y-6 p-6">
      <header className="flex items-center gap-3">
        <button
          onClick={onSave}
          disabled={!dirty}
          className="rounded-md bg-primary px-4 py-2 text-sm font-medium text-primary-foreground disabled:opacity-50"
        >
          保存设置
        </button>
        {saved && <span className="text-xs text-muted-foreground">已保存 ✓</span>}
        {dirty && <span className="text-xs text-amber-500">有未保存的修改</span>}
        {saveErr && <span className="text-xs text-destructive">{saveErr}</span>}
      </header>

      {/* ===== 识别 ===== */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-5">
        <h2 className="text-[1.35rem] font-semibold">识别</h2>
        <Field label="源语言">
          <select
            value={draft.asr.sourceLang}
            onChange={(e) => edit((d) => (d.asr.sourceLang = e.target.value))}
            className="input"
          >
            {SOURCE_LANGS.map(([v, l]) => (
              <option key={v} value={v}>{l}</option>
            ))}
          </select>
        </Field>
        <div className="grid grid-cols-2 gap-3 lg:grid-cols-4">
          <Field label="VAD 阈值 (0.1–0.9)">
            <input
              type="number" step="0.05" min="0.1" max="0.9"
              value={draft.asr.vad.threshold}
              onChange={(e) => edit((d) => (d.asr.vad.threshold = Number(e.target.value)))}
              className="input"
            />
          </Field>
          <Field label="静音切句 (ms)">
            <input
              type="number" step="50" min="100" max="2000"
              value={draft.asr.vad.minSilenceMs}
              onChange={(e) => edit((d) => (d.asr.vad.minSilenceMs = Number(e.target.value)))}
              className="input"
            />
          </Field>
          <Field label="最短语音 (ms)">
            <input
              type="number" step="50" min="50" max="1000"
              value={draft.asr.vad.minSpeechMs}
              onChange={(e) => edit((d) => (d.asr.vad.minSpeechMs = Number(e.target.value)))}
              className="input"
            />
          </Field>
          <Field label="最长单句 (ms)">
            <input
              type="number" step="500" min="2000" max="30000"
              value={draft.asr.vad.maxSpeechMs}
              onChange={(e) => edit((d) => (d.asr.vad.maxSpeechMs = Number(e.target.value)))}
              className="input"
            />
          </Field>
        </div>
        <p className="text-xs text-muted-foreground">
          引擎：SenseVoice-Small int8 + Silero VAD（默认参数移植自参考实现）。VAD 参数变更在下次「开始转写」时生效。
        </p>
      </section>

      {/* ===== 翻译 ===== */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-5">
        <h2 className="text-[1.35rem] font-semibold">翻译</h2>
        <Field label="启用翻译">
          <input
            type="checkbox"
            checked={draft.translation.enabled}
            onChange={(e) => edit((d) => (d.translation.enabled = e.target.checked))}
          />
        </Field>
        <Field label="目标语言">
          <select
            value={draft.translation.targetLang}
            onChange={(e) => edit((d) => (d.translation.targetLang = e.target.value))}
            className="input"
          >
            {TARGET_LANGS.map(([v, l]) => (
              <option key={v} value={v}>{l}</option>
            ))}
          </select>
        </Field>
        <Field label="翻译引擎">
          <div className="flex flex-wrap gap-4">
            <label className="flex items-center gap-2">
              <input
                type="radio" name="provider" value="builtin"
                checked={draft.translation.provider === "builtin"}
                onChange={() => edit((d) => (d.translation.provider = "builtin"))}
              />
              内置引擎（离线·纯本地）
            </label>
            <label className="flex items-center gap-2">
              <input
                type="radio" name="provider" value="openai-compatible"
                checked={draft.translation.provider === "openai-compatible"}
                onChange={() => edit((d) => (d.translation.provider = "openai-compatible"))}
              />
              OpenAI 兼容接口
            </label>
            <label className="flex items-center gap-2">
              <input
                type="radio" name="provider" value="local-http"
                checked={draft.translation.provider === "local-http"}
                onChange={() => edit((d) => (d.translation.provider = "local-http"))}
              />
              本地 HTTP 服务（Ollama 等）
            </label>
          </div>
        </Field>

        {draft.translation.provider === "builtin" && (
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="内置模型">
              <select
                value={draft.translation.builtin.model}
                onChange={(e) => edit((d) => (d.translation.builtin.model = e.target.value))}
                className="input"
              >
                <option value="qwen2.5-3b-instruct-q4_k_m.gguf">Qwen2.5-3B Q4（≈2GB，推荐）</option>
                <option value="qwen2.5-1.5b-instruct-q4_k_m.gguf">Qwen2.5-1.5B Q4（≈1GB，轻量）</option>
                <option value="qwen2.5-7b-instruct-q4_k_m.gguf">Qwen2.5-7B Q4（≈4.7GB，质量优先）</option>
              </select>
            </Field>
            <Field label="运行方式">
              <p className="text-[1rem] text-muted-foreground">
                纯 Rust 进程内推理，无需安装任何外部软件；模型文件放在 models/ 目录
                （下载地址见 readme §10.2）。切换模型后下次开始转写生效。
              </p>
              <p className="text-[1rem] text-amber-500">
                ⚠️ 内置引擎为 CPU 推理，单句约 1~3 分钟，适合离线小段落；实时字幕建议用远程
                API 或本地 HTTP 服务（Ollama 走 GPU）。
              </p>
            </Field>
          </div>
        )}

        {draft.translation.provider === "openai-compatible" && (
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="Base URL（含 /v1）">
              <input
                value={draft.translation.openai.baseUrl}
                onChange={(e) => edit((d) => (d.translation.openai.baseUrl = e.target.value))}
                className="input"
              />
            </Field>
            <Field label="模型">
              <input
                value={draft.translation.openai.model}
                onChange={(e) => edit((d) => (d.translation.openai.model = e.target.value))}
                className="input"
              />
            </Field>
            <Field label="API Key">
              <input
                type="password"
                value={draft.translation.openai.apiKey}
                onChange={(e) => edit((d) => (d.translation.openai.apiKey = e.target.value))}
                className="input"
              />
            </Field>
            <Field label="temperature (0–2)">
              <input
                type="number" step="0.1" min="0" max="2"
                value={draft.translation.openai.temperature}
                onChange={(e) => edit((d) => (d.translation.openai.temperature = Number(e.target.value)))}
                className="input"
              />
            </Field>
          </div>
        )}

        {draft.translation.provider === "local-http" && (
          <div className="grid gap-3 md:grid-cols-2">
            <Field label="Base URL（Ollama OpenAI 兼容端点）">
              <input
                value={draft.translation.local.baseUrl}
                onChange={(e) => edit((d) => (d.translation.local.baseUrl = e.target.value))}
                className="input"
              />
            </Field>
            <Field label="模型">
              <input
                value={draft.translation.local.model}
                onChange={(e) => edit((d) => (d.translation.local.model = e.target.value))}
                className="input"
              />
            </Field>
          </div>
        )}

        <Field label="Google 免费接口兜底（主通道失败时使用）">
          <input
            type="checkbox"
            checked={draft.translation.googleFallback}
            onChange={(e) => edit((d) => (d.translation.googleFallback = e.target.checked))}
          />
        </Field>

        {/* 测试翻译（使用已保存配置） */}
        <div className="space-y-2 rounded-md border border-border bg-background p-3">
          <p className="text-xs text-muted-foreground">
            测试翻译使用上方表单的当前配置（内置引擎为 CPU 推理，测速较慢属正常）：
          </p>
          <div className="flex gap-2">
            <input
              value={testText}
              onChange={(e) => setTestText(e.target.value)}
              className="input flex-1"
            />
            <button
              onClick={onTest}
              disabled={testing || !testText.trim()}
              className="rounded-md border border-border px-4 py-1.5 text-sm hover:bg-accent disabled:opacity-50"
            >
              {testing ? "请求中…" : "测试"}
            </button>
          </div>
          {testResult && (
            <p className="rounded bg-muted px-3 py-2 text-sm">{testResult}</p>
          )}
        </div>
      </section>

      {/* ===== 外观 ===== */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-5">
        <h2 className="text-[1.35rem] font-semibold">外观</h2>
        <Field label="主题">
          <select
            value={draft.appearance.theme}
            onChange={(e) => edit((d) => (d.appearance.theme = e.target.value as Settings["appearance"]["theme"]))}
            className="input"
          >
            <option value="system">跟随系统</option>
            <option value="light">浅色</option>
            <option value="dark">深色</option>
          </select>
        </Field>
        <Field label="默认显示译文">
          <input
            type="checkbox"
            checked={draft.appearance.showTranslated}
            onChange={(e) => edit((d) => (d.appearance.showTranslated = e.target.checked))}
          />
        </Field>
        <Field label="主窗口转写区字体">
          <FontPopover
            label="编辑"
            value={draft.appearance.mainFont}
            onChange={(mainFont) => edit((d) => (d.appearance.mainFont = mainFont))}
          />
          <span className="text-xs text-muted-foreground">
            {draft.appearance.mainFont.family} · {draft.appearance.mainFont.size}px
          </span>
        </Field>
        <Field label="悬浮窗字体（M4 生效）">
          <FontPopover
            label="编辑"
            value={draft.appearance.overlayFont}
            onChange={(overlayFont) => edit((d) => (d.appearance.overlayFont = overlayFont))}
          />
          <span className="text-xs text-muted-foreground">
            {draft.appearance.overlayFont.family} · {draft.appearance.overlayFont.size}px
          </span>
        </Field>
      </section>

      {/* ===== 高级 ===== */}
      <section className="space-y-3 rounded-lg border border-border bg-card p-5">
        <h2 className="text-[1.35rem] font-semibold">高级</h2>
        <Field label="空闲 N 分钟后卸载模型（M3 接入定时器）">
          <input
            type="number" min="1" max="120"
            value={draft.advanced.idleUnloadMinutes}
            onChange={(e) => edit((d) => (d.advanced.idleUnloadMinutes = Number(e.target.value)))}
            className="input w-32"
          />
        </Field>
      </section>
    </div>
  );
}

function Field(props: { label: string; children: React.ReactNode }) {
  return (
    <label className="flex flex-col gap-1.5 text-[1.05rem]">
      <span className="text-muted-foreground">{props.label}</span>
      {props.children}
    </label>
  );
}
