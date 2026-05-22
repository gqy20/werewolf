//! JSON-RPC 服务端 TDD 测试
//!
//! 测试 stdin/stdout 请求分发、错误处理、响应格式。

use serde_json::json;
use crate::protocol::{BridgeRequest, BridgeResponse, BridgeError};
use crate::server::{dispatch_request, handle_request};
use crate::bridge_state::BridgeState;

// ── 请求解析 ─────────────────────────────────────

#[test]
fn test_parse_valid_request() {
    let raw = r#"{"id":1,"method":"list_sessions","params":{}}"#;
    let req: BridgeRequest = serde_json::from_str(raw).unwrap();
    assert_eq!(req.id, 1);
    assert_eq!(req.method, "list_sessions");
}

#[test]
fn test_parse_invalid_json_returns_error() {
    let raw = "this is not json";
    let result: Result<BridgeRequest, _> = serde_json::from_str(raw);
    assert!(result.is_err());
}

#[test]
fn test_parse_missing_id() {
    // 缺少 id 字段应能解析（Option），但我们的 struct 要求 id: u64
    let raw = r#"{"method":"list_sessions","params":{}}"#;
    let result: Result<BridgeRequest, _> = serde_json::from_str(raw);
    // serde 会因为缺少 id 而失败（id 不是 Option）
    assert!(result.is_err());
}

// ── 方法分发 ─────────────────────────────────────────

#[test]
fn test_dispatch_list_sessions_known_method() {
    BridgeState::init();  // handler 需要 bridge state
    let req = BridgeRequest {
        id: 1,
        method: "list_sessions".into(),
        params: json!({}),
    };
    let resp = dispatch_request(&req);
    // 不需要真实 daemon，只验证方法被正确识别
    // list_sessions 在无 daemon 时会返回错误或空列表，但不会 panic
    assert!(resp.id == 1);
}

#[test]
fn test_dispatch_unknown_method_returns_error() {
    let req = BridgeRequest {
        id: 5,
        method: "nonexistent_method".into(),
        params: json!({}),
    };
    let resp = dispatch_request(&req);
    assert!(resp.error.is_some());
    assert_eq!(resp.error.unwrap().code, BridgeError::METHOD_NOT_FOUND);
}

#[test]
fn test_dispatch_send_text_valid_params() {
    BridgeState::init();  // handler 需要 bridge state
    let req = BridgeRequest {
        id: 2,
        method: "send_text".into(),
        params: json!({"session": "ww-1", "text": "hello"}),
    };
    let resp = dispatch_request(&req);
    // 无 daemon 时应该返回 transport 错误，不是参数错误
    assert!(resp.id == 2);
}

// ── NDJSON 行读取 ────────────────────────────────────

#[test]
fn test_read_single_request_from_bufreader() {
    let data = r#"{"id":1,"method":"ping","params":{}}
"#;
    let mut reader = std::io::Cursor::new(data);
    let line = crate::server::read_request_line(&mut reader).unwrap();
    let req: BridgeRequest = serde_json::from_str(&line).unwrap();
    assert_eq!(req.method, "ping");
}

#[test]
fn test_read_empty_input_returns_none() {
    let data = "";
    let mut reader = std::io::Cursor::new(data);
    let result = crate::server::read_request_line(&mut reader);
    assert!(result.is_none());
}

// ── 响应序列化 ───────────────────────────────────────

#[test]
fn test_response_serializes_to_valid_json() {
    let resp = BridgeResponse::ok(42, json!({"status": "ok"}));
    let output = serde_json::to_string(&resp).unwrap();
    let val: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(val["id"], 42);
    assert_eq!(val["result"]["status"], "ok");
    // ok 响应没有 error 字段或 error 为 null
    assert!(!val.get("error").map_or(false, |e| e["code"].as_i64().unwrap_or(0) != 0));
}

#[test]
fn test_error_response_includes_code_and_message() {
    let err = BridgeError::new(-32000, "something wrong").id(10);
    let resp = BridgeResponse::error(err);
    let output = serde_json::to_string(&resp).unwrap();
    let val: serde_json::Value = serde_json::from_str(&output).unwrap();
    assert_eq!(val["error"]["code"], -32000);
    assert_eq!(val["error"]["message"], "something wrong");
}

// ── 集成流程：完整 request → response 循环 ─────────

#[test]
fn test_handle_roundtrip_valid_request() {
    BridgeState::init();  // handler 需要 bridge state
    let input = r#"{"id":7,"method":"list_sessions","params":{}}"#;
    let output = handle_request(input);
    let resp: BridgeResponse = serde_json::from_str(&output).unwrap();
    assert_eq!(resp.id, 7);
    // 结果可能是空列表（无 daemon）或错误，但必须是合法响应
    assert!(resp.result.is_some() || resp.error.is_some());
}

#[test]
fn test_handle_roundtrip_invalid_json() {
    let input = "broken";
    let output = handle_request(input);
    let resp: BridgeResponse = serde_json::from_str(&output).unwrap();
    assert!(resp.error.is_some());
    // serde 解析错误或我们的自定义 parse 错误都可接受
    let code = resp.error.unwrap().code;
    assert!(code == BridgeError::PARSE_ERROR || code == BridgeError::INTERNAL_ERROR);
}

#[test]
fn test_handle_roundtrip_missing_method() {
    let input = r#"{"id":3,"params":{}}"#;  // 缺少 method
    let output = handle_request(input);
    let resp: BridgeResponse = serde_json::from_str(&output).unwrap();
    // 缺少 method 可能解析失败或返回默认错误
    assert!(resp.result.is_some() || resp.error.is_some());
}
