# LiveTranslator — 跨平台实时语音转写翻译应用 · 项目方案

> 状态：**已选型 ✅ 方案 A（Tauri 2 + Rust + Web 前端）**，ASR 首发引擎 SenseVoice+VAD，模型首次启动下载。决策明细见 §9。
> 参考代码：`backend/` 目录（FastAPI + sherpa-onnx 后端，其 ASR/VAD/翻译逻辑作为 §2.6 所述的移植蓝本）

---

## 1. 项目目标与需求

构建一个 **Windows / Linux 跨平台独立桌面应用**，可打包为**单文件可执行程序**，提供实时语音转写与翻译能力。

### 1.1 功能需求

| 编号 | 需求 | 说明 / 验收要点 |
|---|---|---|
| F1 | 音频源枚举与多路采集 | 列出系统全部音频输入源（麦克风、虚拟声卡，Windows 需含**系统声音环回 WASAPI Loopback**，Linux 需含 **PulseAudio/PipeWire Monitor**）；用户可勾选一路或多路同时采集 |
| F2 | 语音识别（语言可设定） | 基于 sherpa-onnx 本地离线识别；源语言可指定或 auto；每路音频独立转写 |
| F3 | 翻译（可选开关） | 支持两种翻译后端：① 本地大模型（Ollama / llama.cpp server 等）② OpenAI Compatible 接口（自定义 base_url / api_key / model，覆盖 DeepSeek、OpenAI、各类网关）。目标语言可设定 |
| F4 | 转写记录与显示 | 主界面滚动显示全部原文与译文（译文显示可开关）；每条含时间戳与来源标签；支持导出 |
| F5 | 导出 | 至少支持 TXT / Markdown / SRT / CSV / JSON |
| F6 | 字体设置 | 主窗口转写区与悬浮窗的字体族、字号均可独立设置 |
| F7 | 桌面悬浮窗 | 类"桌面歌词"：无边框、透明背景、置顶、可拖动、可锁定点击穿透；显示模式三选一：原文+译文（仅翻译开启时可选）/ 仅原文 / 仅译文 |
| F8 | 界面外观 | 现代美观 UI；暗色模式（跟随系统 + 手动切换） |

### 1.2 非功能需求

- **N1 跨平台**：Windows 10/11 x64、主流 Linux x64（X11/Wayland，Wayland 下悬浮窗能力受限，见 §2.4）
- **N2 单文件分发**：Windows 单个 `.exe`、Linux 单个可执行文件（或 AppImage），免安装、免 Python/Node 运行时
- **N3 低延迟**：转写延迟目标（句子切分后）≤ 1s（CPU）；翻译延迟取决于所选后端
- **N4 隐私**：ASR 全程本地推理；翻译可全本地（Ollama）或远程（OpenAI 兼容接口），由用户选择
- **N5 资源占用**：空闲时内存 < 300MB；模型空闲自动卸载（沿用参考项目思路）

---

## 2. 关键技术调研

### 2.1 音频源枚举与采集（含系统声音环回）

| 平台 | 麦克风 | 系统声音环回 | 可用库（Python / Rust） |
|---|---|---|---|
| Windows | WASAPI/MME 设备枚举 | **WASAPI Loopback**（无需驱动，稳定） | Python：`soundcard`（含环回）、`pyaudiowpatch`；Rust：`wasapi` crate（支持 loopback）、`cpal`（**不支持**环回，仅麦克风） |
| Linux | PulseAudio/PipeWwire source | **PulseAudio Monitor 设备**（PipeWire 的 pulse 兼容层直接可用） | Python：`soundcard`、`sounddevice`（可列出 monitor）；Rust：`cpal`（pulse host 可见 monitor）、`libpulse-binding` |

#### PipeWire 支持说明（已确认 ✅，用户环境为 PipeWire）

cpal 自 v0.18 起在 Linux 上提供**原生 PipeWire 后端**（feature = `pipewire`），不仅兼容，而且是环回采集的最优路径：

1. **麦克风**：枚举 `Audio/Source` 节点（含蓝牙/USB/PCI 分类识别），直接采集。
2. **系统声音环回**：cpal 将 PipeWire 的 `Audio/Sink` 节点暴露为**可输入设备**（Duplex），打开输入流时自动携带 `STREAM_CAPTURE_SINK=true` 属性——即 PipeWire 原生的"捕获该 sink 正在播放的音频"机制，等效且优于 PulseAudio 的 monitor 方案（跟随默认 sink 切换）。
3. **回退链**：`pipewire` 后端不可用时自动回退 `pulseaudio` feature（经 pipewire-pulse 兼容层，monitor 即普通 source）；再回退 ALSA host 的 `pipewire` 桥接设备（不推荐，默认设备会被独占、采样率不匹配易爆音，仅作极端兜底）。
4. **构建/运行依赖**：仅构建期需要 `libpipewire-0.3-dev` + `libasound2-dev` + clang（bindgen）；运行期只要求系统装有 PipeWire ≥ 0.3.53（2021 年后所有主流发行版默认满足），无额外部署负担。
5. **注意事项**：pipewire-rs 绑定声明为 WIP，需在 Cargo.lock 锁定经 cpal 验证的版本；流采样率应匹配 PW 时钟率或使用原生 PW 后端（cpal 原生后端对此不敏感），避免兼容层重采样爆音。
6. **加分项（预留）**：经 PulseAudio API 可按应用流采集（`monitor-stream`，即只录某个 App 的声音），作为 V1.x 增强项。


要点：
- 采集线程与 UI 隔离：每路音频一个独立采集线程 + 有界队列，避免相互阻塞。
- 采集得到原生采样率/声道 → 统一重采样至 **16kHz 单声道 float32**（sherpa-onnx 要求；重采样用 `soxr`/`resample_poly`，参考项目是把重采样放在前端浏览器做的，桌面版需自行实现）。
- 多路策略默认"分离模式"（各自独立 VAD+识别，转写带来源标签）；可选"混音模式"（多路叠加为单一流，适合多人会议场景）。

