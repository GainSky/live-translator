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

# CUDA 检测：有 nvcc 则启用内置翻译引擎的 GPU 推理（任意 CUDA 显卡，自动回退 CPU）
# bindgen_cuda 构建期需要 GPU 算力值：优先 nvidia-smi 查询，回退 86（覆盖 RTX 30/40 系）
CARGO_ARGS=""
if command -v nvcc >/dev/null 2>&1; then
  export CUDA_PATH="${CUDA_PATH:-/opt/cuda}"
  export CUDA_COMPUTE_CAP="${CUDA_COMPUTE_CAP:-$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1 | tr -d '.')}"
  export CUDA_COMPUTE_CAP="${CUDA_COMPUTE_CAP:-86}"
  echo "✅ CUDA 就绪（compute_cap=$CUDA_COMPUTE_CAP）→ 内置翻译引擎 GPU 推理"
  CARGO_ARGS="--features cuda"
else
  echo "ℹ️ 未检测到 nvcc → 翻译引擎使用 CPU（安装 cuda 包后自动启用 GPU）"
fi

echo "🚀 启动 LiveTranslator（开发模式）..."
exec pnpm tauri dev -- $CARGO_ARGS
