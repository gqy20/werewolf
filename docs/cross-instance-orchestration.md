# Claude Code 跨实例编排：原理与实现

> 基于 Claude Code v2.1 的多实例控制方案，适用于 AI Agent 编排场景

## 1. 背景

Claude Code 支持同时运行多个实例（多个终端窗口/tmux pane 各自启动一个 `claude` 进程）。
这些实例共享同一个 `~/.claude/` 目录，但默认情况下彼此隔离——每个实例只能看到自己的任务和会话。

本文记录了如何**从一个实例观察、注入任务、甚至向另一个实例发送指令**的完整方案。

---

## 2. 架构概览

### 2.1 文件系统布局

```
~/.claude/
├── tasks/                          # 任务持久化存储（所有实例共享）
│   ├── {task-uuid-a}/              # 实例 A 的任务目录
│   │   ├── .highwatermark          # 任务 ID 自增计数器
│   │   ├── .lock                   # 锁文件
│   │   ├── 1.json                  # 任务 1
│   │   └── 2.json                  # 任务 2
│   ├── {task-uuid-b}/              # 实例 B 的任务目录
│   └── ...
│
│   ⚠️ 关键：Task UUID 与 Session UUID 可能不同！
│      注入任务前必须先获取正确的 Task UUID，不能假设两者一致。
│
├── projects/                       # 按项目路径哈希的会话数据
│   └── {project-hash}/             # 当前项目（按实际路径变化）
│       ├── sessions-index.json     # 会话索引
│       ├── {session-uuid-a}.jsonl  # 实例 A 的完整对话日志
│       ├── {session-uuid-b}.jsonl  # 实例 B 的完整对话日志
│       └── ...
│
├── settings.json                   # 全局配置（hooks、权限等）
└── session-env                     # 会话环境变量
```

### 2.2 进程架构

```
终端/TMUX
  └─ claude --dangerously-skip-permissions (主进程)
       ├─ npm exec @z_ai/ (Agent 运行时)
       │    └─ MainThread (Node.js 线程池)
       ├─ uv run python (子进程, 工具执行)
       └─ {claude} (工作线程)

claude daemon run (后台守护进程)
  ├─ bg-pty-host (PTY 多路复用器)
  │    └─ 管理所有实例的终端 I/O
  ├─ spare (备用实例池)
  └─ control.sock (Unix domain socket, 控制接口)
```

### 2.3 关键发现

| 组件 | 路径 | 用途 |
|------|------|------|
| 任务存储 | `~/.claude/tasks/{task-uuid}/{id}.json` | TaskCreate/TaskGet 底层读写 |
| 会话日志 | `~/.claude/projects/{hash}/{session-uuid}.jsonl` | 完整对话记录（只追加） |
| **UUID 不一致** | task-uuid ≠ session-uuid | 注入任务必须用 task-uuid |
| Daemon 控制口 | `/tmp/cc-daemon-{uid}/{daemon-id}/control.sock` | 自定义协议（未公开） |
| PTY 多路复用 | daemon 内部 bg-pty-host | 拦截直接 TTY 写入 |

---

## 3. 已验证的控制能力

### 3.1 读取其他实例的任务 ✅

```bash
# 列出所有有任务的会话
for dir in ~/.claude/tasks/*/; do
  uuid=$(basename "$dir")
  count=$(ls "$dir"*.json 2>/dev/null | wc -l)
  if [ "$count" -gt 0 ]; then
    echo "[$count tasks] $uuid"
    for f in "$dir"*.json; do
      python3 -c "
import json; d=json.load(open('$f'))
print(f'  {d[\"id\"]}. [{d[\"status\"]}] {d[\"subject\"][:60]}')"
    done
  fi
done
```

### 3.2 向其他实例注入任务 ✅

> **⚠️ 重要前提：必须先获取正确的 Task UUID（不是 Session UUID！）**

#### 步骤 0：获取目标的 Task UUID

Task UUID 与 Session UUID **不一定相同**。可靠的做法是先让目标实例创建一个占位任务来暴露其 Task UUID：

