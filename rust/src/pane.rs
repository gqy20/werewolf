//! Pane 操作封装层
//!
//! 封装 send_text / capture / wait_for 等 pane 操作的参数校验和响应格式化。

use serde_json::json;
use crate::protocol::BridgeResponse;

// ── 校验类型 ─────────────────────────────────────────────

pub struct ValidatedSendText {
    pub session: String,
    pub text: String,
}

pub struct ValidatedCapture {
    pub session: String,
    pub lines: Option<u32>,
}

pub struct ValidatedWaitFor {
    pub session: String,
    pub text: String,
    pub timeout_sec: Option<u64>,
}

// ── 参数校验 ─────────────────────────────────────────────

pub fn validate_send_text_params(params: &serde_json::Value) -> Result<ValidatedSendText, crate::protocol::BridgeError> {
    let target = crate::session::validate_pane_target(params)?;
    let has_text = params.get("text").is_some();
    if !has_text {
        return Err(crate::protocol::BridgeError::new(
            crate::protocol::BridgeError::INVALID_PARAMS,
            "send_text requires 'text' parameter",
        ));
    }
    let text = params["text"].as_str().unwrap_or("").to_string();
    Ok(ValidatedSendText { session: target.session, text })
}

pub fn validate_capture_params(params: &serde_json::Value) -> Result<ValidatedCapture, crate::protocol::BridgeError> {
    let target = crate::session::validate_pane_target(params)?;
    let raw_lines = params.get("lines").and_then(|v| v.as_i64());
    let lines = match raw_lines {
        Some(n) if n > 0 => Some(n as u32),
        Some(0) => None,
        Some(_) => return Err(crate::protocol::BridgeError::new(
            crate::protocol::BridgeError::INVALID_PARAMS,
            "'lines' must be a non-negative integer",
        )),
        None => Some(50),  // 默认 50 行
    };
    Ok(ValidatedCapture { session: target.session, lines })
}

pub fn validate_wait_for_params(params: &serde_json::Value) -> Result<ValidatedWaitFor, crate::protocol::BridgeError> {
    let target = crate::session::validate_pane_target(params)?;
    let text = params
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();
    if text.is_empty() {
        return Err(crate::protocol::BridgeError::new(
            crate::protocol::BridgeError::INVALID_PARAMS,
            "wait_for requires non-empty 'text' parameter",
        ));
    }
    let raw_timeout = params.get("timeout_sec").and_then(|v| v.as_i64());
    let timeout_sec = match raw_timeout {
        Some(0) => None,
        Some(n) if n > 0 => Some(n as u64),
        Some(_) => return Err(crate::protocol::BridgeError::new(
            crate::protocol::BridgeError::INVALID_PARAMS,
            "'timeout_sec' must be a non-negative integer",
        )),
        None => Some(30),  // 默认 30s
    };
    Ok(ValidatedWaitFor { session: target.session, text: text.to_string(), timeout_sec })
}

// ── 响应格式化 ─────────────────────────────────────────────

pub fn format_send_response(id: u64) -> BridgeResponse {
    BridgeResponse::ok(id, json!(null))
}

pub fn format_wait_response(id: u64) -> BridgeResponse {
    BridgeResponse::ok(id, json!(null))
}
