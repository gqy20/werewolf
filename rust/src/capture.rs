//! 结构化输出提取器
//!
//! 基于 PaneSnapshot.visible_lines() 做智能文本提取，
//! 替代 Python 端的 extract_reply() heuristic 解析。

/// 可配置的回复提取器
#[derive(Debug, Clone)]
pub struct ReplyExtractor {
    /// 忽略的行前缀（如 "❯", "│", "╭" 等 tmux/CLI 提示符）
    pub ignore_prefixes: Vec<&'static str>,
    /// 忽略的关键词
    pub ignore_keywords: Vec<&'static str>,
    /// 最小有效回复长度
    pub min_reply_length: usize,
}

impl Default for ReplyExtractor {
    fn default() -> Self {
        Self {
            ignore_prefixes: vec!["❯", "│", "╭", "╰", "├", "└", "─", "═", "✽", ">", "$"],
            ignore_keywords: vec![
                "请",
                "轮到",
                "输入",
                "选项",
                "worker-",
                "Welcome",
                "What's",
                "bypass",
                "Tips for",
                "Added",
                "Status line",
                "/release-notes",
                "Cogitated for",
                "Worked for",
                "Churned for",
                "Bypassed for",
                "@",
            ],
            min_reply_length: 4,
        }
    }
}

impl ReplyExtractor {
    pub fn with_options(
        ignore_prefixes: Vec<&'static str>,
        ignore_keywords: Vec<&'static str>,
        min_reply_length: usize,
    ) -> Self {
        Self {
            ignore_prefixes,
            ignore_keywords,
            min_reply_length,
        }
    }

    /// 从可见行中提取最后一条有意义的玩家回复
    pub fn extract(&self, lines: &[&str]) -> String {
        let mut best = String::new();
        for &line in lines.iter().rev() {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if self.is_noise(trimmed) {
                continue;
            }
            if trimmed.len() < self.min_reply_length && !trimmed.starts_with('*') {
                continue;
            }
            best = trimmed.to_string();
            break;
        }
        best
    }

    fn is_noise(&self, line: &str) -> bool {
        // 检查前缀
        if self.ignore_prefixes.iter().any(|p| line.starts_with(p)) {
            return true;
        }
        // 检查关键词
        if self.ignore_keywords.iter().any(|k| line.contains(k)) {
            return true;
        }
        false
    }
}

/// 使用默认提取器的便捷函数
pub fn extract_reply(lines: &[&str]) -> String {
    ReplyExtractor::default().extract(lines)
}

/// 从输出中解析投票，返回候选者名字或 None
///
/// 先尝试数字匹配，再尝试直接名字匹配
pub fn extract_vote(output: &[&str], candidates: &[&str]) -> Option<String> {
    let text = output.join(" ");

    // 数字匹配
    if let Some(n) = extract_number_from_text(&text) {
        let idx = (n as usize).saturating_sub(1);
        if idx < candidates.len() {
            return Some(candidates[idx].to_string());
        }
    }

    // 名字匹配（不区分大小写）
    let lower = text.to_lowercase();
    for name in candidates {
        if lower.contains(&name.to_lowercase()) {
            return Some(name.to_string());
        }
    }

    None
}

/// 提取纯数字
pub fn extract_number(text: &str) -> Option<u32> {
    use std::collections::BTreeMap;
    let mut numbers: BTreeMap<usize, ()> = BTreeMap::new();
    for cap in regex::Regex::new(r"\b([1-9])\b").ok()?.captures_iter(text) {
        if let Some(m) = cap.get(1) {
            if let Ok(n) = m.as_str().parse::<u32>() {
                numbers.insert(n as usize, ());
            }
        }
    }
    numbers.into_keys().next().map(|n| n as u32)
}

fn extract_number_from_text(text: &str) -> Option<i32> {
    let re = regex::Regex::new(r"\b([1-9])\b").ok()?;
    let caps: Vec<i32> = re
        .captures_iter(text)
        .filter_map(|c| c.get(1)?.as_str().parse().ok())
        .collect();
    caps.last().copied()
}
