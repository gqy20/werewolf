//! ww-observer: 狼人杀 TUI 观战面板
//!
//! 实时流式显示 8 个玩家终端内容（基于 rmux line_stream）。
//! 架构参考 broadcast-demo / mini-zellij：每个 pane 一个独立 tokio task
//! 通过 mpsc channel 向主 TUI 循环推送行数据。
//!
//! 用法:
//!   cargo run --bin ww-observer
//!
//! 键盘:
//!   q / Esc  退出
//!   r       强制刷新（重连所有 stream）

use std::collections::VecDeque;
use std::time::Duration;

use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use rmux_sdk::{Rmux, SessionName, PaneRef, PaneOutputStart, PaneLineItem};
use tokio::sync::mpsc;

const MAX_LINES: usize = 200;

/// 每个 pane 的渲染状态
struct PaneState {
    session_name: String,
    lines: VecDeque<String>,
    is_alive: bool,
    dirty: bool,
}

struct App {
    should_quit: bool,
    panes: Vec<PaneState>,
    error_msg: Option<String>,
    /// 从各 pane task 接收行数据的 channel
    line_rx: mpsc::UnboundedReceiver<(usize, String)>,
    line_tx: mpsc::UnboundedSender<(usize, String)>,
    /// 各 pane 的 stream task 句柄（用于 cleanup / restart）
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            should_quit: false,
            panes: vec![],
            error_msg: None,
            line_rx: rx,
            line_tx: tx,
            tasks: vec![],
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    enable_raw_mode()?;
    execute!(std::io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;

    let backend = CrosstermBackend::new(std::io::stdout());
    let mut terminal = ratatui::Terminal::new(backend)?;
    let mut app = App::new();

    init_streams(&mut app).await;

    loop {
        if app.should_quit { break; }

        // ── 零开销消费所有已到达的行数据 ──
        drain_lines(&mut app);

        terminal.draw(|frame| render_app(frame, &app))?;

        for p in &mut app.panes { p.dirty = false; }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                    KeyCode::Char('r') => {
                        close_all_streams(&mut app);
                        init_streams(&mut app).await;
                    }
                    _ => {}
                }
            }
        }
    }

    drop(terminal);
    execute!(std::io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    disable_raw_mode()?;
    Ok(())
}

/// 为每个 pane 创建一个独立的 line_stream task
async fn init_streams(app: &mut App) {
    match do_init_streams(app).await {
        Ok(()) => app.error_msg = None,
        Err(e) => app.error_msg = Some(e.to_string()),
    }
}

async fn do_init_streams(app: &mut App) -> anyhow::Result<()> {
    let rmux = Rmux::builder()
        .default_timeout(Duration::from_secs(5))
        .connect_or_start()
        .await?;

    let sessions = rmux.list_sessions().await?;
    let ww: Vec<String> = sessions.iter().map(|n| n.to_string())
        .filter(|n| n.starts_with("ww-")).collect();
    let targets: Vec<&str> = if ww.is_empty() { sessions.iter().map(|n| n.as_str()).collect() }
                       else { ww.iter().map(|s| s.as_str()).collect() };

    let tx = app.line_tx.clone();
    let mut panes = Vec::with_capacity(targets.len());
    let mut tasks = Vec::with_capacity(targets.len());

    for (idx, name) in targets.iter().enumerate() {
        let sn = SessionName::new(*name).map_err(|e| anyhow::anyhow!(e))?;
        let pr = PaneRef::in_first_window(sn, 0);
        let pane = rmux.pane(pr).await?;

        // 用 Oldest 模式创建 line_stream — 自动回放历史 + 持续接收新增
        let mut stream = match pane.line_stream_starting_at(PaneOutputStart::Oldest).await {
            Ok(s) => s,
            Err(_) => continue,
        };

        let tx_clone = tx.clone();
        let name_owned = name.to_string();

        // 独立 task：阻塞式 next() 循环，通过 channel 推送数据
        let handle = tokio::spawn(async move {
            loop {
                match stream.next().await {
                    Ok(Some(PaneLineItem::Line { text })) => {
                        if tx_clone.send((idx, text)).is_err() { break; }
                    }
                    Ok(Some(PaneLineItem::Lag(_))) => {}
                    _ => break,
                }
            }
        });

        panes.push(PaneState {
            session_name: name_owned,
            lines: VecDeque::with_capacity(MAX_LINES),
            is_alive: false,  // 首次收到数据时由 drain_lines 设为 true
            dirty: false,
        });
        tasks.push(handle);
    }

    app.panes = panes;
    app.tasks = tasks;
    Ok(())
}

