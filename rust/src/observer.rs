//! ww-observer: 狼人杀 TUI 观战面板
//!
//! 实时流式显示 8 个玩家终端内容（基于 rmux output_stream）。
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
use rmux_sdk::{Rmux, SessionName, PaneRef, PaneOutputStart};

const MAX_LINES: usize = 200;

struct App {
    should_quit: bool,
    panes: Vec<PaneState>,
    error_msg: Option<String>,
}

struct PaneState {
    session_name: String,
    lines: VecDeque<String>,
    is_alive: bool,
    dirty: bool,
}

impl App {
    fn new() -> Self {
        Self { should_quit: false, panes: vec![], error_msg: None }
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

        poll_all_streams(&mut app).await;

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

    let mut panes = Vec::new();
    for name in &targets {
        let sn = SessionName::new(*name).map_err(|e| anyhow::anyhow!(e))?;
        let pr = PaneRef::in_first_window(sn, 0);
        let pane = rmux.pane(pr).await?;
        let snap = pane.snapshot().await?;
        let vis = snap.visible_lines();
        let alive = !vis.is_empty() && !vis.iter().all(|l| l.trim().is_empty());

        // 用 Oldest 模式订阅，回放已有内容 + 接收新增
        let _stream = pane.output_stream_starting_at(PaneOutputStart::Oldest).await?;

        let mut buf = VecDeque::with_capacity(MAX_LINES);
        for l in &vis { buf.push_back(l.clone()); }

        panes.push(PaneState { session_name: name.to_string(), lines: buf, is_alive: alive, dirty: true });
    }
    app.panes = panes;
    Ok(())
}

fn close_all_streams(app: &mut App) { app.panes.clear(); }

async fn poll_all_streams(app: &mut App) {
    if app.panes.is_empty() { return; }

    let rmux = match Rmux::builder().default_timeout(Duration::from_secs(5)).connect_or_start().await {
        Ok(r) => r,
        Err(_) => return,
    };

    for ps in &mut app.panes {
        let sn = match SessionName::new(ps.session_name.as_str()) { Ok(n) => n, Err(_) => continue };
        let pr = PaneRef::in_first_window(sn, 0);
        let pane = match rmux.pane(pr).await { Ok(p) => p, Err(_) => continue };
        let mut stream = match pane.output_stream_starting_at(PaneOutputStart::Now).await { Ok(s) => s, Err(_) => continue };

        while let Ok(chunks) = stream.poll_once().await {
            for ch in &chunks {
                use rmux_sdk::PaneOutputChunk;
                match ch {
                    PaneOutputChunk::Bytes { bytes, .. } => {
                        let text = String::from_utf8_lossy(bytes);
                        for line in text.lines() {
                            if ps.lines.len() >= MAX_LINES { ps.lines.pop_front(); }
                            ps.lines.push_back(line.to_string());
                            ps.dirty = true;
                        }
                    }
                    PaneOutputChunk::Lag(_) => { ps.dirty = true; }
                    _ => {}
                }
            }
            if chunks.is_empty() { break; }
        }
    }
}

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
    let s = format!(" {} pane | {} alive | {} dirty | stream | [r] reconnect [q] quit", n, alive, dirty);
    frame.render_widget(Paragraph::new(Line::from(s)).style(Style::default().fg(Color::DarkGray)), area);
}
