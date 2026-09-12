# 移植蓝本笔记（源自原 backend/ 参考项目）

> 本文档归档了原 `backend/main.py`（FastAPI + sherpa-onnx 参考实现）中需要移植到 Rust 核心的
> 全部经验参数、提示词与逻辑要点。**原 backend/ 目录已于 2026-09-11 删除，本文为唯一移植依据。**
> 对应里程碑：M1（audio/asr）、M3（translate）。

---

## 1. VAD / ASR 参数（→ `src-tauri/src/asr/`）

### 1.1 Silero VAD 配置（实测调优值）

| 参数 | 值 | 说明 |
|---|---|---|
| `threshold` | 0.4 | 语音判定阈值 |
| `min_silence_duration` | 0.5s | 静音判定→切句（可由设置页调整，默认 0.5） |
| `min_speech_duration` | 0.15s | 过滤过短噪音 |
| `max_speech_duration` | 10.0s | 单句强迫切分上限（可由设置页调整） |
| `sample_rate` | 16000 | 全流水线统一采样率 |
| VAD 缓冲 | 30s | `buffer_size_in_seconds` |

### 1.2 SenseVoice 初始化参数

- `OfflineRecognizer.from_sense_voice(model=model.int8.onnx, tokens=tokens.txt, num_threads=4, use_itn=true)`
- **Provider 策略**：先尝试 `cuda`，失败（异常）则自动降级 `cpu` 并记录状态（sherpa-rs 对应 provider 配置）。
- **文本清洗**：SenseVoice 输出含特有标签，需正则剥离 `<|zh|>` `<|NEUTRAL|>` `<|speech|>` 等：`re.sub(r'<\|.*?\|>', '', text)`。

### 1.3 识别语言标签 → 语言代码映射（用于"识别=目标语言则跳过翻译"）

| SenseVoice 标签 | 语言代码 |
|---|---|
| `<|zh|>` 或 `<|yue|>` | `zh` |
| `<|en|>` | `en` |
| `<|ja|>` | `ja` |
| `<|ko|>` | `ko` |

Whisper 语言映射：`auto → ""`；`zh-TW / zh-CN → zh`。

### 1.4 翻译跳过与简繁本地转换规则

1. `target_lang == "none"` → 不翻译；
2. `target_lang == 识别语言` → 跳过翻译；
3. 识别为 `zh` 且目标是 `zh-TW/zh-CN` → **走本地 OpenCC**（s2t / t2s），不耗 LLM；转换后原文字段同步为目标字形（便于前端去重）。

## 2. 翻译提示词（→ `src-tauri/src/translate/prompts.rs`，逐字保留）

### 2.1 OpenAI 兼容接口（原 DeepSeek 路径）

- 端点：`https://api.deepseek.com/v1/chat/completions`（通用化后为用户自定义 base_url + `/chat/completions`）
- 模型：`deepseek-chat`；`temperature: 0.3`
- system prompt（逐字）：

```
你是一個專業的影片字幕即時翻譯官。請將輸入的影片語音字幕，翻譯成簡短流暢的{target_lang_name}。請只輸出翻譯後的文字，不要包含任何解釋、引言或額外標記，保持字數與原句差不多。
```

### 2.2 本地大模型（原 Ollama 原生路径）

- 调用：`POST {base}/api/generate`，`stream: false`，`options: { temperature: 0.2, num_predict: 80 }`
- system prompt（逐字，`{lang_rule}` 见下）：

```
你是一個專業的影片字幕即時翻譯官。請將使用者輸入的影片語音字幕，翻譯成簡短流暢的{target_lang_name}。{lang_rule}只輸出翻譯後的譯文本身，並輸出為「單獨一行純文字」；嚴禁輸出原文、注音、拼音、解釋、引言、括號標註、清單符號、換行或任何額外標記。
```

