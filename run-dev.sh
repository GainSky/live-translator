#!/usr/bin/env bash
# LiveTranslator 开发模式启动（热重载：改前端/Rust 代码自动生效）
# 用法：./run-dev.sh
set -euo pipefail
cd "$(dirname "$0")"

# 模型目录：优先用仓库内 src-tauri/models（便携模式 symlink 也行，此处显式指定最稳）
export LIVE_TRANSLATOR_MODELS_DIR="$(pwd)/src-tauri/models"

# WebKitGTK + NVIDIA 闭源驱动：DMA-BUF/GBM 硬件渲染路径创建 buffer 失败（窗口全黑），
# 禁用后回退软件渲染（文本界面无性能感知差异）
export WEBKIT_DISABLE_DMABUF_RENDERER=1
# 若仍异常可再启用下行（进一步禁用合成模式）：
# export WEBKIT_DISABLE_COMPOSITING_MODE=1

# 首次运行检查
if [[ ! -f "$LIVE_TRANSLATOR_MODELS_DIR/silero_vad.onnx" ]]; then
  echo "❌ 缺少模型文件，请先下载（见 readme §10.2）"
  exit 1
fi
command -v pnpm >/dev/null || { echo "❌ 未安装 pnpm"; exit 1; }

echo "🚀 启动 LiveTranslator（开发模式）..."
exec pnpm tauri dev