```bash
# 方法 A：通过 tmux 发送指令让实例自建一个占位任务
tmux send-keys -t {target} "请用 TaskCreate 创建一个主题为'占位'的任务" Enter
sleep 15

# 方法 B：通过监控 tasks/ 目录的最新变化来推断
LATEST_TASK_UUID=$(ls -lt ~/.claude/tasks/ | head -2 | tail -1 | awk '{print $NF}')
echo "检测到的 Task UUID: $LATEST_TASK_UUID"
```

#### 步骤 1：注入任务

```bash
TARGET_TASK_UUID="{从步骤0获得的正确 UUID}"
TASK_DIR="$HOME/.claude/tasks/$TARGET_TASK_UUID"

mkdir -p "$TASK_DIR"

# 读取当前 highwatermark 避免冲突
HW=$(cat "$TASK_DIR/.highwatermark" 2>/dev/null || echo 0)
NEXT_ID=$((HW + 1))

cat > "$TASK_DIR/${NEXT_ID}.json" << EOF
{
    "id": "${NEXT_ID}",
    "subject": "从外部注入的任务",
    "description": "跨实例任务注入",
    "activeForm": "执行注入任务",
    "status": "pending",
    "blocks": [],
    "blockedBy": []
}
EOF

echo "$NEXT_ID" > "$TASK_DIR/.highwatermark"
```

**效果**：目标实例调用 `TaskList` 时能看到这个任务。

### 3.3 监控其他实例的实时活动 ✅

```bash
# 实时跟踪目标实例的对话日志
tail -f ~/.claude/projects/{project-hash}/{target-session-uuid}.jsonl \
  | python3 -c "
import sys, json
for line in sys.stdin:
    d = json.loads(line.strip())
    t = d.get('type','?')
    ts = d.get('timestamp','')
    if t == 'user':
        msg = d.get('message',{})
        content = msg.get('content','')
        if isinstance(content, list):
            content = ' '.join([c['text'][:100] for c in content if c.get('type')=='text'])
        print(f'[{ts}] USER: {content[:150]}')
    elif t == 'assistant':
        msg = d.get('message',{})
        content = msg.get('content','')
        if isinstance(content, list):
            texts = [c['text'][:200] for c in content if c.get('type')=='text']
            print(f'[{ts}] ASSISTANT: {texts[0][:200] if texts else \"\"}'
"
```

### 3.4 通过 tmux 向实例发送指令 ✅（核心突破）

这是唯一能**让另一个实例执行指令**的方式。

#### 步骤 1：定位目标实例

```bash
# 通过 TTY 找 tmux pane
tmux list-panes -a -F "#{session_name}:#{window_id}.#{pane_index} #{pane_tty} #{pane_pid}" \
  | grep pts/{N}
```

#### 步骤 2：发送指令

```bash
tmux send-keys -t {session}:{window_id}.{pane} "消息内容" Enter
```

---

## 4. 失败的方法与原因分析

### 4.1 直接写入 /dev/pts/N ❌

```bash
echo "消息" > /dev/pts/{N}      # 写入成功但无反应
printf "消息\r\n" > /dev/pts/{N}  # 同样无效
```

**原因**：Claude Code 通过 daemon 的 `bg-pty-host` 进行 PTY 多路复用。
直接写底层 pts 设备文件的字符被 daemon 层拦截/忽略，不会传递给应用层。

### 4.2 TIOCSTI ioctl ❌

```python
import fcntl, termios
with open('/dev/pts/{N}', 'w') as tty:
    for ch in "message\n":
        fcntl.ioctl(tty.fileno(), termios.TIOCSTI, ch.encode())
# OSError: Input/output error
```

**原因**：TIOCSTI 要求调用者是该终端的控制进程，
或系统 `ptrace_scope=0`。当前 Linux 发行版默认 `ptrace_scope=1`（仅允许父→子调试）。

### 4.3 注入 jsonl 日志 ❌

```python
fake_msg = {
    "type": "user",
    "message": {"role": "user", "content": "执行任务"},
}
with open(target_jsonl, 'a') as f:
    f.write(json.dumps(fake_msg) + '\n')
# 写入成功，但实例不读取
```

**原因**：`.jsonl` 是仅追加的审计日志，不是消息队列。
运行中的实例从 stdin/TTY 读取输入，不会轮询此文件。