- `lang_rule`：
  - 目标为 `zh-TW`：`輸出必須是「繁體中文（台灣用語）」，絕對禁止輸出任何簡體字。`
  - 其他：`輸出必須是{target_lang_name}。`
- **后处理**：响应压平换行与多余空白（`" ".join(split())`）；目标为 `zh-TW` 时再过一道 OpenCC s2t 消除简体残留。

### 2.3 语言名称表（填入 `{target_lang_name}`）

```
zh-TW → 繁體中文 (Traditional Chinese)    zh-CN → 簡體中文 (Simplified Chinese)
en    → 英文 (English)                    ja    → 日文 (Japanese)
ko    → 韓文 (Korean)                     es    → 西班牙文 (Spanish)
fr    → 法文 (French)                     de    → 德文 (German)
ru    → 俄文 (Russian)
```

### 2.4 三态降级策略（本地模型连接失败处理，逐条移植）

| 情形 | 判定 | 动作 |
|---|---|---|
| 服务未启动 | `ConnectError` / `ConnectTimeout` | **本会话**跳过本地模型（置 `ollama_online=false`），后续句子直接走下一优先级，避免每句空等 |
| 模型冷启动 | `ReadTimeout`（服务有响应但加载中） | 本句改用下一优先级引擎，**不**永久关闭本地模型，下一句自动重试 |
| 响应 200 | — | 取译文；空译文则继续下一优先级 |

优先级链（参考实现）：远程 API（有 key 时）→ 本地 Ollama → Google 免费接口（`translate.googleapis.com/translate_a/single?client=gtx&sl=auto&dt=t`，超时 3s）→ 全部失败保留原文（`[未翻譯] {text}`）。
> 新版取舍：Provider 抽象下"远程/本地"由用户配置选择；Google 兜底为可选开关（默认关）。

## 3. 模型清单（→ `src-tauri/src/models/manifest`）

| 模型 | 下载 URL（GitHub Releases） | 本地结构 |
|---|---|---|
| Silero VAD | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/silero_vad.onnx` | 单文件 `silero_vad.onnx`（≈2MB） |
| SenseVoice-Small int8 | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2` | 目录内取 `model.int8.onnx` + `tokens.txt` |
| Whisper-small（后续版本） | `https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-whisper-small.tar.bz2` | encoder/decoder/tokens 文件名不固定，需按模式探测（`*encoder*.onnx`、`*decoder*.onnx`、`*tokens.txt`，优先 int8） |

## 4. 资源管理策略（→ `pipeline.rs` / `asr/mod.rs`）

- **空闲卸载**：无活动连接后 10 分钟（600s）自动释放 ASR/VAD 模型内存；每 30s 检查一次。
- **按需加载**：ASR 模型延迟到首次使用时加载，加载中事件需上报 UI（`engine_state`）。
- 每路音频源**独立 VAD 实例**（避免多路互相干扰），ASR recognizer 可共享（每句 create_stream）。
- 每句时长 = `samples.len() / 16000.0`（转写时长、SRT 时间轴用）。

## 5. 导出格式参考（→ `src-tauri/src/store/export.rs`）

参考项目 Markdown 导出格式（保留为新版 Markdown 模板基础）：

```markdown
# LiveTranslator 转写记录
*   **开始时间**：{yyyy-MM-dd HH:mm:ss}
---
### 🕒 [{HH:mm:ss} | 音频 {HH:mm:ss}]
*   **原文**：{raw}
*   **译文**：{translated}
```

## 6. 原实现明确的"不做"项（新版架构决策）

- ~~FastAPI/WebSocket 浏览器中转~~ → 进程内流水线（Tauri IPC 事件直推前端）；
- ~~前端浏览器 AudioWorklet 重采样~~ → Rust 采集线程内 rubato 重采样；
- ~~PyInstaller onedir + CUDA DLL 复制打包~~ → Tauri 单二进制；CUDA GPU 留作后续可选发行版（M7）。
