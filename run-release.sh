#!/usr/bin/env bash
# LiveTranslator 正式版启动（无热重载，启动快、性能好）
# 首次使用先构建：./run-release.sh --build
# 用法：./run-release.sh [--build]
set -euo pipefail
cd "$(dirname "$0")"

BIN="src-tauri/target/release/live-translator"

# CUDA 检测（与 run-dev.sh 同规则）：nvcc 可用 → 启用 GPU 推理特性
# Arch 的 cuda 包安装在 /opt/cuda 但不注入 PATH → 显式加入
if [[ -x /opt/cuda/bin/nvcc && ":$PATH:" != *":/opt/cuda/bin:"* ]]; then
  export PATH="/opt/cuda/bin:$PATH"
fi
CARGO_FEATURES=""
if command -v nvcc >/dev/null 2>&1; then
  export CUDA_PATH="${CUDA_PATH:-/opt/cuda}"
  export CUDA_COMPUTE_CAP="${CUDA_COMPUTE_CAP:-$(nvidia-smi --query-gpu=compute_cap --format=csv,noheader 2>/dev/null | head -1 | tr -d '.')}"
  export CUDA_COMPUTE_CAP="${CUDA_COMPUTE_CAP:-86}"
  CARGO_FEATURES="--features cuda"
  echo "✅ CUDA 就绪（compute_cap=$CUDA_COMPUTE_CAP）"
else
  echo "ℹ️ 未检测到 nvcc → 翻译引擎使用 CPU"
fi

if [[ "${1:-}" == "--build" || ! -x "$BIN" ]]; then
  echo "🔨 构建 release 版本（首次约需几分钟；CUDA 构建更久）..."
  export LIVE_TRANSLATOR_MODELS_DIR="$(pwd)/src-tauri/models"
  pnpm tauri build -- $CARGO_FEATURES
fi

if [[ ! -x "$BIN" ]]; then
  echo "❌ 构建后仍未找到二进制: $BIN"
  exit 1
fi

# 便携模式：release 二进制同目录的 models symlink（已建好）自动生效，
# 但显式指定更稳（防止 cargo clean 清掉 symlink）
export LIVE_TRANSLATOR_MODELS_DIR="$(pwd)/src-tauri/models"

# WebKitGTK + NVIDIA 闭源驱动：DMA-BUF/GBM 硬件渲染路径创建 buffer 失败（窗口全黑），
# 禁用后回退软件渲染（文本界面无性能感知差异）
export WEBKIT_DISABLE_DMABUF_RENDERER=1
# export WEBKIT_DISABLE_COMPOSITING_MODE=1   # 仍异常时再启用

echo "🚀 启动 LiveTranslator（release，CUDA=$([[ -n "$CARGO_FEATURES" ]] && echo on || echo off))..."
exec "$BIN"