### 2.2 语音识别（sherpa-onnx）

sherpa-onnx（k2-fsa，Apache-2.0）提供 Python/C/C++ 绑定，模型均为 ONNX 本地推理，CPU 即可实时，可选 CUDA 加速。引擎选型：

| 引擎 | 语言 | 模式 | 延迟 | 备注 |
|---|---|---|---|---|
| **SenseVoice-Small (int8)** | 中/英/日/韩/粤 | 离线整句（VAD 切段） | ≈ 句长 + 0.2~0.8s 推理 | **参考项目现用方案**，精度/速度平衡最佳，多语言识别 + ITN |
| Whisper small/base (int8) | 99 种语言 | 离线整句（VAD 切段） | 较高（small CPU 约实时率 0.5~1×） | 参考项目已支持，语言可强制指定 |
| 流式 Zipformer（如 bilingual-zh-en） | 中/英（其余需换模型） | 流式 | **200~500ms 出字** | 后续低延迟增强项，接口已预留 |

- **VAD 切句**：Silero VAD（参考项目参数可直接复用：threshold 0.4 / min_silence 0.5s / max_speech 10s）。
- 设计上做 **ASR 引擎抽象层**：`Recognizer` 接口 + SenseVoice/Whisper/StreamingZipformer 实现，运行时可切换、按需加载、空闲释放（复用参考项目 `release_asr_model()` 逻辑）。
- CUDA：Windows 可选 `sherpa-onnx==x.x.x+cuda12.cudnn9` GPU wheel（参考项目 requirements 已注明安装源）；GPU 加载失败自动降级 CPU（参考项目已有此逻辑）。

### 2.3 翻译（本地大模型 + OpenAI 兼容接口）

统一抽象为一个 **Provider 接口**，两种内置实现：

1. **OpenAICompatibleProvider**：`{base_url, api_key, model, prompt 模板, temperature}`，POST `/v1/chat/completions`。
   - 覆盖：DeepSeek、OpenAI、Moonshot、OpenRouter、one-api/new-api 网关等一切兼容端点。
2. **LocalLLMProvider（本地大模型）**：
   - 首选 **Ollama**：其 `/v1/chat/completions` 本身即 OpenAI 兼容，可直接由 Provider 1 覆盖（`http://127.0.0.1:11434/v1`）；另保留原生 `/api/generate` 调用作为兼容路径（参考项目已实现）。
   - 同理支持 llama.cpp server（`/v1` 端点）、LM Studio。

工程要点（均可从参考项目移植）：
- **识别语言 = 目标语言则跳过翻译**（SenseVoice 输出含 `<|zh|>` 等语言标签可用于判定）；
- **中文简繁**：OpenCC 本地转换（zh-TW/zh-CN 互转不耗 LLM）；
- **翻译队列**：串行 + 超时降级（Ollama 冷启动 ReadTimeout 不永久关闭、连接失败则本会话跳过——参考项目的三态处理逻辑直接复用）；
- **上下文翻译**：请求携带前 1~2 句已翻文本作为 few-shot 上下文，提升口语连贯性（新增）；
- 译文后处理：剥离 LLM 解释性输出、压平换行（参考项目已有）。

### 2.4 桌面悬浮窗能力对比

| 能力 | Tauri 2 | Electron | PySide6 (Qt) |
|---|---|---|---|
| 无边框 + 透明背景 | ✅ | ✅ | ✅（FramelessWindowHint + WA_TranslucentBackground） |
| 窗口置顶 | ✅ | ✅ | ✅（WindowStaysOnTopHint） |
| 点击穿透（锁定） | ✅ `set_ignore_cursor_events` | ✅ `setIgnoreMouseEvents` | ✅ `WindowTransparentForInput`（Windows/X11） |
| 拖动定位 | ✅ | ✅ | ✅（自实现 mouseMove） |
| 任务栏隐藏 | ✅ skip taskbar | ✅ | ✅ Tool 窗口 |
| 文本渐隐动画 | CSS（最灵活） | CSS | QPropertyAnimation / QML |

⚠️ **Wayland 限制（三方案共同）**：GNOME 默认不支持 always-on-top 与点击穿透；KDE Wayland 基本可用。Linux 下悬浮窗以 **X11 完整支持、Wayland 尽力支持**为验收口径（主窗口不受影响）。

### 2.5 单文件打包

| 方案 | Windows | Linux | 体积（不含模型） |
|---|---|---|---|
| PyInstaller onefile（Python 系） | 单 `.exe`（自解压启动 3~8s） | 单可执行文件（需在老 glibc 基线如 Ubuntu 20.04 构建；或 AppImage） | 80~150MB |
| Tauri 2 | 单 `.exe`（依赖系统 WebView2，Win10/11 自带） | AppImage（单文件） | **10~30MB** |
| Electron | portable 单 `.exe`（自解压） | AppImage | 200MB+（若含 Python sidecar 则 300MB+） |

- **模型不内嵌**（SenseVoice int8 ≈ 220MB、Whisper small int8 ≈ 200MB、Silero VAD ≈ 2MB；内嵌会让单文件膨胀到数百 MB 且启动自解压极慢、易触发误报）。统一采用：**首次启动向导下载模型**至用户数据目录（支持断点续传、镜像加速、sha256 校验、离线导入）；同时支持"便携模式"（exe 同目录 `models/` 文件夹优先加载）。
- Windows onefile exe 有杀软误报概率：可提供签名构建 + 附 onedir 版本兜底。

### 2.6 参考代码（backend/）复用度评估

