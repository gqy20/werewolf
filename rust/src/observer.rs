//! ww-observer: 狼人杀 TUI 观战面板
//!
//! 实时显示 8 个玩家终端的网格视图。
//!
//! 用法:
//!   cargo run --bin ww-observer
//!
//! 键盘:
//!   q / Esc  退出
//!   r       强制刷新

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
use rmux_sdk::Rmux;

/// 观察器应用状态
struct App {
    should_quit: bool,
    last_refresh: std::time::Instant,
    refresh_interval_ms: u64,
    player_panes: Vec<PlayerPane>,
    error_msg: Option<String>,
}

struct PlayerPane {
    session_name: String,
    lines: Vec<String>,
    is_alive: bool,
}

impl App {
    fn new() -> Self {
        Self {
            should_quit: false,
            last_refresh: std::time::Instant::now()
                - Duration::from_secs(999),
            refresh_interval_ms: 100,
            player_panes: vec![],
            error_msg: None,
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

    refresh_data(&mut app).await;

    loop {
        if app.should_quit {
            break;
        }

        if app.last_refresh.elapsed().as_millis() >= app.refresh_interval_ms as u128 {
            refresh_data(&mut app).await;
            app.last_refresh = std::time::Instant::now();
        }

        terminal.draw(|frame| render_app(frame, &app))?;

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                    KeyCode::Char('r') => {
                        app.last_refresh =
                            std::time::Instant::now() - Duration::from_secs(999);
                    }
                    _ => {}
                }
            }
        }
    }

    drop(terminal);
    execute!(
        std::io::stdout(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    disable_raw_mode()?;
    Ok(())
}

async fn refresh_data(app: &mut App) {
    match do_refresh(app).await {
        Ok(()) => app.error_msg = None,
        Err(e) => {
            app.error_msg = Some(e.to_string());
            app.player_panes.clear();
        }
    }
}

async fn do_refresh(app: &mut App) -> anyhow::Result<()> {
    let rmux = Rmux::builder()
        .default_timeout(std::time::Duration::from_secs(5))
        .connect_or_start()
        .await?;

    let sessions = rmux.list_sessions().await?;
    let ww_sessions: Vec<String> = sessions
        .iter()
        .map(|n| n.to_string())
        .filter(|name| name.starts_with("ww-"))
        .collect();

    let target_sessions: Vec<&str> = if ww_sessions.is_empty() {
        sessions.iter().map(|n| n.as_str()).collect()
    } else {
        ww_sessions.iter().map(|s| s.as_str()).collect()
    };

    let mut panes = Vec::new();
    for session_name in &target_sessions {
        let name = rmux_sdk::SessionName::new(*session_name)
            .map_err(|e| anyhow::anyhow!(e))?;
        let pane_ref = rmux_sdk::PaneRef::in_first_window(name, 0);
        let pane = rmux.pane(pane_ref).await?;
        let snapshot = pane.snapshot().await?;
        let lines = snapshot.visible_lines();
        let is_alive = !lines.is_empty()
            && !lines.iter().all(|l| l.trim().is_empty());

        panes.push(PlayerPane {
            session_name: session_name.to_string(),
            lines,
            is_alive,
        });
    }

    app.player_panes = panes;
    Ok(())
}

fn render_app(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let title_block = Block::default()
        .title(Span::styled(
            " 🐺 狼人杀 · 观战模式 ",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));

    let inner_area = title_block.inner(size);
    frame.render_widget(title_block, size);

    if let Some(ref err) = app.error_msg {
        let msg = format!("⚠ 无法连接 rmux daemon\n\n{err}\n\n按 [r] 重试  [q] 退出");
        let lines: Vec<Line> = msg.lines().map(|s| Line::from(s.to_string())).collect();
        let para = Paragraph::new(lines).style(Style::default().fg(Color::Yellow));
        frame.render_widget(para, inner_area);
        return;
    }

    if app.player_panes.is_empty() {
        let msg = "未找到任何 session\n\n请先启动游戏创建 ww-* session\n\n按 [r] 重试  [q] 退出";
        let lines: Vec<Line> = msg.lines().map(|s| Line::from(s.to_string())).collect();
        let para = Paragraph::new(lines).style(Style::default().fg(Color::DarkGray));
        frame.render_widget(para, inner_area);
        return;
    }

    // 主区域 + 状态栏
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(inner_area);

    let grid_area = chunks[0];
    let status_area = chunks[1];

    let count = app.player_panes.len().max(1);
    let cols = std::cmp::min(count, 4);
    let rows = (count + cols - 1) / cols;

    let cell_areas = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            std::iter::repeat(Constraint::Ratio(1, rows as u32))
                .take(rows)
                .collect::<Vec<_>>(),
        )
        .split(grid_area);

    for (idx, pane) in app.player_panes.iter().enumerate() {
        let row_idx = idx / cols;
        let col_idx = idx % cols;
        let row_area = cell_areas[row_idx];

        let col_constraints: Vec<Constraint> = std::iter::repeat(Constraint::Ratio(1, cols as u32))
            .take(cols)
            .collect();
        let col_areas = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(col_constraints)
            .split(row_area);

        let cell_area = col_areas.get(col_idx).copied().unwrap_or(row_area);

        render_player_pane(frame, cell_area, pane);
    }

    render_status_bar(frame, status_area, app);
}

fn render_player_pane(frame: &mut Frame, area: Rect, pane: &PlayerPane) {
    let title = format!(" {}", pane.session_name);
    let border_color = if pane.is_alive { Color::Green } else { Color::DarkGray };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(border_color)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color));

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let height = inner.height as usize;
    let content_height = pane.lines.len().max(1);
    let start = if content_height > height { content_height - height } else { 0 };

    let lines: Vec<Line> = pane.lines[start..]
        .iter()
        .map(|l| Line::from(l.as_str()))
        .collect();

    let paragraph = Paragraph::new(lines);
    frame.render_widget(paragraph, inner);
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let count = app.player_panes.len();
    let alive = app.player_panes.iter().filter(|p| p.is_alive).count();
    let status = format!(" {} 个 session │ {} 活跃 │ 按 [r] 刷新  [q] 退出 ", count, alive);
    let line = Line::from(status).style(Style::default().fg(Color::DarkGray));
    frame.render_widget(Paragraph::new(vec![line]), area);
}
