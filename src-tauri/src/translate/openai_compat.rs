//! OpenAI Compatible 翻译客户端（/chat/completions）。
//!
//! 覆盖：DeepSeek / OpenAI / 各类网关，以及本地大模型的 OpenAI 兼容端点
//! （Ollama http://127.0.0.1:11434/v1、llama.cpp server、LM Studio）。
//!
//! 错误分类（HttpError）供三态降级判定：
//! - Connect：服务不可达（本地 → 本会话跳过）
//! - Timeout：响应超时（本地冷启动 → 本句跳过下句重试）
//! - Status：HTTP 状态码错误（4xx 配置问题等）

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("连接失败: {0}")]
    Connect(String),
    #[error("响应超时: {0}")]
    Timeout(String),
    #[error("HTTP {0}: {1}")]
    Status(u16, String),
    #[error("{0}")]
    Other(String),
}

/// 构建客户端（超时按用途区分：本地冷启动 30s，远程 15s）
fn client(timeout_secs: u64) -> Result<reqwest::Client, reqwest::Error> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(timeout_secs))
        .build()
}

/// 调用 {base_url}/chat/completions，返回译文（已压平换行——移植参考实现后处理）
#[allow(clippy::too_many_arguments)]
pub async fn chat(
    base_url: &str,
    api_key: &str,
    model: &str,
    temperature: f32,
    max_tokens: u32,
    timeout_secs: u64,
    system: &str,
    user: &str,
    diag_tag: &str,
) -> Result<String, HttpError> {
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));

    // 推理模型（GLM/deepseek-r1 等）先输出 reasoning_content 思考链：
    // 预算不足时 content 为空且 finish_reason=length → 自动加大预算重试一次
    let mut budget = max_tokens;
    for attempt in 0..2 {
        let body = serde_json::json!({
            "model": model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": user }
            ],
            "temperature": temperature,
            "max_tokens": budget,
            "stream": false,
        });

        #[cfg(debug_assertions)]
        crate::translate::diag::log(
            diag_tag,
            "request",
            &format!("POST {url} budget={budget} attempt={attempt} body={body}"),
        );

        let mut req = client(timeout_secs)
            .map_err(|e| HttpError::Other(e.to_string()))?
            .post(&url)
            .json(&body);
        if !api_key.is_empty() {
            req = req.bearer_auth(api_key);
        }

        let resp = req.send().await.map_err(classify)?;
        let status = resp.status();
        if !status.is_success() {
            let body_text = resp.text().await.unwrap_or_default();
            return Err(HttpError::Status(
                status.as_u16(),
                format!("{status}: {}", truncate(&body_text, 200)),
            ));
        }

        let value: serde_json::Value = resp.json().await.map_err(classify)?;
        let finish = value["choices"][0]["finish_reason"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        #[cfg(debug_assertions)]
        crate::translate::diag::log(
            diag_tag,
            "response",
            &format!("finish={finish} {}", truncate(&value.to_string(), 600)),
        );

        let msg = &value["choices"][0]["message"];
        let text = msg["content"].as_str().unwrap_or_default().trim().to_string();
        if !text.is_empty() {
            // 压平换行与多余空白（docs/porting-notes.md §2.2）
            return Ok(text.split_whitespace().collect::<Vec<_>>().join(" "));
        }

        // content 为空：思考链耗尽预算（截断的思考碎片不可用作译文，丢弃）
        if attempt == 0 && finish == "length" {
            budget *= 4; // 例如 512 → 2048 再试一次
            continue;
        }
        return Err(HttpError::Other(format!(
            "推理模型思考预算不足（finish_reason={finish}），译文为空"
        )));
    }
    unreachable!("重试循环必然在两轮内返回")
}

impl From<HttpError> for crate::error::AppError {
    fn from(e: HttpError) -> Self {
        crate::error::AppError::Message(e.to_string())
    }
}

fn classify(e: reqwest::Error) -> HttpError {
    if e.is_connect() {
        HttpError::Connect(e.to_string())
    } else if e.is_timeout() {
        HttpError::Timeout(e.to_string())
    } else if e.is_status() {
        HttpError::Status(e.status().map(|s| s.as_u16()).unwrap_or(0), e.to_string())
    } else {
        HttpError::Other(e.to_string())
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let cut: String = s.chars().take(n).collect();
        format!("{cut}…")
    }
}