| 参考模块 | 复用情况 |
|---|---|
| Silero VAD 初始化/切段参数（`init_vad`） | ✅ 原样复用 |
| SenseVoice/Whisper 加载 + CUDA 降级 + 空闲释放（`init_*`/`release_asr_model`/`monitor_idle_timeout`） | ✅ 原样复用 |
| 识别后语言标签判定 + 跳过翻译 + OpenCC 简繁 | ✅ 原样复用 |
| Ollama 原生 / DeepSeek 翻译 + 超时降级策略（`translate_text`） | ✅ 逻辑复用，重构为 Provider 接口 |
| FastAPI WebSocket 音频传输 | ❌ 桌面版改为进程内流水线/本地 IPC（不再需要浏览器中转） |
| 前端 16k 重采样 | ❌ 由本应用采集层自行实现 |
| PyInstaller 打包脚本（`build_release.py`） | ✅ 思路复用（模型外置、CUDA DLL 复制等） |

---

## 3. 候选方案

### 方案 A：Tauri 2 + Rust 后端 + Web 前端（React/Vue + TypeScript）

```
┌─────────────────────────────────────────────┐
│ Tauri 2 壳                                   │
│  ┌──────────────┐   ┌────────────────────┐  │
│  │ 主窗口 (Web)  │   │ 悬浮窗 (Web, 透明  │  │
│  │ React + TW   │   │ 置顶/穿透/CSS动画) │  │
│  └──────┬───────┘   └─────────┬──────────┘  │
│         │  Tauri IPC / events            │
│  ┌──────┴────────────────────────────────┴┐ │
│  │ Rust 核心                               │ │
│  │ 采集: cpal(麦克风) + wasapi loopback     │ │
│  │ ASR: sherpa-rs (SenseVoice/Whisper+VAD) │ │
│  │ 翻译: reqwest → OpenAI兼容/Ollama       │ │
│  │ 模型管理/导出/设置                       │ │
│  └─────────────────────────────────────────┘ │
└─────────────────────────────────────────────┘
```

