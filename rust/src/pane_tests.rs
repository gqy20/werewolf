//! Pane 操作 TDD 测试
use serde_json::json;
use crate::session::validate_pane_target;
use crate::pane::{
    validate_send_text_params, validate_capture_params,
    validate_wait_for_params, format_send_response, format_wait_response,
};

// ── send_text 参数校验 ──────────────────────────────────

#[test]
fn test_send_text_valid() {
    let params = json!({"session": "ww-1", "text": "hello world"});
    assert!(validate_send_text_params(&params).is_ok());
}

#[test]
fn test_send_text_missing_text() {
    let params = json!({"session": "ww-1"});
    assert!(validate_send_text_params(&params).is_err());
}

#[test]
fn test_send_text_empty_text_ok() {
    // 空文本是合法的（比如只发 Enter）
    let params = json!({"session": "ww-1", "text": ""});
    assert!(validate_send_text_params(&params).is_ok());
}

// ── capture 参数校验 ────────────────────────────────────

#[test]
fn test_capture_valid_with_lines() {
    let params = json!({"session": "ww-1", "lines": 50});
    let v = validate_capture_params(&params).unwrap();
    assert_eq!(v.session, "ww-1");
    assert_eq!(v.lines, Some(50));
}

#[test]
fn test_capture_valid_without_lines_defaults_50() {
    let params = json!({"session": "ww-1"});
    let v = validate_capture_params(&params).unwrap();
    assert_eq!(v.lines, Some(50));  // 默认值
}

#[test]
fn test_capture_zero_lines_becomes_none() {
    let params = json!({"session": "ww-1", "lines": 0});
    let v = validate_capture_params(&params).unwrap();
    assert_eq!(v.lines, None);  // 0 表示全部
}

#[test]
fn test_capture_negative_lines_rejected() {
    let params = json!({"session": "ww-1", "lines": -5});
    assert!(validate_capture_params(&params).is_err());
}

// ── wait_for 参数校验 ──────────────────────────────────

#[test]
fn test_wait_for_valid_with_timeout() {
    let params = json!({"session": "ww-1", "text": "ready", "timeout_sec": 30});
    let v = validate_wait_for_params(&params).unwrap();
    assert_eq!(v.text, "ready");
    assert_eq!(v.timeout_sec, Some(30));
}

#[test]
fn test_wait_for_valid_default_timeout() {
    let params = json!({"session": "ww-1", "text": "ready"});
    let v = validate_wait_for_params(&params).unwrap();
    assert_eq!(v.timeout_sec, Some(30));  // 默认 30s
}

#[test]
fn test_wait_for_zero_timeout_means_no_timeout() {
    let params = json!({"session": "ww-1", "text": "ready", "timeout_sec": 0});
    let v = validate_wait_for_params(&params).unwrap();
    assert_eq!(v.timeout_sec, None);  // 0 = 无超时
}

#[test]
fn test_wait_for_empty_text_rejected() {
    let params = json!({"session": "ww-1", "text": ""});
    assert!(validate_wait_for_params(&params).is_err());
}

// ── 响应格式化 ─────────────────────────────────────────

#[test]
fn test_format_send_response() {
    let resp = format_send_response(1);
    assert_eq!(resp.id, 1);
    assert_eq!(resp.result.unwrap(), json!(null));  // send 无返回数据
}

#[test]
fn test_format_wait_response() {
    let resp = format_wait_response(3);
    assert_eq!(resp.id, 3);
    assert_eq!(resp.result.unwrap(), json!(null));  // wait 成功无返回数据
}
