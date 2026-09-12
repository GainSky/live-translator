//! 翻译诊断日志：**仅在测试环境（debug 构建）生成**，release 编译为零开销。
//!
//! 用于排查翻译请求/回复内容；`./run-dev.sh` 终端即可看到（tracing info 级别）。
//! 调用点请配合 `#[cfg(debug_assertions)]` 门控，避免 release 下序列化开销。

#[cfg(debug_assertions)]
pub fn log(provider: &str, phase: &str, detail: &str) {
    tracing::info!("[翻译日志][{provider}][{phase}] {detail}");
}

#[cfg(not(debug_assertions))]
pub fn log(_provider: &str, _phase: &str, _detail: &str) {}
