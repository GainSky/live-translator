pub mod diag;
pub mod local_engine;
pub mod openai_compat;
pub mod opencc;
pub mod prompts;

use std::path::Path;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::settings::TranslationSettings;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TranslateOutcome {
    pub text: String,
    /// builtin / openai-compatible / local-http / opencc / google / none
    pub provider: String,
}

/// 三态降级状态机（每次流水线会话持有一份；docs/porting-notes.md §2.4）
#[derive(Debug, Default)]
pub struct DegradationState {
    /// 本地引擎/服务连接失败 → 本会话跳过本地通道（参考实现经验）
    pub local_offline: bool,
    /// 远程/本地连续失败计数
    pub failures: u32,
    /// 连续 3 次失败后的冷却截止（30s 内不再尝试主通道）
    pub cooldown_until: Option<Instant>,
}

impl DegradationState {
    fn in_cooldown(&self) -> bool {
        self.cooldown_until.is_some_and(|t| Instant::now() < t)
    }

    fn record_failure(&mut self, is_local: bool, e: &AppError) {
        if is_local {
            if matches!(e, AppError::EngineUnavailable(_)) {
                // 模型缺失等"必然失败"：本会话直接跳过
                self.local_offline = true;
            }
            return;
        }
        self.failures += 1;
        if self.failures >= 3 {
            self.cooldown_until = Some(Instant::now() + Duration::from_secs(30));
            self.failures = 0;
            tracing::warn!("翻译主通道连续失败，冷却 30s");
        }
    }
}

/// 文本是否包含 CJK 汉字（识别语言标签缺失时的启发式之一）
pub fn contains_cjk(text: &str) -> bool {
    text.chars().any(|c| {
        ('\u{4E00}'..='\u{9FFF}').contains(&c)
            || ('\u{3400}'..='\u{4DBF}').contains(&c)
            || ('\u{3000}'..='\u{303F}').contains(&c) // CJK 标点
    })
}

/// 是否包含日文假名（平假名/片假名）——日文句子的可靠特征
fn has_kana(text: &str) -> bool {
    text.chars()
        .any(|c| ('\u{3040}'..='\u{30FF}').contains(&c) || c == '\u{30FC}')
}

/// 是否包含韩文谚文
fn has_hangul(text: &str) -> bool {
    text.chars()
        .any(|c| ('\u{AC00}'..='\u{D7AF}').contains(&c) || ('\u{1100}'..='\u{11FF}').contains(&c))
}

/// 简繁即时转换（零 LLM 消耗）：目标为中文变体且内容已是中文时返回 Some。
///
/// ⚠️ 日文/韩文同样含 CJK 汉字，必须排除（曾导致日文原句被误判为中文、
/// OpenCC 原样通过 → 译文=原文且远程翻译不触发）。日文句子几乎必含假名。
pub fn try_opencc_inline(
    raw: &str,
    lang: Option<&str>,
    cfg: &TranslationSettings,
) -> Option<String> {
    if !cfg.enabled {
        return None;
    }
    let zh_target = matches!(cfg.target_lang.as_str(), "zh-TW" | "zh-CN");
    if !zh_target {
        return None;
    }
    let is_zh = match lang {
        Some(l) => l == "zh",
        None => contains_cjk(raw) && !has_kana(raw) && !has_hangul(raw),
    };
    if !is_zh {
        return None;
    }
    Some(match cfg.target_lang.as_str() {
        "zh-TW" => opencc::convert_s2t(raw),
        _ => opencc::convert_t2s(raw),
    })
}

/// 清洗 LLM 译文：剔除思考内容与前导套话（部分推理模型会带入正文）
pub fn strip_thinking(text: &str) -> String {
    let mut t = text.trim().to_string();
    // <think>…</think>（r1 风格）；未闭合时仅去掉标记
    if let Some(i) = t.find("<think>") {
        match t[i..].find("</think>") {
            Some(rel) => {
                let end = i + rel + "</think>".len();
                t = format!("{}{}", &t[..i], &t[end..]).trim().to_string();
            }
            None => t = t.replace("<think>", "").trim().to_string(),
        }
    }
    // 前导套话（扁平化后常见形态）：最多剥两轮
    for _ in 0..3 {
        let trimmed = t.trim_start();
        let hit = [
            "好的，", "好的,", "当然，", "当然,", "以下是", "翻译如下：", "翻译如下:",
            "翻译：", "翻译:", "译文：", "译文:", "繁體：", "繁體:", "简体：", "简体:",
        ]
        .iter()
        .find(|p| trimmed.starts_with(**p));
        match hit {
            Some(p) => t = trimmed[p.len()..].trim_start().to_string(),
            None => break,
        }
    }
    // 整体被引号包裹
    for (l, r) in [("\u{201C}", "\u{201D}"), ("\"", "\""), ("「", "」")] {
        if t.starts_with(l) && t.ends_with(r) && t.chars().count() > 2 {
            t = t[l.len()..t.len() - r.len()].to_string();
        }
    }
    t.trim().to_string()
}

