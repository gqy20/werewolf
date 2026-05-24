//! ww-observer: 狼人杀 TUI 观战面板
//!
//! 实时显示 8 个玩家终端内容。
//!
//! 这里不直接渲染 raw/line output stream。Claude Code 这类 TUI 会频繁输出
//! 光标移动、清屏和状态栏重绘控制序列；直接追加输出会把观战面板画乱。
//! 因此每个 pane 使用 rmux render_stream 触发刷新，并渲染 daemon 已解析后的
//! PaneSnapshot.visible_lines()。
//!
//! 用法:
//!   cargo run --bin ww-observer
//!
//! 键盘:
//!   q / Esc  退出
//!   r       强制刷新（重连所有 stream）
//!   g       网格视图
//!   f/Enter 聚焦视图
//!   1-8     选择玩家
//!   ←/↑     上一个玩家
//!   →/↓     下一个玩家

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
use rmux_sdk::{PaneRef, PaneSnapshot, Rmux, SessionName};
use tokio::sync::mpsc;

/// 每个 pane 的渲染状态
struct PaneState {
    session_name: String,
    lines: Vec<String>,
    revision: u64,
    source_size: (u16, u16),
    is_alive: bool,
    dirty: bool,
    error_msg: Option<String>,
}

enum PaneUpdate {
    Snapshot(PaneSnapshot),
    Error(String),
}

