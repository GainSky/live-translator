//! 翻译提示词 —— 逐字移植自参考实现（docs/porting-notes.md §2），占位符已模板化。

pub const SYSTEM_PROMPT_OPENAI: &str = "你是一个专业的影片字幕即时翻译官。请将输入的影片语音字幕，翻译成简短流畅的{target}。请只输出翻译后的文字，不要包含任何解释、引言或额外标记，保持字数与原句差不多。";

pub const SYSTEM_PROMPT_LOCAL: &str = "你是一个专业的影片字幕即时翻译官。请将用户输入的影片语音字幕，翻译成简短流畅的{target}。{lang_rule}只输出翻译后的译文本身，并输出为「单独一行纯文字」；严禁输出原文、注音、拼音、解释、引言、括号标注、清单符号、换行或任何额外标记。";

pub const LANG_RULE_ZH_TW: &str = "输出必须是「繁体中文（台湾用语）」，绝对禁止输出任何简体字。";
pub const LANG_RULE_GENERIC: &str = "输出必须是{target}。";

/// 追加到所有系统提示末尾：抑制推理模型的思考/客套输出泄漏
pub const NO_THINKING: &str = "不要输出任何思考过程、推理内容、前言或解释，第一个字就必须是译文。";

/// 语言代码 → 展示名（移植自参考实现 LANG_MAP，docs/porting-notes.md §2.3）
pub fn lang_display_name(code: &str) -> &'static str {
    match code {
        "zh-TW" => "繁體中文 (Traditional Chinese)",
        "zh-CN" => "简体中文 (Simplified Chinese)",
        "en" => "英文 (English)",
        "ja" => "日文 (Japanese)",
        "ko" => "韓文 (Korean)",
        "es" => "西班牙文 (Spanish)",
        "fr" => "法文 (French)",
        "de" => "德文 (German)",
        "ru" => "俄文 (Russian)",
        _ => "简体中文",
    }
}

fn fill(template: &str, target_name: &str, lang_rule: Option<&str>) -> String {
    let mut s = template.replace("{target}", target_name);
    if let Some(rule) = lang_rule {
        s = s.replace("{lang_rule}", rule);
    }
    s
}

pub fn render_openai_system(target_lang: &str) -> String {
    let mut s = fill(SYSTEM_PROMPT_OPENAI, lang_display_name(target_lang), None);
    s.push_str(NO_THINKING);
    s
}

pub fn render_local_system(target_lang: &str) -> String {
    let name = lang_display_name(target_lang);
    let rule = if target_lang == "zh-TW" {
        LANG_RULE_ZH_TW
    } else {
        LANG_RULE_GENERIC
    };
    let mut s = fill(SYSTEM_PROMPT_LOCAL, name, Some(rule));
    s.push_str(NO_THINKING);
    s
}

/// 内置引擎专用：短提示（减少 prefill 加快响应；本地小模型指令服从性好）
pub fn render_builtin_system(target_lang: &str) -> String {
    format!(
        "你是专业字幕翻译引擎。把用户输入翻译成{}，只输出译文本身，不要解释、不要原文。{}",
        lang_display_name(target_lang),
        NO_THINKING
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zh_cn_name_is_simplified() {
        // 回归：zh-CN 的语言名曾是繁体字形（簡體中文），导致提示词混入繁体
        let name = lang_display_name("zh-CN");
        assert_eq!(name, "简体中文 (Simplified Chinese)");
        assert!(!name.contains('簡') && !name.contains('體'));
    }

    #[test]
    fn render_prompts() {
        let openai = render_openai_system("en");
        assert!(openai.contains("英文 (English)"));
        let local = render_local_system("zh-TW");
        assert!(local.contains(LANG_RULE_ZH_TW));
        assert!(local.contains("繁體中文"));
    }
}
