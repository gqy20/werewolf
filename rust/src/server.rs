//! JSON-RPC 服务端
//!
//! stdin 读取请求 → 分发处理 → stdout 写回响应。
//! 协议: NDJSON（每行一个 JSON 对象）

use std::io::BufRead;

use serde_json::json;
use crate::protocol::{BridgeRequest, BridgeResponse, BridgeError};
use crate::session;
use crate::pane;

/// 从 BufReader 读取一行 JSON（NDJSON 协议）
pub fn read_request_line<R: BufRead>(reader: &mut R) -> Option<String> {
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) => None,
        Ok(_) => Some(line.trim_end().to_string()),
        Err(_) => None,
    }
}

/// 处理单条请求，返回 JSON 响应字符串
pub fn handle_request(input: &str) -> String {
    let req: Result<BridgeRequest, _> = serde_json::from_str(input);
    match req {
        Ok(request) => {
            let resp = dispatch_request(&request);
            serde_json::to_string(&resp).unwrap_or_else(|_| {
                serde_json::to_string(&error_response(0, "response serialization failed")).unwrap()
            })
        }
        Err(_) => {
            serde_json::to_string(&error_response(0, "parse error: invalid json")).unwrap()
        }
    }
}

/// 根据方法名分发到对应处理器
pub fn dispatch_request(req: &BridgeRequest) -> BridgeResponse {
    let id = req.id;
    match req.method.as_str() {
        "send_text" => handle_send_text(id, &req.params),
        "capture" => handle_capture(id, &req.params),
        "wait_for" => handle_wait_for(id, &req.params),
        "new_session" => handle_new_session(id, &req.params),
        "list_sessions" => handle_list_sessions(id, &req.params),
        "kill_session" => handle_kill_session(id, &req.params),
        "session_exists" => handle_session_exists(id, &req.params),
        other => BridgeResponse::error(BridgeError {
            id,
            code: BridgeError::METHOD_NOT_FOUND,
            message: format!("unknown method: {other}"),
        }),
    }
}

// ── 方法处理器 ───────────────────────────────────────

fn handle_send_text(id: u64, params: &serde_json::Value) -> BridgeResponse {
    match pane::validate_send_text_params(params) {
        Ok(_v) => pane::format_send_response(id),
        Err(e) => BridgeResponse::error(e.id(id)),
    }
}

fn handle_capture(id: u64, params: &serde_json::Value) -> BridgeResponse {
    match pane::validate_capture_params(params) {
        Ok(_v) => session::format_capture_response(
            id,
            "",
            session::CaptureCursor { row: 0, col: 0 },
            0,
        ),
        Err(e) => BridgeResponse::error(e.id(id)),
    }
}

fn handle_wait_for(id: u64, params: &serde_json::Value) -> BridgeResponse {
    match pane::validate_wait_for_params(params) {
        Ok(_) => pane::format_wait_response(id),
        Err(e) => BridgeResponse::error(e.id(id)),
    }
}

fn handle_new_session(id: u64, params: &serde_json::Value) -> BridgeResponse {
    match session::validate_new_session(params) {
        Ok(v) => BridgeResponse::ok(id, json!({"name": v.name})),
        Err(e) => BridgeResponse::error(e.id(id)),
    }
}

fn handle_list_sessions(id: u64, _params: &serde_json::Value) -> BridgeResponse {
    session::format_list_response(id, &[])
}

fn handle_kill_session(id: u64, params: &serde_json::Value) -> BridgeResponse {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if name.is_empty() {
        return BridgeResponse::error(BridgeError {
            id,
            code: BridgeError::INVALID_PARAMS,
            message: "kill_session requires 'name' parameter".to_string(),
        });
    }
    BridgeResponse::ok(id, json!({"killed": name}))
}

fn handle_session_exists(id: u64, params: &serde_json::Value) -> BridgeResponse {
    let name = params
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    if name.is_empty() {
        return BridgeResponse::error(BridgeError {
            id,
            code: BridgeError::INVALID_PARAMS,
            message: "session_exists requires 'name' parameter".to_string(),
        });
    }
    session::format_exists_response(id, false)
}

fn error_response(id: u64, message: &str) -> BridgeResponse {
    BridgeResponse::error(BridgeError {
        id,
        code: BridgeError::INTERNAL_ERROR,
        message: message.to_string(),
    })
}