struct App {
    should_quit: bool,
    panes: Vec<PaneState>,
    error_msg: Option<String>,
    focused: bool,
    selected: usize,
    /// 从各 pane task 接收 snapshot 更新的 channel
    update_rx: mpsc::UnboundedReceiver<(usize, PaneUpdate)>,
    update_tx: mpsc::UnboundedSender<(usize, PaneUpdate)>,
    /// 各 pane 的 render stream task 句柄（用于 cleanup / restart）
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl App {
    fn new() -> Self {
        let (tx, rx) = mpsc::unbounded_channel();
        Self {
            should_quit: false,
            panes: vec![],
            error_msg: None,
            focused: true,
            selected: 0,
            update_rx: rx,
            update_tx: tx,
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
        if app.should_quit {
            break;
        }

        // ── 零开销消费所有已到达的 snapshot 更新 ──
        drain_updates(&mut app);

        terminal.draw(|frame| render_app(frame, &app))?;

        for p in &mut app.panes {
            p.dirty = false;
        }

        if event::poll(Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
                    KeyCode::Char('r') => {
                        close_all_streams(&mut app);
                        init_streams(&mut app).await;
                    }
                    KeyCode::Char('g') => app.focused = false,
                    KeyCode::Char('f') | KeyCode::Enter => app.focused = true,
                    KeyCode::Char(c) if ('1'..='8').contains(&c) => {
                        let idx = c as usize - '1' as usize;
                        if idx < app.panes.len() {
                            app.selected = idx;
                            app.focused = true;
                        }
                    }
                    KeyCode::Right | KeyCode::Down => select_next(&mut app),
                    KeyCode::Left | KeyCode::Up => select_prev(&mut app),
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

/// 为每个 pane 创建一个独立的 render_stream task
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
    let ww: Vec<String> = sessions
        .iter()
        .map(|n| n.to_string())
        .filter(|n| n.starts_with("ww-"))
        .collect();
    let targets: Vec<&str> = if ww.is_empty() {
        sessions.iter().map(|n| n.as_str()).collect()
    } else {
        ww.iter().map(|s| s.as_str()).collect()
    };

    let tx = app.update_tx.clone();
    let mut panes = Vec::with_capacity(targets.len());
    let mut tasks = Vec::with_capacity(targets.len());

    for (idx, name) in targets.iter().enumerate() {
        let sn = SessionName::new(*name).map_err(|e| anyhow::anyhow!(e))?;
        let pr = PaneRef::in_first_window(sn, 0);
        let pane = rmux.pane(pr).await?;

        let tx_clone = tx.clone();
        let name_owned = name.to_string();
        let pane_for_task = pane.clone();

        // 独立 task：发送初始 snapshot，然后由 render_stream 以输出驱动刷新。
        let handle = tokio::spawn(async move {
            match pane_for_task.snapshot().await {
                Ok(snapshot) => {
                    if tx_clone
                        .send((idx, PaneUpdate::Snapshot(snapshot)))
                        .is_err()
                    {
                        return;
                    }
                }
                Err(e) => {
                    let _ = tx_clone.send((idx, PaneUpdate::Error(e.to_string())));
                }
            }

            let mut stream = match pane_for_task.render_stream().await {
                Ok(s) => s,
                Err(e) => {
                    let _ = tx_clone.send((idx, PaneUpdate::Error(e.to_string())));
                    return;
                }
            };

            loop {
                match stream.next().await {
                    Ok(Some(update)) => {
                        if tx_clone
                            .send((idx, PaneUpdate::Snapshot(update.into_snapshot())))
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(None) => break,
                    Err(e) => {
                        let _ = tx_clone.send((idx, PaneUpdate::Error(e.to_string())));
                        break;
                    }
                };
            }
        });

        panes.push(PaneState {
            session_name: name_owned,
            lines: vec![],
            revision: 0,
            source_size: (0, 0),
            is_alive: false, // 首次收到 snapshot 时由 drain_updates 设为 true
            dirty: false,
            error_msg: None,
        });
        tasks.push(handle);
    }

    app.panes = panes;
    app.tasks = tasks;
    if app.selected >= app.panes.len() {
        app.selected = 0;
    }
    Ok(())
}

fn close_all_streams(app: &mut App) {
    // abort 所有 stream task
    for t in app.tasks.drain(..) {
        t.abort();
    }
    app.panes.clear();
}

/// 非阻塞消费 channel 中所有已到达的 snapshot 更新
fn drain_updates(app: &mut App) {
    while let Ok((idx, update)) = app.update_rx.try_recv() {
        if let Some(ps) = app.panes.get_mut(idx) {
            match update {
                PaneUpdate::Snapshot(snapshot) => {
                    ps.lines = snapshot.visible_lines();
                    ps.revision = snapshot.revision;
                    ps.source_size = (snapshot.cols, snapshot.rows);
                    ps.error_msg = None;
                    ps.dirty = true;
                    ps.is_alive = snapshot.revision != 0;
                }
                PaneUpdate::Error(message) => {
                    ps.error_msg = Some(message);
                    ps.dirty = true;
                    ps.is_alive = false;
                }
            }
        }
    }
}

fn select_next(app: &mut App) {
    if app.panes.is_empty() {
        return;
    }
    app.selected = (app.selected + 1) % app.panes.len();
}

fn select_prev(app: &mut App) {
    if app.panes.is_empty() {
        return;
    }
    app.selected = if app.selected == 0 {
        app.panes.len() - 1
    } else {
        app.selected - 1
    };
}

// ═══════════════════════ 渲染层 ═══════════════════════

fn render_app(frame: &mut Frame, app: &App) {
    let size = frame.area();

    let title = Block::default()
        .title(Span::styled(
            " 🐺 狼人杀 · 实时观战 ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = title.inner(size);
    frame.render_widget(title, size);

    if let Some(ref err) = app.error_msg {
        let lines: Vec<Line> = err.lines().map(|s| Line::from(s.to_string())).collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(Color::Yellow)),
            inner,
        );
        return;
    }

    if app.panes.is_empty() {
        let msg = "未找到任何 session\n\n请先启动游戏创建 ww-* session\n\n按 [r] 重试  [q] 退出";
        let lines: Vec<Line> = msg.lines().map(|s| Line::from(s.to_string())).collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(3)])
        .split(inner);
    let grid_area = chunks[0];
    let status_area = chunks[1];

    if app.focused && !app.panes.is_empty() {
        render_focused(frame, grid_area, app);
        render_status_bar(frame, status_area, app);
        return;
    }

    let count = app.panes.len().max(1);
    let cols = if grid_area.width >= 160 {
        std::cmp::min(count, 4)
    } else if grid_area.width >= 96 {
        std::cmp::min(count, 3)
    } else if grid_area.width >= 54 {
        std::cmp::min(count, 2)
    } else {
        1
    };
    let rows = (count + cols - 1) / cols;

    let rows_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            std::iter::repeat(Constraint::Ratio(1, rows as u32))
                .take(rows)
                .collect::<Vec<_>>(),
        )
        .split(grid_area);

    for (idx, pane) in app.panes.iter().enumerate() {
        let ri = idx / cols;
        let ci = idx % cols;
        let ra = rows_layout[ri];
        let ca = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                std::iter::repeat(Constraint::Ratio(1, cols as u32))
                    .take(cols)
                    .collect::<Vec<_>>(),
            )
            .split(ra);
        let cell = ca.get(ci).copied().unwrap_or(ra);
        render_player_pane(frame, cell, pane, idx == app.selected);
    }
    render_status_bar(frame, status_area, app);
}

fn render_focused(frame: &mut Frame, area: Rect, app: &App) {
    let selected = app.selected.min(app.panes.len().saturating_sub(1));
    let chunks = if area.width >= 74 && app.panes.len() > 1 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(22), Constraint::Min(20)])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Length(0), Constraint::Min(1)])
            .split(area)
    };

    if chunks[0].width > 0 {
        render_sidebar(frame, chunks[0], app);
    }
    render_player_pane(frame, chunks[1], &app.panes[selected], true);
}

