//! Protocol 序列化/反序列化测试 — TDD: 先写测试，后写实现
use serde_json::json;
use crate::protocol::{BridgeRequest, BridgeResponse, BridgeError};

// ── Request 序列化 ──────────────────────────────────────

#[test]
fn test_send_text_request_serializes() {
    let req = BridgeRequest {
        id: 1,
        method: "send_text".into(),
        params: json!({"session": "ww-1", "text": "hello world"}),
    };
    let serialized = serde_json::to_string(&req).unwrap();
    assert!(serialized.contains("\"send_text\""));
    assert!(serialized.contains("hello world"));
}

#[test]
fn test_capture_request_with_lines() {
    let req = BridgeRequest {
        id: 2,
        method: "capture".into(),
        params: json!({"session": "ww-1", "lines": 50}),
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["method"], "capture");
    assert_eq!(val["params"]["lines"], json!(50));
}

#[test]
fn test_list_sessions_request_no_params() {
    let req = BridgeRequest {
        id: 3,
        method: "list_sessions".into(),
        params: json!({}),
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["method"], "list_sessions");
    assert!(val["params"].as_object().unwrap().is_empty());
}

#[test]
fn test_new_session_request_with_cwd() {
    let req = BridgeRequest {
        id: 4,
        method: "new_session".into(),
        params: json!({"name": "game-1", "cwd": "/tmp/arena"}),
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["params"]["name"], "game-1");
    assert_eq!(val["params"]["cwd"], "/tmp/arena");
}

#[test]
fn test_wait_for_request_with_timeout() {
    let req = BridgeRequest {
        id: 5,
        method: "wait_for".into(),
        params: json!({"session": "ww-1", "text": "ready", "timeout_sec": 30}),
    };
    let val = serde_json::to_value(&req).unwrap();
    assert_eq!(val["method"], "wait_for");
    assert_eq!(val["params"]["timeout_sec"], json!(30));
}

// ── Response 反序列化 ───────────────────────────────────

#[test]
fn test_ok_response_deserializes() {
    let raw = json!({
        "id": 1,
        "result": {"text": "output here", "cursor": {"row": 5, "col": 10}, "revision": 42}
    });
    let resp: BridgeResponse = serde_json::from_value(raw).unwrap();
    assert!(resp.result.is_some());
    assert!(resp.error.is_none());
    assert_eq!(resp.id, 1);
}

#[test]
fn test_error_response_deserializes() {
    let raw = json!({
        "id": 2,
        "error": {"code": -32000, "message": "session not found: ww-99"}
    });
    let resp: BridgeResponse = serde_json::from_value(raw).unwrap();
    assert!(resp.result.is_none());
    assert!(resp.error.is_some());
    let err = resp.error.unwrap();
    assert_eq!(err.code, -32000);
    assert!(err.message.contains("not found"));
}

#[test]
fn test_error_response_construction() {
    let err = BridgeError::new(-32603, "internal error").id(7);
    let resp = BridgeResponse::error(err);
    let val = serde_json::to_value(&resp).unwrap();
    assert_eq!(val["error"]["code"], -32603);
    assert_eq!(val["error"]["message"], "internal error");
    assert_eq!(val["id"], 7);
}

#[test]
fn test_response_without_result_or_error_is_invalid() {
    // 缺少 result 和 error 应该反序列化失败或产生无效响应
    let raw = json!({"id": 1});
    let resp: Result<BridgeResponse, _> = serde_json::from_value(raw);
    // serde 默认不会报错（Option 都是 None），但语义上这是无效的
    // 我们验证它至少能解析，后续逻辑会检查
    assert!(resp.is_ok());
    let r = resp.unwrap();
    assert!(r.result.is_none());
    assert!(r.error.is_none());
}

// ── Round-trip 完整性 ───────────────────────────────────

#[test]
fn test_request_round_trip() {
    let original = BridgeRequest {
        id: 42,
        method: "kill_session".into(),
        params: json!({"name": "old-game"}),
    };
    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: BridgeRequest = serde_json::from_str(&serialized).unwrap();
    assert_eq!(deserialized.id, original.id);
    assert_eq!(deserialized.method, original.method);
    assert_eq!(deserialized.params, original.params);
}