### 4.4 Daemon control.sock ❌（协议未公开）

```bash
socat - UNIX-CONNECT:/tmp/cc-daemon-{uid}/{daemon-id}/control.sock
# 协议未知（Bun 编译的二进制），尝试过的格式全部返回 "malformed request"
```

**原因**：daemon 使用自定义二进制协议（非标准 JSON-RPC），请求格式未公开。

### 4.5 GDB attach ❌

```bash
gdb -p {PID}
# ptrace: Operation not permitted
```

**原因**：`/proc/sys/kernel/yama/ptrace_scope = 1`，禁止非父子进程间的 ptrace。

---

## 5. 能力矩阵总览

| 控制能力 | tmux 中启动的实例 | 普通终端启动的实例 |
|---------|:---:|:---:|
| **读取**任务列表 | ✅ 直接读 JSON 文件 | ✅ 同左 |
| **注入/创建**任务 | ✅ 写 JSON 文件 | ✅ 同左 |
| **修改**任务状态 | ✅ 改 status 字段 | ✅ 同左 |
| **删除**任务 | ✅ 删除 JSON 文件 | ✅ 同左 |
| **监控**实时对话 | ✅ tail -f jsonl | ✅ 同左 |
| **发送指令**让它执行 | ✅ `tmux send-keys` | ❌ 所有方法均失败 |
| **通过 daemon API 控制** | ❌ 协议未知 | ❌ 同左 |
| **通过 gdb/ptrace 注入** | ❌ 权限限制 | ❌ 同左 |

---

## 6. UUID 发现算法（本项目实现）

本项目的 `werewolf/tmux.py` 中实现了自动化的批量 UUID 发现：

```python
def discover_all_uuids(sessions: list[str], timeout: int = 35,
                       retries: int = 2) -> dict[str, str]:
    """并行发现所有 session 的 Task UUID（带自动重试）

    原理: 给每个实例发带唯一标记的 TaskCreate 指令 →
          并行等待 → 通过 task 文件内容中的标记反查归属 → 返回 {session: uuid}
          未命中的 session 自动重试
    """
```

**核心流程：**

1. 记录当前 `~/.claude/tasks/` 下所有目录快照
2. 向每个待发现的 tmux session 发送探测指令（含唯一标记如 `__probe_0_ww-1__`）
3. 轮询等待新 task 目录出现
4. 遍历新出现的 task 目录，读取其 `1.json` 中的 subject 字段
5. 通过标记字符串反向匹配到对应的 tmux session
6. 未匹配到的 session 进入下一轮重试（最多 retries 轮）

**优势：**
- 并行探测，无需串行等待每个实例响应
- 带唯一标记避免多实例并发时的误匹配
- 自动重试机制应对超时情况

---

## 7. 应用场景示例

### 7.1 多实例并行 Worker 模式

```bash
#!/bin/bash
# 启动 N 个 worker 实例 + 1 个调度器

WORKERS=3
PROJECT="{your-project-path}"

for i in $(seq 1 $WORKERS); do
  tmux new-session -d -s "worker-$i" -c "$PROJECT" \
    "claude --dangerously-skip-permissions --name worker-$i"
done

sleep 10  # 等待初始化完成

# 发现每个 worker 的 Task UUID 后分发任务
for i in $(seq 1 $WORKERS); do
  uuid=$(cc_discover_uuid "worker-$i")
  cc_inject_task "$uuid" "处理批次 $i 的数据"
  tmux send-keys -t "worker-$i" "请列出你的任务并执行第一个待办任务" Enter
done
```

### 7.2 中央监控面板