fn render_sidebar(frame: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(Span::styled(" players ", Style::default().fg(Color::Cyan)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let lines: Vec<Line> = app
        .panes
        .iter()
        .enumerate()
        .map(|(idx, pane)| {
            let marker = if idx == app.selected { ">" } else { " " };
            let live = if pane.error_msg.is_some() {
                "!"
            } else if pane.is_alive {
                "*"
            } else {
                "-"
            };
            let style = if idx == app.selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else if pane.error_msg.is_some() {
                Style::default().fg(Color::Red)
            } else if pane.is_alive {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            };
            Line::from(Span::styled(
                format!("{marker} {}. {live} {}", idx + 1, pane.session_name),
                style,
            ))
        })
        .collect();

    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_player_pane(frame: &mut Frame, area: Rect, p: &PaneState, selected: bool) {
    let title = format!(
        " {} {}x{} rev:{} ",
        p.session_name, p.source_size.0, p.source_size.1, p.revision
    );
    let bc = if p.is_alive {
        Color::Green
    } else {
        Color::DarkGray
    };
    let bs = if selected {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else if p.dirty {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(bc)
    };

    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(bc)))
        .borders(Borders::ALL)
        .border_style(bs);
    let inner = block.inner(area);
    frame.render_widget(block, area);

    if let Some(err) = &p.error_msg {
        let lines: Vec<Line> = err.lines().map(|s| Line::from(s.to_string())).collect();
        frame.render_widget(
            Paragraph::new(lines).style(Style::default().fg(Color::Red)),
            inner,
        );
        return;
    }

    let h = inner.height as usize;
    let total = p.lines.len();
    let start = if total > h { total - h } else { 0 };
    let lines: Vec<Line> = p.lines[start..]
        .iter()
        .map(|l| Line::from(l.as_str()))
        .collect();
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_status_bar(frame: &mut Frame, area: Rect, app: &App) {
    let n = app.panes.len();
    let alive = app.panes.iter().filter(|p| p.is_alive).count();
    let dirty = app.panes.iter().filter(|p| p.dirty).count();
    let active_tasks = app.tasks.iter().filter(|t| !t.is_finished()).count();
    let view = if app.focused { "focus" } else { "grid" };
    let s = format!(
        " {} | {} pane | {} alive | {} dirty | {} tasks | [1-8/←→] select [f] focus [g] grid [r] reconnect [q] quit",
        view, n, alive, dirty, active_tasks
    );
    frame.render_widget(
        Paragraph::new(Line::from(s)).style(Style::default().fg(Color::DarkGray)),
        area,
    );
}
