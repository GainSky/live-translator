# LiveTranslator Windows 构建脚本（在 Windows 机器上运行）
# 前置：已安装 Rust (msvc)、Node 20+、pnpm（npm i -g pnpm）、VS Build Tools C++ 工作负载、CMake
# 用法：powershell -ExecutionPolicy Bypass -File .\build-windows.ps1
$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot

Write-Host "==> 安装前端依赖" -ForegroundColor Cyan
pnpm install --frozen-lockfile

Write-Host "==> 构建 Windows x64（无安装器，产出独立 exe）" -ForegroundColor Cyan
# --no-default-features：跳过 Linux 专用的 PipeWire feature
# nvcc 可用时启用 CUDA（内置翻译引擎 GPU 推理）
if (Get-Command nvcc -ErrorAction SilentlyContinue) {
    Write-Host "✅ 检测到 nvcc → 启用 CUDA" -ForegroundColor Green
    $env:CUDA_COMPUTE_CAP = "86"  # 目标 GPU 算力；可按显卡代际调整（如 89=40系）
    pnpm tauri build --no-bundle -- --no-default-features --features cuda
} else {
    Write-Host "ℹ️ 未检测到 nvcc → 翻译引擎使用 CPU" -ForegroundColor Yellow
    pnpm tauri build --no-bundle -- --no-default-features
}

$exe = "src-tauri\target\release\live-translator.exe"
if (Test-Path $exe) {
    Write-Host "`n✅ 构建完成: $exe" -ForegroundColor Green
    Write-Host "使用方式：" -ForegroundColor Cyan
    Write-Host "  1. 新建一个文件夹，把 live-translator.exe 放进去"
    Write-Host "  2. 把 Linux 侧 models\ 整个目录也拷贝到该文件夹下（与 exe 同级）"
    Write-Host "     必需: silero_vad.onnx + sherpa-onnx-sense-voice... 目录"
    Write-Host "     可选: qwen2.5-3b-instruct-q4_k_m.gguf + qwen2.5-3b-tokenizer.json（内置翻译引擎）"
    Write-Host "  3. 双击运行；设置文件会自动生成在 exe 同目录（便携模式）"
} else {
    Write-Host "❌ 未找到产物，请检查上方构建日志" -ForegroundColor Red
    exit 1
}
