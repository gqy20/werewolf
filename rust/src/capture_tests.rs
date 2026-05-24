//! 结构化输出提取 TDD 测试
//!
//! 替代 Python 的 extract_reply() heuristic，基于 PaneSnapshot 做智能文本提取。

use crate::capture::{extract_number, extract_reply, extract_vote, ReplyExtractor};

// ── extract_reply ────────────────────────────────────────

#[test]
fn test_extract_reply_basic() {
    let lines = vec![
        "❯ 请发言",
        "我觉得 p3 很可疑，一直在带节奏",
        "✽ Worked for 5s",
        "❯",
    ];
    let result = extract_reply(&lines);
    assert!(result.contains("可疑"));
}

#[test]
fn test_extract_reply_ignores_prompt_prefixes() {
    let lines = vec![
        "❯ 输入编号",
        "   1. p1",
        "   2. p2",
        "我选 2",
        "qy113@qy113",
    ];
    let result = extract_reply(&lines);
    assert!(result.contains("选"));
}

#[test]
fn test_extract_reply_skips_noise_lines() {
    let lines = vec![
        "Welcome to Claude Code",
        "What's on your mind?",
        "这是我的分析：p1 是好人",
        "Cogitated for 3s",
    ];
    let result = extract_reply(&lines);
    assert!(result.contains("好人"));
}

#[test]
fn test_extract_reply_short_lines_ignored() {
    let lines = vec!["hi", "ok", "yes", "这是一个足够长的有效回复内容"];
    let result = extract_reply(&lines);
    assert!(result.contains("足够长"));
}

#[test]
fn test_extract_reply_empty_for_all_noise() {
    let lines = vec!["❯", "│", "╭", "请输入选项", "qy113@host", "Bypassed for 2s"];
    let result = extract_reply(&lines);
    // 全是噪音行应返回空字符串
    assert!(result.is_empty() || result.len() <= 4);
}

#[test]
fn test_extract_reply_prefers_last_meaningful() {
    let lines = vec![
        "第一轮发言",
        "我投 p1",
        "第二轮发言",
        "我改投 p2 了，因为发现了新线索",
    ];
    let result = extract_reply(&lines);
    // 应该返回最后一条有意义的回复（最长的）
    assert!(result.contains("p2") || result.contains("新线索"));
}

// ── extract_vote ─────────────────────────────────────────

#[test]
fn test_extract_vote_from_number() {
    let lines = vec!["我投给 2 号", "p2 最可疑"];
    let candidates = vec!["p1", "p2", "p3"];
    assert_eq!(extract_vote(&lines, &candidates), Some("p2".into()));
}

#[test]
fn test_extract_vote_from_name() {
    let lines = vec!["我觉得 alice 是狼人", "同意投票给她"];
    let candidates = vec!["alice", "bob", "charlie"];
    assert_eq!(extract_vote(&lines, &candidates), Some("alice".into()));
}

#[test]
fn test_extract_vote_no_match_returns_none() {
    let lines = vec!["我不知道，随便吧", "大家看着办"];
    assert_eq!(extract_vote(&lines, ["a", "b"].as_slice()), None);
}

// ── extract_number ─────────────────────────────────────────

#[test]
fn test_extract_number_found() {
    assert_eq!(extract_number("我选择 3"), Some(3));
}

#[test]
fn test_extract_number_not_found() {
    assert_eq!(extract_number("没有数字"), None);
}

#[test]
fn test_extract_number_first_match() {
    assert_eq!(extract_number("1 和 2 都可以"), Some(1));
}

// ── ReplyExtractor 配置 ───────────────────────────────────

#[test]
fn test_custom_extractor_with_different_ignore_prefixes() {
    let ext = ReplyExtractor {
        ignore_prefixes: vec![">>", ">>"],
        ..ReplyExtractor::default()
    };
    let lines = vec![
        ">> system prompt",
        ">> user message",
        "这是真实回复内容在这里",
    ];
    let result = ext.extract(&lines);
    assert!(result.contains("真实回复"));
}
