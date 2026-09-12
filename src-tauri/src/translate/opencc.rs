//! 简繁转换（OpenCC 语义：s2t / t2s）—— ferrous-opencc 纯 Rust 实现。
//!
//! 词典编译期内嵌（BuiltinConfig），无系统依赖；OnceLock 惰性初始化单例。
//! 中文简繁转换走此路径（零 LLM 消耗、亚毫秒延迟），见 docs/porting-notes.md §1.4。

use ferrous_opencc::{config::BuiltinConfig, OpenCC};
use std::sync::OnceLock;

fn s2t() -> &'static OpenCC {
    static C: OnceLock<OpenCC> = OnceLock::new();
    C.get_or_init(|| OpenCC::from_config(BuiltinConfig::S2t).expect("OpenCC S2T 初始化失败"))
}

fn t2s() -> &'static OpenCC {
    static C: OnceLock<OpenCC> = OnceLock::new();
    C.get_or_init(|| OpenCC::from_config(BuiltinConfig::T2s).expect("OpenCC T2S 初始化失败"))
}

/// 简体 → 繁體
pub fn convert_s2t(text: &str) -> String {
    s2t().convert(text)
}

/// 繁體 → 简体
pub fn convert_t2s(text: &str) -> String {
    t2s().convert(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn s2t_basic() {
        assert_eq!(convert_s2t("开放中文转换"), "開放中文轉換");
        assert!(convert_s2t("天气很好，我们出去玩").contains("天氣"));
    }

    #[test]
    fn t2s_basic() {
        assert_eq!(convert_t2s("開放中文轉換"), "开放中文转换");
    }

    #[test]
    fn passthrough_non_cjk() {
        // 非中文内容原样返回（转换是字形映射）
        assert_eq!(convert_s2t("hello world 123"), "hello world 123");
    }
}