/// 翻译单句（含三态降级 + Google 兜底）。
///
/// 已移植规则（docs/porting-notes.md §1.4 / §2.4）：
/// - 未启用 / 目标语言与识别语言一致 → 原样返回（provider = none）
/// - 中文↔简繁目标 → 本地 OpenCC（provider = opencc），不耗 LLM
/// - 主通道连接失败（本地）→ 本会话跳过；超时（本地冷启动）→ 本句跳过下句重试
/// - 远程连续 3 次失败 → 冷却 30s
/// - Google 兜底仅在设置开启时启用
pub async fn translate_with_fallback(
    text: &str,
    lang: Option<&str>,
    cfg: &TranslationSettings,
    state: &mut DegradationState,
    model_root: &Path,
    hub: &local_engine::EngineHub,
) -> AppResult<TranslateOutcome> {
    if !cfg.enabled {
        return Ok(TranslateOutcome { text: text.to_string(), provider: "none".into() });
    }
    // 同语言跳过（识别语言可靠时）
    if let Some(l) = lang {
        if l == cfg.target_lang {
            return Ok(TranslateOutcome { text: text.to_string(), provider: "none".into() });
        }
    }
    // OpenCC 快路径（中文变体）
    if let Some(t) = try_opencc_inline(text, lang, cfg) {
        return Ok(TranslateOutcome { text: t, provider: "opencc".into() });
    }

    let is_local = cfg.provider == "builtin" || cfg.provider == "local-http";
    let primary = if is_local && state.local_offline {
        Err(AppError::Message("本地翻译通道本会话不可用".into()))
    } else if !is_local && state.in_cooldown() {
        Err(AppError::Message("远程翻译通道冷却中".into()))
    } else {
        translate_primary(text, cfg, state, model_root, hub).await
    };

    match primary {
        Ok(outcome) => Ok(outcome),
        Err(primary_err) => {
            // Google 兜底（免费 Web 接口，3s 超时——移植参考实现）
            if cfg.google_fallback {
                match google_translate(text, &cfg.target_lang).await {
                    Ok(t) => {
                        tracing::info!("主通道失败，Google 兜底成功");
                        return Ok(TranslateOutcome { text: t, provider: "google".into() });
                    }
                    Err(e) => tracing::warn!("Google 兜底失败: {e}"),
                }
            }
            Err(primary_err)
        }
    }
}

async fn translate_primary(
    text: &str,
    cfg: &TranslationSettings,
    state: &mut DegradationState,
    model_root: &Path,
    hub: &local_engine::EngineHub,
) -> AppResult<TranslateOutcome> {
    match cfg.provider.as_str() {
        "builtin" => {
            let target = cfg.target_lang.clone();
            let text_owned = text.to_string();
            let model_root_owned = model_root.to_path_buf();
            let model_name = cfg.builtin.model.clone();
            let tokenizer_name = cfg.builtin.tokenizer.clone();
            let hub_owned = hub.clone();
            // candle 推理是阻塞 CPU 密集型：放到阻塞线程池
            let result = tokio::task::spawn_blocking(move || {
                let engine =
                    hub_owned.get_or_load(&model_root_owned, &model_name, &tokenizer_name, |s, _| {
                        tracing::info!("内置翻译引擎: {s}");
                    })?;
                let mut engine = engine.lock().unwrap();
                engine.translate(&text_owned, &target)
            })
            .await
            .map_err(|e| AppError::Message(format!("推理任务失败: {e}")))?;
            match result {
                Ok(t) => {
                    state.failures = 0;
                    Ok(TranslateOutcome { text: strip_thinking(&t), provider: "builtin".into() })
                }
                Err(e) => {
                    state.record_failure(true, &e);
                    Err(e)
                }
            }
        }
        "local-http" => {
            let system = prompts::render_local_system(&cfg.target_lang);
            let r = openai_compat::chat(
                &cfg.local.base_url,
                "",
                &cfg.local.model,
                0.2,
                128,
                30, // 本地冷启动宽松超时
                &system,
                text,
                "local-http",
            )
            .await;
            match r {
                Ok(t) => {
                    state.failures = 0;
                    // 目标为 zh-TW 时过一道 OpenCC 消除简体残留（docs/porting-notes.md §2.2）
                    let t = if cfg.target_lang == "zh-TW" {
                        opencc::convert_s2t(&t)
                    } else {
                        strip_thinking(&t)
                    };
                    Ok(TranslateOutcome { text: strip_thinking(&t), provider: "local-http".into() })
                }
                Err(e) => {
                    if matches!(e, openai_compat::HttpError::Connect(_)) {
                        state.local_offline = true;
                        let msg = format!(
                            "本地翻译服务未启动或不可达（{}）——请确认 Ollama / llama.cpp server 正在运行",
                            cfg.local.base_url
                        );
                        tracing::warn!("本地 HTTP 翻译失败: {msg}");
                        return Err(AppError::Message(msg));
                    }
                    let app_err: AppError = e.into();
                    tracing::warn!("本地 HTTP 翻译失败: {app_err}");
                    Err(app_err)
                }
            }
        }
        "openai-compatible" => {
            let system = prompts::render_openai_system(&cfg.target_lang);
            let r = openai_compat::chat(
                &cfg.openai.base_url,
                &cfg.openai.api_key,
                &cfg.openai.model,
                cfg.openai.temperature,
                512, // 推理模型（GLM 等）思考链消耗预算，译文在其后
                30,
                &system,
                text,
                "remote",
            )
            .await;
            match r {
                Ok(t) => {
                    state.failures = 0;
                    Ok(TranslateOutcome { text: strip_thinking(&t), provider: "openai-compatible".into() })
                }
                Err(e) => {
                    let app_err: AppError = e.into();
                    tracing::warn!("远程翻译失败: {app_err}");
                    state.record_failure(false, &app_err);
                    Err(app_err)
                }
            }
        }
        other => Err(AppError::Message(format!("未知翻译 provider: {other}"))),
    }
}