- **UI**：Web 技术栈，现代感上限最高（Tailwind + shadcn/Radix 或 Element Plus，暗色模式一行切换）；悬浮窗即第二个透明 Web 窗口，文字动画最华丽。
- **音频**：`cpal` 枚举/采集麦克风（Linux 下 pulse host 可见 monitor 设备）；Windows 环回用 `wasapi` crate（cpal 不支持 loopback）。
- **ASR**：[sherpa-rs](https://github.com/thewh1teagle/sherpa-rs)（活跃，内置多平台预编译库，支持 VAD/SenseVoice/Whisper/流式），成熟度中等，个别 API 需自行补绑 C API。
- **翻译**：reqwest 直连 OpenAI 兼容端点（Ollama 走 `/v1`）。
- **打包**：Windows 单 exe（WebView2 系统自带）；Linux AppImage。**体积最小（~10–30MB），无自解压延迟**。
- **优点**：产品形态最佳（小、快、美）；Rust 内存安全；无运行时依赖。
- **缺点**：开发成本最高（Rust + FFI + 双平台音频底层，环回采集、重采样、并发流水线全要手写）；团队若无 Rust 经验风险大；sherpa-rs 出问题需深入 C API 排查。
- **工作量估计**：UI 3~4 周 / Rust 核心流水线 3~5 周 / 打包与双平台调试 1~2 周，合计 **7~11 周**。

### 方案 B：PySide6 全 Python 单体应用（推荐）

```
┌────────────────────────────────────────────┐
│ PySide6 (Qt for Python)                     │
│  主窗口(侧导航: 音源/转写/设置) + 悬浮窗     │
│  主题: 自研QSS 或 Fluent风格库, 暗/亮切换    │
│  ┌──────────────────────────────────────┐  │
│  │ 核心流水线 (纯Python, QThread/asyncio)│  │
│  │ 采集: soundcard (麦克风+环回, 双平台)  │  │
│  │   → soxr重采样16k → Silero VAD 切句   │  │
│  │ ASR: sherpa-onnx Python (SenseVoice/  │  │
│  │   Whisper, CUDA可选降级CPU)           │  │
│  │ 翻译: httpx → OpenAI兼容/Ollama       │  │
│  │ 记录: SQLite/JSONL + 导出TXT/MD/SRT.. │  │
│  └──────────────────────────────────────┘  │
└────────────────────────────────────────────┘
```

- **UI**：Qt Widgets + QSS 定制现代风格（或采用 Fluent 风格组件库 `qfluentwidgets`——注意其为 **GPLv3/商业双授权**，闭源需自研 QSS 或选 MIT 的 QDarkStyle 等）；暗色模式内建支持。悬浮窗为 Qt 原生能力（无边框/透明/置顶/穿透均支持，Win+X11 稳定）。
- **音频**：`soundcard` 一个库同时解决双平台"麦克风 + 系统环回"的枚举与采集（Windows 走 WASAPI loopback，Linux 走 PulseAudio/PipeWire monitor）；备选 `pyaudiowpatch`(Win)+`sounddevice`(Linux)。
- **ASR**：sherpa-onnx **官方 Python 绑定**（一等公民，参考项目全部代码直接迁移）。
- **翻译**：httpx + Provider 接口，参考项目 `translate_text` 重构即得。
- **打包**：PyInstaller **onefile** 双平台单文件（Linux 建议同时在 Ubuntu 20.04 基线构建/AppImage 兜底）；`build_release.py` 的模型外置 + CUDA DLL 思路直接沿用。
- **优点**：**开发速度最快**（`backend/` 约 70% 逻辑平移）；单一语言栈好维护；ASR 绑定零风险；Qt 事件循环天然适合多流并发（采集线程→信号槽→UI）。
- **缺点**：单文件 80~150MB、onefile 自解压启动 3~8 秒（可用启动画面掩盖，或提供 onedir 版）；UI 精致度需 QSS 投入才能达到 Web 水准。
- **工作量估计**：核心流水线 1.5~2 周 / UI+悬浮窗 2~3 周 / 导出设置打磨 1 周 / 双平台打包 1~1.5 周，合计 **5.5~7.5 周**。

### 方案 C：Electron + Python FastAPI Sidecar（最大化复用现有后端）

```
┌───────────────────────────────────────────────┐
│ Electron 壳                                    │
│  主窗口 + 悬浮窗 (Web UI, 透明/穿透/置顶)      │
│        │ WebSocket (127.0.0.1)                │
│  ┌─────┴──────────────────────────────────┐   │
│  │ Sidecar: PyInstaller 打包的 FastAPI     │   │
│  │ = backend/main.py 原有 /stream 协议     │   │
│  │ + 新增: 音源枚举/采集(本机 soundcard)    │   │
│  │ + 转写记录/导出 REST 接口               │   │
│  └────────────────────────────────────────┘   │
└───────────────────────────────────────────────┘
```

- **UI**：与方案 A 同级的 Web 现代 UI；Electron 悬浮窗能力齐全。
- **后端**：`backend/` **几乎原样**变成 sidecar（FastAPI/WebSocket/翻译引擎全保留），只需新增本机音频采集路由（原来靠浏览器插件取音频）。
- **打包**：electron-builder `portable`（Windows 单 exe，内嵌 sidecar，运行时解压到临时目录）；Linux AppImage。**体积最大 300MB+、内存双运行时开销**；sidecar 随壳启停、崩溃守护、端口管理等胶水代码繁琐。
- **优点**：后端复用度最高（~90%）；前端可渐进复用任何既有 Web 代码。
- **缺点**：体积/内存最重；单文件是"自解压伪单文件"；双运行时（Node+Python）长期维护成本高；杀软误报叠加（Electron+PyInstaller 双重风险）。
- **工作量估计**：sidecar 改造 1~1.5 周 / Electron UI 3~4 周 / 打包胶水 1.5~2 周，合计 **5.5~7.5 周**。

---

## 4. 方案对比总表

| 维度 | A：Tauri+Rust | **B：PySide6（推荐）** | C：Electron+Sidecar |
|---|---|---|---|
| UI 现代感上限 | ★★★★★ | ★★★☆（QSS 投入后 ★★★★） | ★★★★★ |
| 悬浮窗能力 | ★★★★★ | ★★★★☆（Wayland 同样受限） | ★★★★★ |
| 参考代码复用 | ~30%（仅逻辑思路） | **~70%** | ~90% |
| 开发速度 | 慢（7~11 周） | **快（5.5~7.5 周）** | 中（5.5~7.5 周） |
| 单文件体积 | **10~30MB** | 80~150MB | 300MB+ |
| 启动速度 | **即时** | onefile 自解压 3~8s | 自解压 5~15s |
| 内存占用 | **最低** | 中 | 最高（双运行时） |
| 技术风险 | Rust 生态/FFI/双平台音频底层 | 低（全部成熟库） | 中（胶水复杂度、双运行时） |
| 长期维护 | 单栈但 Rust 门槛 | **单栈 Python，门槛最低** | 双栈 |
| 许可证注意 | MIT/Apache | PySide6 LGPLv3（闭源需遵守 LGPL 动态链接义务；qfluentwidgets 若采用需注意 GPLv3） | Electron MIT + Python 各依赖 |

**推荐理由（B）**：本项目核心难点在"音频流水线 + sherpa-onnx + 翻译降级"，这些全是参考项目已经趟通的 Python 代码；而 UI 现代感是 QSS 可以后天弥补的。

> **实际决策**：项目所有者于 2026-09-11 选择 **方案 A（Tauri 2 + Rust + Web 前端）**——以最高产品形态（小体积、快启动、最美 UI）为优先，接受 Rust 开发成本。方案 B/C 的对比内容保留于上文供追溯；`backend/` 参考代码按 §2.6 的映射关系作为**逻辑移植蓝本**（VAD 参数、CUDA 降级、翻译三态降级、语言标签判定等），而非代码级复用。

---

## 5. 公共详细设计（无论选择哪个方案均适用）

### 5.1 数据流水线与事件模型

```
[采集线程×N] --PCM块--> [重采样16k] --> [VAD切句] --> [ASR识别]
                                                           │ TranscriptItem{src, t0, t1, raw, lang}
[翻译队列] <──────────────────────────────────────────────┘
     │ (Provider: OpenAI兼容 / 本地LLM)
     ▼
TranscriptItem{translated} ──> 主窗口列表 / 悬浮窗 / 记录存储(SQLite+JSONL)
```

- 事件总线（Qt 信号槽 / Tauri event / Electron IPC）：`audio_level`(音量表)、`partial`(流式中间结果,预留)、`transcript`(最终句)、`error`、`engine_state`(模型加载中/就绪/降级CPU)。
- 每路音源独立 pipeline 实例；翻译器全局单例串行消费，避免 LLM 端并发限制与乱序。

### 5.2 主窗口信息架构

- **音源页**：设备列表（名称/类型[麦克风|环回]/采样率/音量电平表），多选 + 开始/停止；运行中显示各路状态。
- **转写页**：时间线卡片流（时间戳 + 来源标签 + 原文 + 译文[可隐藏]），顶部工具条（翻译开关、目标语言、暂停/继续、清空、导出、字体设置弹层[字体族/字号，实时预览，主窗与悬浮窗各自记忆]）。
- **设置页**：识别（引擎/源语言/VAD 参数）、翻译（Provider 配置、模型、prompt 模板、超时、测试按钮）、外观（暗/亮/跟随系统、强调色）、模型管理（下载/导入/删除、存储路径）、高级（CUDA 开关、便携模式、日志）。

### 5.3 悬浮窗（桌面歌词模式）

- 显示模式：`原文+译文`（仅翻译开启）/ `仅原文` / `仅译文`；
- 行为：拖动移动、双击锁定（点击穿透 + 鼠标手势禁用）、记忆位置、置顶、任务栏隐藏、可选"文字描边/阴影/背景透明度"；
- 字体族/字号/颜色独立于主窗口设置；
- 文本更新策略：最新句淡入，上一句上移渐隐（保留最近 1~2 句）。

### 5.4 数据存储与导出

- 会话记录：SQLite（结构化检索）+ 同步 JSONL（容灾）；字段：id、session、source、t0/t1、raw_text、lang、translated_text、provider、asr_engine。
- 导出：TXT（纯文本拼接）、Markdown（表格，含元信息）、SRT（按 t0/t1 生成双语或单语字幕）、CSV、JSON。SRT 时间轴 = 句子 VAD 起止时间。

### 5.5 模型管理

- 首次启动向导：选择识别引擎 → 从 GitHub Releases / HF / 镜像下载（进度、断点续传、sha256 校验）；
- 模型目录：`<用户数据>/LiveTranslator/models/`；便携模式优先读 exe 同目录；
- 模型清单：silero_vad.onnx、SenseVoice int8、Whisper small int8（按需）、流式 zipformer（预留）。

### 5.6 目录结构草案（方案 A：Tauri 2 + Rust）

```
live_translator/
├── src-tauri/                  # Rust 端
│   ├── src/
│   │   ├── main.rs  lib.rs     # Tauri 壳、双窗口（主窗 + 悬浮窗）管理
│   │   ├── audio/              # 音频采集
│   │   │   ├── mod.rs          #   统一 CaptureStream trait + 设备枚举 + host 运行时选择
│   │   │   ├── wasapi_loopback.rs  # Windows 环回（wasapi crate）
│   │   │   ├── pw_host.rs      # Linux: cpal pipewire feature（麦克风 + STREAM_CAPTURE_SINK 环回）
│   │   │   ├── pa_host.rs      # Linux 回退: cpal pulseaudio feature（经 pipewire-pulse）
│   │   │   ├── cpal_mic.rs     # 通用麦克风（cpal ALSA host 兜底）
│   │   │   └── resample.rs     # → 16k mono f32（rubato crate）
│   │   ├── asr/                # sherpa-rs 封装
│   │   │   ├── mod.rs          #   Recognizer trait + 引擎状态机(加载/就绪/降级CPU/空闲卸载)
│   │   │   ├── sense_voice.rs  #   SenseVoice + Silero VAD 切句
│   │   │   └── vad.rs          #   VAD 参数（移植 backend/main.py init_vad）
│   │   ├── translate/
│   │   │   ├── mod.rs          #   Provider trait + 串行翻译队列 + 三态降级(移植 translate_text)
│   │   │   ├── openai_compat.rs#   /v1/chat/completions（覆盖 DeepSeek/Ollama/llama.cpp）
│   │   │   └── opencc.rs       #   简繁转换（opencc-rs 或内嵌词典）
│   │   ├── store/              # SQLite(rusqlite) + JSONL + 导出 TXT/MD/SRT/CSV/JSON
│   │   ├── models/             # 模型管理：下载/断点续传/sha256/镜像/便携目录探测
│   │   └── pipeline.rs         # 采集→VAD→ASR→翻译 编排（每源一个 pipeline，crossbeam 通道）
│   └── tauri.conf.json         # 双窗口配置（悬浮窗 transparent/alwaysOnTop/skipTaskbar）
├── src/                        # Web 前端（React + TS + Tailwind）
│   ├── pages/ (Sources/Transcript/Settings)
│   ├── overlay/                # 悬浮窗入口（独立 route：三显示模式/拖动/锁定）
│   ├── stores/  components/  theme/(暗色/亮色)
└── backend/                    # 参考代码（只读，逻辑移植蓝本）
```

---

## 6. 里程碑草案（方案 A 口径）

| 里程碑 | 内容 | 交付物 |
|---|---|---|
| M1 ✅ | Rust 核心流水线：设备枚举 + 采集（Windows WASAPI 环回 / **Linux PipeWire 原生（STREAM_CAPTURE_SINK）+ PulseAudio 回退**）+ 重采样 16k + Silero VAD + SenseVoice 识别 | **已完成**：`src-tauri/src/{audio,asr,pipeline}` + CLI 冒烟 `src/bin/smoke.rs`。验证：cargo check 0 警告、单测 7/7、272s 中文演讲 WAV 离线解码输出高质量转写、"播放→default_sink 环回→实时转写"时间轴吻合。遗留：SenseVoice 语言标签未随 result.text 返回（M3 改用其他语言信号）；Windows 环回待 M1-Win |
| M2 ✅ | Tauri 双窗口骨架 + 主窗口 UI：音源页/转写页/暗色主题/字体设置；IPC 事件总线打通 | **已完成**：设置持久化（结构体与前端 TS 类型一一对应）、会话 SQLite+JSONL 双写、电平表/引擎状态/错误事件全接线、字体弹层（预览+候选列表+自定义）、暗色主题三态（跟随系统/浅/深）持久化、翻译连通性测试按钮 |
| M3 ✅ | 翻译：OpenAI 兼容 Provider（覆盖 Ollama `/v1`）+ 三态降级 + 译文显示开关 + OpenCC | **已完成**：三种 Provider（内置 candle 引擎 / OpenAI 兼容远程 / 本地 HTTP 服务）+ 串行翻译队列 + 三态降级（本地连接失败跳过会话、超时本句跳过下句重试、远程 3 连败冷却 30s）+ Google 兜底开关 + OpenCC 简繁即时路径（中文变体零 LLM 消耗）+ `transcript:update` 译文回填 + `translate:state` 状态徽章 + 设置页三选一 + `smoke translate` 测速。**性能口径**：OpenCC 0ms；远程/HTTP 网络级；内置引擎 CPU 慢（3B Q4 ≈150s/句，candle 单线程量化推理限制）——实时字幕的本地快路径请用 Ollama(local-http)，跨语言远程用 API |
| M4 | 悬浮窗：三显示模式/拖动/锁定点击穿透/字体/记忆位置 | 功能完备版 |
| M5 | 导出 + 设置持久化 + 模型管理向导（下载/续传/校验/便携模式） | 1.0 候选 |
| M6 | 双平台单文件打包（Windows 单 exe / Linux AppImage）+ GitHub Actions 双矩阵 CI | v1.0 发布 |
| M7（后续） | 流式 Zipformer 低延迟模式、Whisper 备选引擎、CUDA GPU 构建 | v1.x |

---

## 7. 风险与对策

| 风险 | 影响 | 对策 |
|---|---|---|
| Wayland（GNOME）不支持置顶/穿透 | Linux 悬浮窗失效 | 明确验收口径为 X11 完整支持；Wayland 下降级为普通置顶窗口提示用户 |
| pipewire-rs 绑定为 WIP（官方声明预期破坏性变更） | Rust 音频层编译/升级风险 | 已改由 cpal 0.18 内部承担（其 PipeWire host 基于该绑定）；Cargo.lock 锁定版本，运行时 host 自动回退 pulseaudio/ALSA |
| PyInstaller onefile 杀软误报 / 启动慢 | 分发体验 | 提供签名 + onedir 便携 zip 双形态；启动画面掩盖自解压 |
| Linux glibc 兼容性 | 单文件在旧发行版无法运行 | 在 Ubuntu 20.04 容器基线构建；AppImage 兜底 |
| Ollama 冷启动超时 | 翻译长时间无输出 | 参考项目三态降级逻辑（连接失败跳过/超时暂退重试）+ UI 显示引擎状态 |
| 多路音频线程与 GIL 竞争 | 掉帧/延迟 | 采集仅做 numpy 缓冲拷贝，重活全部在 ASR 线程；识别为 C 扩展不占 GIL |
| 模型下载源不可达 | 首启失败 | 多镜像（GitHub + HF + 自建）+ 离线导入 |

---

## 8. 待决策问题清单

| # | 问题 | 状态 |
|---|---|---|
| 1 | 技术方案：A / B / C | ✅ 已决策：**A（Tauri 2 + Rust + Web 前端）** |
| 2 | ASR 首发引擎 | ✅ 已决策：**SenseVoice + VAD**（Whisper/流式后续版本加入，引擎抽象层预留） |
| 3 | 模型分发 | ✅ 已决策：**首次启动下载**（多镜像 + 断点续传 + sha256 校验 + 便携模式） |
| 4 | Windows GPU：是否需要 CUDA 加速构建（体积 +1.3GB）？ | ⏳ 待定（1.0 建议 CPU-only，GPU 作为可选发行版） |
| 5 | 前端框架与组件库：React 还是 Vue？Tailwind + shadcn/Radix 还是 Element Plus / Naive UI？ | ✅ 已决策：React + TS + Tailwind（骨架采用，见 §9） |
| 6 | 导出格式优先级：SRT 双语字幕是否为高频需求（影响时间轴精度设计）？ | ⏳ 待定 |

---

## 9. 决策记录

| 日期 | 决策项 | 结论 | 备注 |
|---|---|---|---|
| 2026-09-11 | 技术方案 | **A：Tauri 2 + Rust 后端 + Web 前端** | 以产品形态（10~30MB 单文件、即时启动、Web 级 UI）为优先，接受 Rust 开发成本；`backend/` 作为逻辑移植蓝本 |
| 2026-09-11 | ASR 首发引擎 | **SenseVoice-Small int8 + Silero VAD** | VAD 参数与 CUDA 降级逻辑移植自 `backend/main.py`；Whisper 与流式 Zipformer 留作后续版本 |
| 2026-09-11 | 模型分发 | **首次启动下载** | 多镜像 + 断点续传 + sha256 校验；支持便携模式（exe 同目录 `models/` 优先） |
| 2026-09-11 | Linux 音频后端 | **cpal 原生 PipeWire（首选）+ PulseAudio 兼容回退** | 用户环境为 PipeWire；环回采用 STREAM_CAPTURE_SINK 机制（见 §2.1 PipeWire 支持说明） |
| 2026-09-11 | 工程骨架 | **backend/ 已删除，代码骨架已建立并通过构建验证** | 参考价值全部归档至 `docs/porting-notes.md`（提示词逐字保留 / VAD 参数 / 模型清单 / 降级策略），原 Python 代码不再保留 |
| 2026-09-11 | 前端框架 | **React + TypeScript + Tailwind CSS（按 §8 建议采用）** | 双入口（主窗 index.html / 悬浮窗 overlay.html）；M2 动工前如需更换成本可控 |
| 2026-09-11 | ASR 绑定 | **官方 sherpa-onnx Rust crate v1.13.8**（static feature） | sherpa-rs 已宣布弃用，上游 k2-fsa 提供官方 Rust API；✅ 已编译通过 |
| 2026-09-11 | M1 核心流水线 | **✅ 完成并通过端到端验证** | PW 枚举/环回/麦克风采集、48k→16k 重采样、Silero VAD、SenseVoice 全部打通；离线解码与"播放→sink 环回→实时转写"双验证通过（见 §6 M1 备注） |
| 2026-09-11 | M2 主窗口 UI | **✅ 完成** | 设置持久化（get/save_settings ↔ settings.json）、会话存储（SQLite+JSONL 双写）、音源页（电平表/运行态/引擎徽章/会话条）、转写页（字体弹层即时生效/状态栏）、设置页全量表单+翻译测试按钮、主题三态切换持久化；`pnpm tauri dev` 实测 GUI 启动（数据库/Web 数据目录确认） |
| 2026-09-11 | Cargo features | **audio-pipewire 默认启用** | tauri dev 不传 feature 标志也能用 PW 后端；无 clang 环境用 `--no-default-features` 回退 ALSA；另加 `default-run` 消除双二进制歧义 |
| 2026-09-12 | 应用流捕获（提前实现，原 V1.x 加分项） | **✅ PipeWire StreamOutput 节点可定向捕获** | 设备列表新增「应用音频」类；实现 = capture 流 + `TARGET_OBJECT` 指向应用节点（等价 `pw-record --target`）；排除 StreamInput（应用录音流，无输出端口不可捕获）；应用流动态出现/消失，线程退出自动从运行表清理（可再次启动）；实测 Firefox 日语视频流 → 实时转写成功 |
| 2026-09-12 | 同名应用流区分 | **id 与显示名带序号**（`pw:Firefox#1` / `Firefox（流#1）`） | 每个标签页是一条独立流且 node.name 相同（cpal DeviceId 仅含节点名无法区分）→ enumerate 顺序内生成 `#N` 序号，open 时按序号路由到第 N 条同名流；流增删后序号会移位，刷新设备后需重新勾选 |
| 2026-09-12 | 音源信号测试 | **✅ 列表获取时并行探测，有声源排前 + 🔊 标识** | `list_audio_devices` 枚举后每源开流测峰值 700ms（每源一线程并行，总耗时 ≈0.7s 与数量无关）；阈值 0.01，有声源按峰值降序排前并在 UI 加「有声 🔊」徽标；实测 11 源 0.73s 完成，播放中的 sink 97%/应用流 95%、底噪麦克风 2%、静音源 0% 分层清晰 |
| 2026-09-12 | OpenCC 误判修复 | **日文/韩文不再走简繁快路径** | 日文含汉字（CJK 区段）被 contains_cjk 误判为中文 → OpenCC 原样通过 → 译文=原文且远程翻译不触发；已加假名/谚文排除 + 单元回归测试（14/14）；默认目标语言改为简体中文（繁体选项下移），生效配置同步更新 |
| 2026-09-12 | M3 翻译管线 | **✅ 完成**（串行队列/三态降级/OpenCC/译文回填/状态事件/三 Provider 设置） | 快路径：中文↔简繁 = OpenCC 即时（0 LLM 消耗）；LLM/远程走异步队列 + `transcript:update` 回填；降级 = 本地连接失败跳过会话/超时本句跳过/远程 3 连败冷却 30s/Google 兜底可开关；测试 10/10 |
| 2026-09-12 | Windows CI 修复 | icon.ico 补齐（tauri-build 资源必需）/ .gitignore 误吞 src\/models / wasapi 0.24 Windows 类型适配（WaveFormat usize、probe 线程所有权） | CI 可编译性按轮次验证 |
| 2026-09-12 | M4 悬浮窗 | **✅ 完成**（v2） | 智能显示：有译文按模式（双语/仅原文/仅译文），无译文自动回落单行原文；悬浮窗本体可切换模式 + 锁定（点击穿透，主窗解锁）；拖动 + 位置记忆（Moved 事件 → 设置内存态，退出落盘）；启动恢复位置；字体独立（overlayFont）；上一句渐隐；主窗转写页工具栏「悬浮窗开关/锁定」按钮 |
| 2026-09-12 | M1-Win 系统声音环回 | **✅ wasapi 0.24 真实实现** | Windows 下音源列表新增全部「系统声音」源（默认输出 + 各渲染设备，`wasapi:{endpoint_id}`）；设备级环回 = Render 设备 + Capture 方向初始化（wasapi-rs 内部自动 LOOPBACK 标志）；WASAPI autoconvert 直接输出 16k 单声道免重采样；按应用流捕获（进程级 LOOPBACK）留作后续增强 |
| 2026-09-12 | 内置引擎选型 | **candle 纯 Rust 进程内推理 + Qwen2.5-3B Q4**（用户要求纯 Rust、不依赖 Ollama） | Qwen3.5-4B GGUF 为 `arch=qwen35`，candle 不支持（仅 qwen2），文件已移至工作区根目录留档；**性能实测（3700X CPU）**：加载 8s，但量化 vec_dot 单线程内存带宽封顶 → **~150s/句，不适合实时字幕**。本地跨语言快路径建议 = Ollama（local-http Provider，3070 上 1~3s/句）或远程 API；内置引擎定位为零依赖兜底（小段落离线翻译）。CPU 提速需多线程量化内核（candle 生态缺失）或 CUDA 构建（M7） |
| | Windows GPU 构建 | 待定 | 1.0 倾向 CPU-only，CUDA 作可选发行版 |

---

## 10. 开发环境与启动

### 10.1 构建依赖（Arch Linux 实测，其他发行版见括号内对应包）

| 依赖 | 用途 | Arch 包 |
|---|---|---|
| rustup/cargo ≥ 1.85 | Rust 工具链 | `rustup` |
| node ≥ 20 + pnpm | 前端工具链 | `nodejs` `pnpm` |
| webkit2gtk-4.1 | Tauri 运行时（Linux） | `webkit2gtk-4.1` |
| alsa-lib | cpal 默认 ALSA 后端 | `alsa-lib` |
| pipewire（含头文件） | cpal PipeWire 后端（feature） | `pipewire` |
| libpulse | cpal PulseAudio 后端（feature） | `libpulse` |
| clang | bindgen（pipewire feature 需要） | `clang` |

### 10.2 常用命令

```bash
# ===== 一键启动（推荐）=====
./run-dev.sh                          # 开发模式（热重载，改代码自动生效）
./run-release.sh --build              # 构建并启动 release 版（快、性能好）

# ===== 手动方式 =====
# 前端
pnpm install
pnpm build            # tsc + vite（生成 dist/，cargo check 需要它存在）

# Rust（默认 features 已含 PipeWire；无 clang 环境加 --no-default-features 回退 ALSA）
cd src-tauri && cargo check
cargo test            # 已含移植逻辑的单元测试（标签清洗/语言映射/重采样/提示词渲染）
cargo run --bin smoke -- list                          # CLI 冒烟：设备列表
cargo run --bin smoke -- decode models/lei-jun-test.wav models   # 离线解码
cargo run --bin smoke -- capture models "" 15          # 实时采集 15 秒

# 开发运行（双窗口热重载）
pnpm tauri dev
```

模型目录解析顺序：环境变量 `LIVE_TRANSLATOR_MODELS_DIR` → 便携模式（exe 同目录 `models/`，dev/release 均已建 symlink）→ `~/.local/share/dev.live-translator.app/models`。

**存储位置（按平台）**：
- `settings.json`：Windows = exe 同目录（便携模式，随单文件走）；Linux/macOS = `~/.config/dev.live-translator.app/`（XDG 约定）。旧位置（`~/.local/share/...`）存在设置时自动迁移。
- 会话数据（live-translator.db / transcript_*.jsonl）：`~/.local/share/dev.live-translator.app/`（Linux）/ exe 同目录（Windows）。

### 10.3 沙箱内构建注意

沙箱对工作区外的路径只读，构建缓存需重定向到工作区内（已加入 .gitignore）：

```bash
export CARGO_HOME="$PWD/.cargo-home"                       # cargo registry 缓存
pnpm install --store-dir "$PWD/.pnpm-store"                # pnpm 全局 store
```

### 10.4 排障记录

| 症状 | 原因 | 解决 |
|---|---|---|
| 窗口全黑，日志 `Failed to create GBM buffer of size ...: 无效的参数` | WebKitGTK 2.44+ 默认走 DMA-BUF/GBM 硬件渲染，NVIDIA 闭源驱动创建 GBM buffer 失败（实测环境：RTX 3070 + nvidia 驱动） | `export WEBKIT_DISABLE_DMABUF_RENDERER=1`（启动脚本已内置；回退软件渲染，文本界面无感知差异）。仍异常时追加 `WEBKIT_DISABLE_COMPOSITING_MODE=1` |
| 选具体麦克风设备（如"K66 模拟立体声"）无转写，选 default_input 正常 | **设备 id 撞车**：同一声卡的输入/输出节点在 PipeWire 中描述名相同（都叫"K66 模拟立体声"），按名匹配 id 时 `open()` 命中排前的 sink 节点 → 实际采集的是输出设备静音而非麦克风 | 设备 id 改用 PipeWire **节点名**（`cpal::DeviceId::id()`，如 `alsa_input.usb-K66...`，全局唯一）；`default_input`/`sink_default` 合成设备同理 |
| 停止转写时日志出现 `音频流错误: Device disconnected` | 流销毁时 cpal 错误回调被触发，属正常关闭时序的噪声日志 | 采集流构建时注入 stop 标志，置位后错误回调静默（真实的运行中错误仍会记录） |
| `cargo run could not determine which binary to run` | 项目含 main + smoke 双二进制 | 已在 Cargo.toml 设 `default-run = "live-translator"` |
| pnpm 报 `[ERR_SQLITE_ERROR] unable to open database file`（构建机沙箱） | 全局 store 在只读路径 | `.npmrc`/`pnpm-workspace.yaml` 已将 store 重定向至工作区 `.pnpm-store/` |

### 10.5 Windows 版构建

> ⚠️ 不要尝试在 Linux 直接交叉编译 Windows 版：Tauri Windows 端需要 MSVC 工具链，且 sherpa-onnx 需静态编译 C++，交叉环境极其脆弱。用以下两条可靠路径。

**路径 ①：GitHub Actions 云端构建（推荐，无需 Windows 机器）**

1. 把仓库推到 GitHub（仓库现为本地私有，`git remote add origin … && git push -u origin main`）
2. GitHub 仓库页 → Actions → **build** → Run workflow
3. 运行结束后在该次运行页面的 **Artifacts** 下载：
   - `live-translator-windows-x64`（含 `live-translator.exe`）
   - `live-translator-linux-x64`（含 `live-translator`，可选）
4. 使用：新建文件夹放入 exe + 从 Linux 侧拷贝 `models/` 整个目录（与 exe 同级）→ 双击运行。设置文件自动生成在 exe 同目录（Windows 便携模式）

工作流定义：`.github/workflows/build.yml`（手动触发；Windows job 用 `--no-default-features` 跳过 PipeWire，WASAPI 采集/环回不受影响）。首次 Windows 构建约 15~25 分钟（sherpa-onnx C++ 静态编译 + 依赖树）。

**路径 ②：在 Windows 机器上直接构建**

1. 安装前置：[Rust (msvc)](https://rustup.rs)、Node 20+、`npm i -g pnpm`、[VS Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/)（勾选"C++ 生成工具"工作负载）、CMake
2. 拷贝整个项目目录到 Windows 机器（或 git clone）
3. PowerShell 执行：`powershell -ExecutionPolicy Bypass -File .\build-windows.ps1`
4. 产物与使用方式同上（exe + models\ 同目录）

**Windows 版注意事项**：
- 采集：麦克风走 cpal/WASAPI；系统声音环回走 wasapi crate（WASAPI Loopback），功能与 Linux 对等
- PipeWire 相关代码在 Windows 构建中不参与（cfg 门控 + feature 关闭）
- 翻译：远程 API（tokenrhythm 等 OpenAI 兼容端点）开箱可用；内置 candle 引擎为 CPU 推理（速度说明见 §9），Windows 上同样适用
- Wayland/GBM 等排障条目仅适用 Linux
