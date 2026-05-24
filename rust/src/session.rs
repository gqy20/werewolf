//! Session 管理封装层
//!
//! 封装 rmux-sdk 的 session 操作，提供参数校验、错误映射和响应格式化。

use std::path::PathBuf;

use crate::protocol::{BridgeError, BridgeResponse};
use serde_json::json;

// ── 校验类型 ─────────────────────────────────────────────

pub struct ValidatedNewSession {
    pub name: String,
    pub cwd: Option<PathBuf>,
}

pub struct ValidatedPaneTarget {
    pub session: String,
}

pub struct SessionInfo {
    pub name: String,
}

pub struct CaptureCursor {
    pub row: u16,
    pub col: u16,
}

// ── 参数校验 ─────────────────────────────────────────────

pub fn validate_new_session(
    params: &serde_json::Value,
) -> Result<ValidatedNewSession, BridgeError> {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();

    if name.is_empty() {
        return Err(BridgeError::new(
            BridgeError::INVALID_PARAMS,
            "new_session requires non-empty 'name'",
        ));
    }

    let cwd = params
        .get("cwd")
        .and_then(|v| v.as_str())
        .map(PathBuf::from);

    Ok(ValidatedNewSession {
        name: name.to_string(),
        cwd,
    })
}

pub fn validate_pane_target(
    params: &serde_json::Value,
) -> Result<ValidatedPaneTarget, BridgeError> {
    let session = params
        .get("session")
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim();

    if session.is_empty() {
        return Err(BridgeError::new(
            BridgeError::INVALID_PARAMS,
            "pane operation requires 'session' parameter",
        ));
    }

    Ok(ValidatedPaneTarget {
        session: session.to_string(),
    })
}

// ── 错误映射 ─────────────────────────────────────────────

pub fn map_sdk_error(err: &rmux_sdk::RmuxError, request_id: u64) -> BridgeError {
    match err {
        rmux_sdk::RmuxError::PaneNotFound { .. } => BridgeError {
            id: request_id,
            code: BridgeError::INVALID_PARAMS,
            message: err.to_string(),
        },
        rmux_sdk::RmuxError::Transport { .. } => BridgeError {
            id: request_id,
            code: -32001,
            message: err.to_string(),
        },
        _ => BridgeError {
            id: request_id,
            code: BridgeError::INTERNAL_ERROR,
            message: err.to_string(),
        },
    }
}

// ── 响应格式化 ─────────────────────────────────────────────

pub fn format_list_response(id: u64, sessions: &[SessionInfo]) -> BridgeResponse {
    let arr: Vec<serde_json::Value> = sessions.iter().map(|s| json!({"name": s.name})).collect();
    BridgeResponse::ok(id, json!(arr))
}

pub fn format_exists_response(id: u64, exists: bool) -> BridgeResponse {
    BridgeResponse::ok(id, json!({"exists": exists}))
}

pub fn format_capture_response(
    id: u64,
    text: &str,
    cursor: CaptureCursor,
    revision: u64,
) -> BridgeResponse {
    BridgeResponse::ok(
        id,
        json!({
            "text": text,
            "cursor": {"row": cursor.row, "col": cursor.col},
            "revision": revision
        }),
    )
}