/// Google 免费 Web 翻译兜底（免 Key；移植参考实现，超时 3s）
async fn google_translate(text: &str, target: &str) -> AppResult<String> {
    #[cfg(debug_assertions)]
    crate::translate::diag::log("google", "request", &format!("q={text} tl={target}"));
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .build()?;
    let resp = client
        .get("https://translate.googleapis.com/translate_a/single")
        .query(&[
            ("client", "gtx"),
            ("sl", "auto"),
            ("tl", target),
            ("dt", "t"),
            ("q", text),
        ])
        .send()
        .await?
        .error_for_status()?;
    let value: serde_json::Value = resp.json().await?;
    #[cfg(debug_assertions)]
    crate::translate::diag::log("google", "response", &format!("{value}"));
    let joined: String = value[0]
        .as_array()
        .map(|parts| {
            parts
                .iter()
                .filter_map(|p| p[0].as_str())
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();
    if joined.is_empty() {
        return Err(AppError::Message("Google 兜底返回为空".into()));
    }
    Ok(joined)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(target: &str) -> TranslationSettings {
        let mut c = TranslationSettings::default();
        c.enabled = true;
        c.target_lang = target.into();
        c
    }

    #[test]
    fn japanese_not_mistaken_for_chinese() {
        // 回归：日文含汉字但被误判为中文 → OpenCC 原样通过 → 译文=原文
        let jp = "じゃなくともあなたに出会えた痛みだけが愛だって信じられるように";
        assert!(contains_cjk(jp), "日文汉字在 CJK 区段");
        assert!(has_kana(jp), "日文句子含假名");
        assert!(try_opencc_inline(jp, None, &cfg("zh-TW")).is_none());
    }

    #[test]
    fn korean_not_mistaken_for_chinese() {
        let ko = "오늘 날씨가 정말 좋네요, 같이 공원에 갑시다";
        assert!(try_opencc_inline(ko, None, &cfg("zh-TW")).is_none());
    }

    #[test]
    fn chinese_still_converts() {
        let zh = "今天天气真不错，我们一起去公园散步吧。";
        let out = try_opencc_inline(zh, None, &cfg("zh-TW")).unwrap();
        assert_eq!(out, "今天天氣真不錯，我們一起去公園散步吧。");
        // 已是繁体的目标下简繁一致
        assert!(try_opencc_inline(zh, Some("zh"), &cfg("zh-CN")).is_some());
    }

    #[test]
    fn strip_thinking_removes_preamble_and_tags() {
        assert_eq!(strip_thinking("好的，以下是翻译：今天天氣真不錯。"), "今天天氣真不錯。");
        assert_eq!(strip_thinking("<think>推理过程…</think>今天天氣真不錯。"), "今天天氣真不錯。");
        assert_eq!(strip_thinking("  译文：今天天氣真不錯。 "), "今天天氣真不錯。");
        assert_eq!(strip_thinking("今天天氣真不錯。"), "今天天氣真不錯。");
    }

    #[test]
    fn disabled_or_non_zh_target_skips() {
        let zh = "今天天气真不错";
        assert!(try_opencc_inline(zh, None, &cfg("en")).is_none());
        let mut c = cfg("zh-TW");
        c.enabled = false;
        assert!(try_opencc_inline(zh, None, &c).is_none());
    }
}
