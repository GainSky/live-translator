#!/usr/bin/env bash
# LiveTranslator 正式版启动（无热重载，启动快、性能好）
# 首次使用先构建：./run-release.sh --build
# 用法：./run-release.sh [--build]
set -euo pipefail
cd "$(dirname "$0")"

BIN="src-tauri/target/release/live-translator"

if [[ "${1:-}" == "--build" || ! -x "$BIN" ]]; then
  echo "🔨 构建 release 版本（首次约需几分钟）..."
  export LIVE_TRANSLATOR_MODELS_DIR="$(pwd)/src-tauri/models"
  pnpm tauri build
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

echo "🚀 启动 LiveTranslator（release）..."
exec "$BIN"
