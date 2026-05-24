# 🐺 Werewolf — Claude Code 狼人杀 Game Master

基于 **Claude Code 多实例** 的 AI 狼人杀游戏系统。每个玩家由一个独立的 Claude Code 实例扮演，通过 rmux 会话进行消息收发与行为控制，Game Master 协调完整的游戏流程。

## 特性

- **多实例 AI 对战** — 每位玩家是一个独立的 Claude Code 实例，拥有自己的工作目录和上下文
- **完整角色系统** — 狼人、村民、预言家、女巫、猎人、守卫
- **标准游戏流程** — 白天发言 → 投票处决 → 夜晚行动（狼刀/守卫/预言家/女巫）→ 结算
- **实时 Dashboard** — 内置 HTTP 监控面板，实时查看游戏状态、日志和玩家屏幕
- **游戏日志** — 每局自动记录完整日志（JSONL）和最终报告（Markdown）
- **Rust Bridge** — 通过 `werewolf-bridge` JSON-RPC 二进制用 rmux-sdk 替代 tmux subprocess 调用，支持结构化 PaneSnapshot
- **TUI 观战面板** — `ww-observer` 终端面板，基于 rmux `render_stream` + `PaneSnapshot` 实时显示 8 个玩家终端画面
- **智能游戏平衡** — 女巫药水持久化、狼人刀人历史去重、预言家查验去重、自适应超时参数

## 快速开始

### 环境要求

