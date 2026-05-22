//! Session 管理 TDD 测试
use serde_json::json;
use crate::protocol::{BridgeRequest, BridgeError};
use crate::session::{
    validate_new_session, validate_pane_target, map_sdk_error,
    format_list_response, format_exists_response, format_capture_response,
    SessionInfo, CaptureCursor,
};

// ── 参数校验（纯逻辑）──────────────────────────────────

#[test]
fn test_new_session_requires_name() {
    let req = BridgeRequest {
        id: 1,
        method: "new_session".into(),
        params: json!({}),
    };
    let result = validate_new_session(&req.params);
    assert!(result.is_err(), "空参数应被拒绝");
}

#[test]
fn test_new_session_accepts_valid_params() {
    let params = json!({"name": "game-1", "cwd": "/tmp/arena"});
    let result = validate_new_session(&params).unwrap();
    assert_eq!(result.name, "game-1");
    assert_eq!(result.cwd, Some(std::path::PathBuf::from("/tmp/arena")));
}

#[test]
fn test_new_session_name_empty_rejected() {
    let params = json!({"name": "", "cwd": "/tmp"});
    assert!(validate_new_session(&params).is_err());
}

#[test]
fn test_send_text_requires_session() {
    let params = json!({"text": "hello"});
    assert!(validate_pane_target(&params).is_err());
}

#[test]
fn test_send_text_accepts_valid_target() {
    let params = json!({"session": "ww-1", "text": "hello"});
    let result = validate_pane_target(&params).unwrap();
    assert_eq!(result.session, "ww-1");
}

// ── 错误映射 ─────────────────────────────────────────────

#[test]
fn test_map_not_found_error() {
    let err = map_sdk_error(
        &rmux_sdk::RmuxError::pane_not_found(
            rmux_proto::SessionName::new("ghost").unwrap(),
            rmux_sdk::PaneId::new(99),
        ),
        42,
    );
    assert_eq!(err.code, BridgeError::INVALID_PARAMS);
    assert!(err.message.contains("not found") || err.message.contains("ghost"));
    assert_eq!(err.id, 42);
}

#[test]
fn test_map_timeout_error() {
    let err = map_sdk_error(
        &rmux_sdk::RmuxError::transport(
            "wait_for",
            std::io::Error::new(std::io::ErrorKind::TimedOut, "timed out"),
        ),
        5,
    );
    assert!(err.message.contains("timed out"));
    assert_eq!(err.id, 5);
}

#[test]
fn test_map_generic_error() {
    let err = map_sdk_error(
        &rmux_sdk::RmuxError::protocol(
            rmux_proto::RmuxError::Server("daemon error".into()),
        ),
        10,
    );
    assert_eq!(err.code, BridgeError::INTERNAL_ERROR);
}

// ── 响应格式化 ─────────────────────────────────────────

#[test]
fn test_format_session_list_response() {
    let sessions = vec![
        SessionInfo { name: "ww-alice".into() },
        SessionInfo { name: "ww-bob".into() },
    ];
    let resp = format_list_response(1, &sessions);
    assert_eq!(resp.id, 1);
    let result = resp.result.unwrap();
    let arr = result.as_array().unwrap();
    assert_eq!(arr.len(), 2);
}

#[test]
fn test_format_exists_true_response() {
    let resp = format_exists_response(3, true);
    assert_eq!(resp.id, 3);
    assert_eq!(resp.result.unwrap()["exists"], json!(true));
}

#[test]
fn test_format_capture_response() {
    let resp = format_capture_response(
        7,
        "line1\nline2\nline3",
        CaptureCursor { row: 2, col: 4 },
        99,
    );
    assert_eq!(resp.id, 7);
    let r = resp.result.unwrap();
    assert_eq!(r["text"], "line1\nline2\nline3");
    assert_eq!(r["cursor"]["row"], 2);
    assert_eq!(r["revision"], 99);
}
