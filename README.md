# 🐺 Werewolf — Claude Code 狼人杀 Game Master

基于 **Claude Code 多实例** 的 AI 狼人杀游戏系统。每个玩家由一个独立的 Claude Code 实例扮演，通过 tmux 会话进行消息收发与行为控制，Game Master 协调完整的游戏流程。

## 特性

- **多实例 AI 对战** — 每位玩家是一个独立的 Claude Code 实例，拥有自己的工作目录和上下文
- **完整角色系统** — 狼人、村民、预言家、女巫、猎人、守卫
- **标准游戏流程** — 白天发言 → 投票处决 → 夜晚行动（狼刀/守卫/预言家/女巫）→ 结算
- **实时 Dashboard** — 内置 HTTP 监控面板，实时查看游戏状态、日志和玩家屏幕
- **游戏日志** — 每局自动记录完整日志（JSONL）和最终报告（Markdown）
- **tmux 跨实例控制** — 通过 Task UUID 探测机制实现会话发现与消息分发

## 快速开始

### 环境要求

- Python >= 3.11
- [uv](https://docs.astral.sh/uv/) 包管理器
- [tmux](https://github.com/tmux/tmux/wiki) 终端复用器
- [Claude Code CLI](https://claude.ai/code)

### 安装

```bash
git clone <repo-url>
cd werewolf
uv sync
```

### 运行一局游戏

```bash
# 1. 启动 8 个 Claude Code 实例作为玩家
python -m werewolf bootstrap 8

# 2. 开始游戏（自动启动 Dashboard）
python -m werewolf run

# 3. 查看实时状态
python -m werewolf status

# 4. 游戏结束后清理所有实例
python -m werewolf kill
```

## 命令参考

| 命令 | 说明 |
|------|------|
| `bootstrap [N]` | 启动 N 个 Claude Code 实例（默认 8），自动发现 Task UUID |
| `run` | 开始完整游戏循环，含 Dashboard |
| `status` | 查看所有实例的存活/角色/连接状态 |
| `kill` | 终止所有狼人杀 tmux 会话 |
| `send <name> <msg>` | 向指定实例发送调试消息 |

## 项目结构

```
werewolf/
├── config.json              # 游戏配置（角色数量、规则参数）
├── pyproject.toml           # 项目配置 & 入口点
├── src/werewolf/
│   ├── __init__.py
│   ├── cli.py               # CLI 入口 & Game Master 主控逻辑
│   ├── game.py              # 纯逻辑游戏引擎（无 I/O 依赖）
│   ├── models.py            # 数据模型（Player, Team, VoteResult）
│   ├── config.py            # 配置加载 & 注册表管理
│   ├── tmux.py              # tmux 操作封装 + 消息解析 + UUID 发现
│   ├── dashboard.py         # HTTP Dashboard 服务（实时监控 API）
│   ├── logging/             # 游戏日志模块
│   └── static/
│       └── dashboard.html   # Dashboard 前端页面
├── data/runs/               # 每局游戏的存档（时间戳目录）
│   └── <timestamp>/
│       ├── game_state.json  # 游戏快照
│       ├── game_log.jsonl   # 事件日志
│       └── final_report.md  # 最终报告
├── tests/                   # 测试套件
└── docs/                    # 设计文档
```

## 架构概览

```
┌─────────────┐    tmux send/capture    ┌──────────────────┐
│  Game Master │ ◄─────────────────────► │  ww-1 (Player 1) │
│  (cli.py)    │                         │  Claude Code     │
│              │ ◄─────────────────────► │  ww-2 (Player 2) │
│              │                         │  ...             │
│              │ ◄─────────────────────► │  ww-N (Player N) │
└──────┬───────┘                         └──────────────────┘
       │
       │  GameLogger
       ▼
┌──────────────┐     HTTP API      ┌─────────────────┐
│  game_state  │ ◄────────────────► │  Dashboard UI   │
│  game_log    │    :9876          │  (浏览器访问)    │
└──────────────┘                    └─────────────────┘
```

**核心设计原则：**

- **game.py** — 纯逻辑引擎，不依赖任何 I/O，可独立测试
- **cli.py** — Game Master 控制层，编排白天/夜晚流程，通过 tmux 与 AI 实例交互
- **tmux.py** — 基础设施层，封装 tmux 操作、UUID 发现算法、回复/投票解析
- **dashboard.py** — 观察者层，提供只读 API 供前端消费

## 默认角色配置（8 人局）

| 角色 | 数量 | 阵营 | 技能 |
|------|------|------|------|
| 🐺 狼人 | 2 | 狼人阵营 | 夜间共同选择击杀目标 |
| 🙍‍⅌️ 村民 | 2 | 好人阵营 | 无特殊技能 |
| 🔮 预言家 | 1 | 好人阵营 | 夜间查验一名玩家身份 |
| 🦨️ 女巫 | 1 | 好人阵营 | 一瓶解药 + 一瓶毒药 |
| 🐫️ 猎人 | 1 | 好人阵营 | 死亡时可开枪带走一人 |
| 🛡️ 守卫 | 1 | 好人阵营 | 夜间守护一名玩家（不能连续守同一人） |

## 规则要点

- **发言超时**: 60s / **投票超时**: 45s / **夜间行动超时**: 40s
- 平票时不处决，平安度过
- 狼人数 ≥ 好人数时狼人胜利；狼人全灭时好人胜利
- 女巫解药/毒药各限一次使用
- 猎人死亡后可开枪（可选拉人陪葬或不开枪）

## 开发

```bash
# 运行测试
uv run pytest

# 安装为可执行命令
uv pip install -e .
werewolf bootstrap 8
```

## License

MIT