- Python >= 3.11
- [uv](https://docs.astral.sh/uv/) 包管理器
- [rmux](https://github.com/helvesec/rmux) 终端复用器 (可选，用于 TUI 观战)
- [Claude Code CLI](https://claude.ai/code)
- Rust toolchain (可选，用于编译 bridge 和 observer)

### 安装

```bash
git clone <repo-url>
cd werewolf
uv sync
cargo build              # 编译 werewolf-bridge + ww-observer
```

### 运行一局游戏

```bash
# 1. 启动 8 个 Claude Code 实例作为玩家
python -m werewolf bootstrap 8

# 2. 开始游戏（自动启动 Dashboard）
python -m werewolf run

# 3. 查看实时状态
python -m werewolf status

# 4. TUI 观战面板（终端内同时看 8 个玩家）
cargo run --bin ww-observer

# 5. 游戏结束后清理所有实例
python -m werewolf kill
```

### 观战界面

Dashboard 和 TUI 面板各自覆盖不同视角：

- **Dashboard** — 浏览器中查看全局游戏状态、事件流、发言记录、Token 用量和玩家终端截屏。运行 `python -m werewolf run` 后打开 `http://127.0.0.1:9876`。
- **ww-observer** — 终端中实时查看 8 个玩家的 rmux 画面。默认是左侧玩家列表 + 右侧选中玩家大屏；按 `g` 可切到网格视图，一屏看所有玩家。

`ww-observer` 快捷键：

| 按键 | 说明 |
|------|------|
| `1`-`8` | 选择玩家并进入聚焦视图 |
| `←` / `↑` | 上一个玩家 |
| `→` / `↓` | 下一个玩家 |
| `f` / `Enter` | 聚焦选中玩家 |
| `g` | 网格视图，一屏显示所有玩家 |
| `r` | 重连并刷新所有玩家 stream |
| `q` / `Esc` | 退出 |

## 命令参考

| 命令 | 说明 |
|------|------|
| `bootstrap [N]` | 启动 N 个 Claude Code 实例（默认 8），自动发现 Task UUID |
| `run` | 开始完整游戏循环，含 Dashboard |
| `status` | 查看所有实例的存活/角色/连接状态 |
| `kill` | 终止所有狼人杀 rmux 会话 |
| `send <name> <msg>` | 向指定实例发送调试消息 |

## 项目结构

```
werewolf/
├── Cargo.toml                # Rust crate 配置 (werewolf-bridge + ww-observer)
├── config.json              # 游戏配置（角色数量、规则参数）
├── pyproject.toml           # 项目配置 & 入口点
├── src/werewolf/
│   ├── __init__.py
│   ├── cli.py               # CLI 入口 & Game Master 主控逻辑
│   ├── game.py              # 纯逻辑游戏引擎（无 I/O 依赖）
│   ├── models.py            # 数据模型（Player, Team, VoteResult）
│   ├── config.py            # 配置加载 & 注册表管理
│   ├── tmux.py              # tmux 操作封装（已 deprecated）
│   ├── rmux_bridge.py       # Python 客户端（JSON-RPC → werewolf-bridge）
│   ├── dashboard.py         # HTTP Dashboard 服务（实时监控 API）
│   ├── logging/             # 游戏日志模块
│   └── static/
│       └── dashboard.html   # Dashboard 前端页面
├── rust/                    # Rust 源码
│   └── src/
│       ├── lib.rs           # 库入口 (protocol/session/pane/capture/server/bridge_state)
│       ├── main.rs          # werewolf-bridge 二进制入口 (stdin/stdout JSON-RPC)
│       ├── observer.rs      # ww-observer TUI (render_stream + PaneSnapshot)
│       ├── protocol.rs      # JSON-RPC 类型定义
│       ├── session.rs       # Session 管理 & 校验
│       ├── pane.rs          # Pane 操作校验 & 格式化
│       ├── capture.rs       # 结构化输出提取（替代启发式解析）
│       ├── server.rs        # RPC 分发器（接入真实 rmux-sdk）
│       └── bridge_state.rs  # Tokio runtime + Rmux 连接管理
├── data/runs/               # 每局游戏的存档（时间戳目录）
│   └── <timestamp>/
│       ├── game_state.json  # 游戏快照
│       ├── game_log.jsonl   # 事件日志
│       └── final_report.md  # 最终报告
├── tests/                   # 测试套件
│   ├── test_rmux_bridge.py # Bridge 集成测试 (28 tests)
│   └── test_game.py         # 游戏逻辑测试
└── docs/                    # 设计文档
```

## 架构概览

```
Python (werewolf)              Rust (werewolf-bridge)         rmux daemon
┌──────────────┐   JSON-RPC    ┌──────────────────┐   IPC   ┌─────────────┐
│ rmux_bridge  │ ───stdin───▶  │ main.rs (server) │ ──────▶ │ sessions    │
│ .py (client) │ ◀──stdout──  │ protocol.rs     │        │ panes       │
│              │              │ session.rs       │        │ PTYs        │
│ cli.py       │              │ pane.rs          │        └─────────────┘
│ logging.py   │              │ capture.rs       │
│              │              │ server.rs        │
│              │              │ bridge_state.rs │
└──────────────┘              └──────────────────┘

                    ┌──────────────────────┐
                    │  ww-observer (TUI)    │
                    │  ratatui + rmux-sdk  │
                    │  render_stream 触发   │
                    │  PaneSnapshot 渲染    │
                    │  聚焦/网格双视图       │
                    │  数字选人 / r 重连     │
                    └──────────────────────┘
```

### Bridge 协议 (NDJSON-RPC)

```
Python ──{"id":1,"method":"list_sessions","params":{}}──▶ Rust
Rust  ──{"id":1,"result":[...],"error":null}──▶ Python

methods:
  "send_text"     → {session, text}
  "capture"       → {session, lines?} → {text, cursor, revision}
  "wait_for"      → {session, text, timeout_sec?} → {}
  "new_session"   → {name, cwd?} → {session_id}
  "list_sessions" → {} → [{name}, ...]
  "kill_session"  → {name} → {}
  "session_exists" {name} → {exists: bool}
```

### 核心设计原则

- **game.py** — 纯逻辑引擎，不依赖任何 I/O，可独立测试
- **cli.py** — Game Master 控制层，编排白天/夜晚流程，通过 bridge 与 AI 实例交互
- **rmux_bridge.py** — 通过 JSON-RPC 调用 Rust bridge，替代原 tmux subprocess
- **rust/** — 全部 TDD 开发，65 个 Rust 测试 + 28 个 Python 测试
- **observer.rs** — 独立 TUI 二进制，通过 rmux-sdk `render_stream` 触发刷新，并渲染 daemon 解析后的 `PaneSnapshot.visible_lines()`，避免直接显示 Claude Code 的 ANSI 控制序列

## 默认角色配置（8 人局）

| 角色 | 数量 | 阵营 | 技能 |
|------|------|------|------|
| 🐺 狼人 | 2 | 狼人阵营 | 夜间共同选择击杀目标 |
| 🙍‍♂️ 村民 | 2 | 好人阵营 | 无特殊技能 |
| 🔮 预言家 | 1 | 好人阵营 | 夜间查验一名玩家身份 |
| 🦨️ 女巫 | 1 | 好人阵营 | 一瓶解药 + 一瓶毒药 |
| 🐫️ 猎人 | 1 | 好人阵营 | 死亡时可开枪带走一人 |
| 🛡️ 守卫 | 1 | 好人阵营 | 夜间守护一名玩家（不能连续守同一人） |

## 规则要点

- **发言超时**: 180s / **投票超时**: 150s / **夜间行动超时**: 120s / **预热等待**: 180s
- 发言等待默认至少 300s；可通过 `rules.min_speak_wait_sec` 和 `rules.speak_poll_interval_sec` 调整，便于本地演示或快速测试
- 平票时不处决，平安度过
- 狼人数 ≥ 好人数时狼人胜利；狼人全灭时好人胜利
- 女巫解药/毒药各限一次使用
- 猎人死亡后可开枪（可选拉人陪葬或不开枪）

## 开发

```bash
# 运行测试
uv run pytest
cargo test                                # Rust 测试 (65 tests)
python -m pytest tests/test_rmux_bridge.py   # Bridge 集成测试 (28 tests)

# 编译 release
cargo build --release

# 安装为可执行命令
uv pip install -e .
werewolf bootstrap 8
```

## License

MIT