fn close_all_streams(app: &mut App) {
    // abort 所有 stream task
    for t in app.tasks.drain(..) { t.abort(); }
    app.panes.clear();
}

/// 非阻塞消费 channel 中所有已到达的行数据
fn drain_lines(app: &mut App) {
    while let Ok((idx, line)) = app.line_rx.try_recv() {
        if let Some(ps) = app.panes.get_mut(idx) {
            if ps.lines.len() >= MAX_LINES { ps.lines.pop_front(); }
            ps.lines.push_back(line);
            ps.dirty = true;
            ps.is_alive = true;
        }
    }
}

// ═══════════════════════ 渲染层 ═══════════════════════

fn render_app(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let title = Block::default()
        .title(Span::styled(" 🐺 狼人杀 · 实时观战 ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = title.inner(size);
    frame.render_widget(title, size);

    if let Some(ref err) = app.error_msg {
        let lines: Vec<Line> = err.lines().map(|s| Line::from(s.to_string())).collect();
        frame.render_widget(Paragraph::new(lines).style(Style::default().fg(Color::Yellow)), inner);
        return;
    }

    if app.panes.is_empty() {
        let msg = "未找到任何 session\n\n请先启动游戏创建 ww-* session\n\n按 [r] 重试  [q] 退出";
        let lines: Vec<Line> = msg.lines().map(|s| Line::from(s.to_string())).collect();
        frame.render_widget(Paragraph::new(lines).style(Style::default().fg(Color::DarkGray)), inner);
        return;
    }

    let chunks = Layout::default().direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)]).split(inner);
    let grid_area = chunks[0];
    let status_area = chunks[1];

    let count = app.panes.len().max(1);
    let cols = std::cmp::min(count, 4);
    let rows = (count + cols - 1) / cols;

    let rows_layout = Layout::default().direction(Direction::Vertical)
        .constraints(std::iter::repeat(Constraint::Ratio(1, rows as u32)).take(rows).collect::<Vec<_>>()).split(grid_area);

    for (idx, pane) in app.panes.iter().enumerate() {
        let ri = idx / cols;
        let ci = idx % cols;
        let ra = rows_layout[ri];
        let ca = Layout::default().direction(Direction::Horizontal)
            .constraints(std::iter::repeat(Constraint::Ratio(1, cols as u32)).take(cols).collect::<Vec<_>>()).split(ra);
        let cell = ca.get(ci).copied().unwrap_or(ra);
        render_player_pane(frame, cell, pane);
    }
    render_status_bar(frame, status_area, app);
}

fn render_player_pane(frame: &mut Frame, area: Rect, p: &PaneState) {
    let title = format!(" {}", p.session_name);
    let bc = if p.is_alive { Color::Green } else { Color::DarkGray };
    let bs = if p.dirty { Style::default().fg(Color::Yellow) } else { Style::default().fg(bc) };

    let block = Block::default().title(Span::styled(title, Style::default().fg(bc))).borders(Borders::ALL).border_style(bs);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let h = inner.height as usize;
    let total = p.lines.len();
    let start = if total > h { total - h } else { 0 };
    let lines: Vec<Line> = p.lines.range(start..).map(|l| Line::from(l.as_str())).collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let n = app.panes.len();
    let alive = app.panes.iter().filter(|p| p.is_alive).count();
    let dirty = app.panes.iter().filter(|p| p.dirty).count();
    let active_tasks = app.tasks.iter().filter(|t| !t.is_finished()).count();
    let s = format!(
        " {} pane | {} alive | {} dirty | {} tasks active | [r] reconnect [q] quit",
        n, alive, dirty, active_tasks
    );
    frame.render_widget(
        Paragraph::new(Line::from(s)).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