```bash
#!/bin/bash
# 实时监控所有实例的活动

CC_PROJECT_DIR="$HOME/.claude/projects/{your-project-hash}"

watch -n 2 "bash -c '
echo \"=== Claude Code 集群状态 ===\"
for jsonl in \$(ls -t \"$CC_PROJECT_DIR\"/*.jsonl 2>/dev/null | head -5); do
  uuid=\$(basename \$jsonl .jsonl)
  mod=\$(stat -c %y \$jsonl 2>/dev/null | cut -d. -f1)
  echo \"[\${uuid:0:8}] \$mod\"
  tail -1 \$jsonl | python3 -c \"
import sys,json
try:
    d=json.loads(sys.stdin.read())
    t=d.get(\\\"type\\\",\\\"?\\\")
    if t==\\\"assistant\\\":
        c=d.get(\\\"message\\\",{}).get(\\\"content\\\",\\\"\\\")
        if isinstance(c,list):
            texts=[x.get(\\\"text\\\",\\\"\\\")[:80] for x in c if x.get(\\\"type\\\")==\\\"text\\\"]
            print(f\\\"  → {texts[0] if texts else \\\"\\\"}\\\")
except: pass
\"
done
'"
```

### 7.3 跨实例任务依赖链

```bash
# 实例 A 完成后自动触发实例 B
monitor_and_chain() {
  local uuid_a="$1"  # 上游实例 Task UUID
  local tmux_b="$2"  # 下游 tmux target

  while true; do
    remaining=$(find "$HOME/.claude/tasks/$uuid_a" -name "*.json" \
      -exec python3 -c "
import sys,json
for f in sys.argv[1:]:
    d=json.load(open(f))
    if d.get('status') not in ('completed','deleted'):
        print(f)
" {} + 2>/dev/null | wc -l)

    if [ "$remaining" -eq 0 ]; then
      echo "上游完成，触发下游..."
      tmux send-keys -t "$tmux_b" "上游已完成，请开始你的工作" Enter
      break
    fi
    sleep 10
  done
}
```

---

## 8. 安全注意事项

| 风险 | 说明 | 建议 |
|------|------|------|
| 任务注入 | 任何同用户进程都能读写 `~/.claude/tasks/` | 默认已隔离（每实例独立 UUID 目录） |
| 日志泄露 | jsonl 包含完整对话内容 | 确保 `~/.claude/` 权限为 700 |
| tmux send-keys | 同用户可向任意 tmux pane 发送按键 | 使用 `tmux lock-session` 锁定敏感 session |
| daemon socket | control.sock 权限开放 | 目前协议未破解，风险有限 |

---

## 9. 局限性与未来方向

### 当前局限

1. **普通终端无法发送指令**：必须使用 tmux（或 screen）作为终端复用器
2. **Daemon API 未公开**：control.sock 协议是闭源的
3. **无反馈确认**：`tmux send-keys` 是单向投递
4. **任务 ID 冲突**：手动注入需管理 `.highwatermark`
5. **只能同用户**：跨用户控制需要额外权限配置
6. **Task UUID ≠ Session UUID**：注入前必须先发现正确的 Task UUID

### 可能的改进方向

1. **逆向 daemon 协议**：通过 strace 追踪通信来还原协议
2. **Hook 链式触发**：利用 `TaskCompleted` hook 触发 shell 脚本
3. **文件系统事件监听**：用 `inotifywait` 监控任务目录变化
4. **Remote Control 功能**：Claude Code 原生支持 `--remote-control`，可能提供正式的多实例 API

---

## 附录：快速参考卡

```bash
# === 发现实例 ===
ps aux | grep "claude --" | grep -v grep
tmux list-panes -a -F "#{session_name}:#{window_id}.#{pane_index} #{pane_tty}"

# === 发现 Task UUID（关键前置步骤）===
# 让实例创建带唯一标记的占位任务 → 监控 tasks/ 目录变化 → 匹配标记归属

# === 读任务（使用正确的 Task UUID！）===
cat ~/.claude/tasks/{task-uuid}/{id}.json | python3 -m json.tool

# === 写任务（先读 highwatermark 再写入）===
HW=$(cat ~/.claude/tasks/{task-uuid}/.highwatermark)
cat > ~/.claude/tasks/{task-uuid}/$((HW+1)).json << 'JSON'
{ "id": "N", "subject": "...", "status": "pending", ... }
JSON

# === 发送指令（仅 tmux）===
tmux send-keys -t {session}:{window_id}.{pane} "指令内容" Enter

# === 监控日志（Session UUID，不是 Task UUID）===
tail -f ~/.claude/projects/{project-hash}/{session-uuid}.jsonl
```
