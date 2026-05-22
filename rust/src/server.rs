//! JSON-RPC 服务端
//!
//! stdin 读取请求 → 分发处理 → stdout 写回响应。
//! 协议: NDJSON（每行一个 JSON 对象）
//!
//! Handler 通过 BridgeState.block_on() 调用异步 rmux-sdk。

use std::io::BufRead;

use serde_json::json;
use crate::protocol::{BridgeRequest, BridgeResponse, BridgeError};
use crate::session;
use crate::pane;
use crate::bridge_state;

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

// ── 辅助：统一 SDK 错误映射 ───────────────────────────

fn sdk_err(id: u64, e: impl std::fmt::Display) -> BridgeResponse {
    BridgeResponse::error(BridgeError {
        id,
        code: -32002,
        message: e.to_string(),
    })
}

fn transport_err(id: u64, e: impl std::fmt::Display) -> BridgeResponse {
    BridgeResponse::error(BridgeError {
        id,
        code: BridgeError::INTERNAL_ERROR,
        message: e.to_string(),
    })
}

// ── 方法处理器（接入真实 rmux-sdk）──────────────────

fn handle_send_text(id: u64, params: &serde_json::Value) -> BridgeResponse {
    let v = match pane::validate_send_text_params(params) {
        Ok(v) => v,
        Err(e) => return BridgeResponse::error(e.id(id)),
    };
    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    let session_name = v.session.clone();
    let text = v.text.clone();
    match state.block_on(async move {
        let name = rmux_sdk::SessionName::new(&session_name)
            .map_err(|e| e.to_string())?;
        let pane_ref = rmux_sdk::PaneRef::in_first_window(name, 0);
        let pane = rmux.pane(pane_ref).await
            .map_err(|e| e.to_string())?;
        pane.send_text(&text).await
            .map_err(|e| e.to_string())
    }) {
        Ok(()) => pane::format_send_response(id),
        Err(e) => sdk_err(id, e),
    }
}

fn handle_capture(id: u64, params: &serde_json::Value) -> BridgeResponse {
    let v = match pane::validate_capture_params(params) {
        Ok(v) => v,
        Err(e) => return BridgeResponse::error(e.id(id)),
    };
    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    let session_name = v.session.clone();
    let lines_limit = v.lines;
    match state.block_on(async move {
        let name = rmux_sdk::SessionName::new(&session_name)
            .map_err(|e| e.to_string())?;
        let pane_ref = rmux_sdk::PaneRef::in_first_window(name, 0);
        let pane = rmux.pane(pane_ref).await
            .map_err(|e| e.to_string())?;
        let snapshot = pane.snapshot().await
            .map_err(|e| e.to_string())?;
        let all_lines = snapshot.visible_lines();
        let text = if let Some(n) = lines_limit {
            let n = n as usize;
            if all_lines.len() > n { all_lines[all_lines.len() - n..].join("\n") }
            else { all_lines.join("\n") }
        } else {
            all_lines.join("\n")
        };
        Ok::<_, String>((text, session::CaptureCursor {
            row: snapshot.cursor.row,
            col: snapshot.cursor.col,
        }, snapshot.revision))
    }) {
        Ok((text, cursor, rev)) => session::format_capture_response(id, &text, cursor, rev),
        Err(e) => sdk_err(id, e),
    }
}

fn handle_wait_for(id: u64, params: &serde_json::Value) -> BridgeResponse {
    let v = match pane::validate_wait_for_params(params) {
        Ok(v) => v,
        Err(e) => return BridgeResponse::error(e.id(id)),
    };
    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    let session_name = v.session.clone();
    let text = v.text.clone();
    match state.block_on(async move {
        let name = rmux_sdk::SessionName::new(&session_name)
            .map_err(|e| e.to_string())?;
        let pane_ref = rmux_sdk::PaneRef::in_first_window(name, 0);
        let pane = rmux.pane(pane_ref).await
            .map_err(|e| e.to_string())?;
        pane.wait_for_text(&text).await
            .map_err(|e| e.to_string())
    }) {
        Ok(()) => pane::format_wait_response(id),
        Err(e) => sdk_err(id, e),
    }
}

fn handle_new_session(id: u64, params: &serde_json::Value) -> BridgeResponse {
    let v = match session::validate_new_session(params) {
        Ok(v) => v,
        Err(e) => return BridgeResponse::error(e.id(id)),
    };
    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    let name = v.name.clone();
    // new_session 总是返回 ok（name 已校验），即使 SDK 出错也返回错误响应
    match state.block_on(async move {
        let session_name = rmux_sdk::SessionName::new(&name)
            .map_err(|e| e.to_string())?;
        let ensure = rmux_sdk::EnsureSession::named(session_name)
            .policy(rmux_sdk::EnsureSessionPolicy::CreateOrReuse)
            .detached(true)
            .size(rmux_sdk::TerminalSizeSpec::new(120, 32));
        rmux.ensure_session(ensure).await
            .map_err(|e| e.to_string())
    }) {
        Ok(_) => BridgeResponse::ok(id, json!({"name": v.name})),
        Err(e) => sdk_err(id, e),
    }
}

fn handle_list_sessions(id: u64, _params: &serde_json::Value) -> BridgeResponse {
    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    match state.block_on(async { rmux.list_sessions().await.map_err(|e| e.to_string()) }) {
        Ok(names) => {
            let infos: Vec<session::SessionInfo> = names
                .into_iter()
                .map(|n| session::SessionInfo { name: n.to_string() })
                .collect();
            session::format_list_response(id, &infos)
        }
        Err(e) => sdk_err(id, e),
    }
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

    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    let name_owned = name.to_string();
    match state.block_on(async move {
        let session_name = rmux_sdk::SessionName::new(&name_owned)
            .map_err(|e| e.to_string())?;
        let session = rmux.session(session_name).await
            .map_err(|e| e.to_string())?;
        session.kill().await.map_err(|e| e.to_string())
    }) {
        Ok(existed) => BridgeResponse::ok(id, json!({"killed": name, "existed": existed})),
        Err(e) => sdk_err(id, e),
    }
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

    let state = bridge_state::state();
    let rmux = match state.get_rmux() {
        Ok(r) => r,
        Err(e) => return transport_err(id, e),
    };
    let name_owned = name.to_string();
    match state.block_on(async move {
        let session_name = rmux_sdk::SessionName::new(&name_owned)
            .map_err(|e| e.to_string())?;
        rmux.has_session(session_name).await.map_err(|e| e.to_string())
    }) {
        Ok(exists) => session::format_exists_response(id, exists),
        Err(e) => sdk_err(id, e),
    }
}

fn error_response(id: u64, message: &str) -> BridgeResponse {
    BridgeResponse::error(BridgeError {
        id,
        code: BridgeError::INTERNAL_ERROR,
        message: message.to_string(),
    })
}
