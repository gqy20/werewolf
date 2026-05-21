"""tmux 操作封装 + 消息解析"""
import glob
import json
import re
import subprocess
import time
from pathlib import Path

TASKS_DIR = Path.home() / ".claude" / "tasks"
CLAUDE_DIR = Path.home() / ".claude"


# ── tmux 基本操作 ──────────────────────────────────────

def tmux_send(session: str, text: str):
    """向 tmux session 发送文字并回车"""
    subprocess.run(
        ["tmux", "send-keys", "-t", session, text, "Enter"],
        capture_output=True,
    )


def tmux_capture(session: str, lines: int = 50) -> str:
    r = subprocess.run(
        ["tmux", "capture-pane", "-t", session, "-p", f"-S-{lines}"],
        capture_output=True, text=True,
    )
    return r.stdout.strip()


def session_exists(name: str) -> bool:
    return subprocess.run(
        ["tmux", "has-session", "-t", name], capture_output=True,
    ).returncode == 0


def kill_session(name: str):
    subprocess.run(["tmux", "kill-session", "-t", name], capture_output=True)


def list_sessions(prefix: str = "") -> list[str]:
    r = subprocess.run(
        ["tmux", "list-sessions", "-F", "#{session_name}"],
        capture_output=True, text=True,
    )
    if r.returncode != 0:
        return []
    # tmux 会加数字前缀如 "6-ww-1"，用 in 匹配
    return [s for s in r.stdout.strip().split("\n") if prefix in s]


# ── UUID 发现 ────────────────────────────────────────────

def discover_all_uuids(sessions: list[str], timeout: int = 35,
                       retries: int = 2) -> dict[str, str]:
    """并行发现所有 session 的 Task UUID（带自动重试）

    原理: 给每个实例发带唯一标记的 TaskCreate 指令 →
          并行等待 → 通过 task 文件内容中的标记反查归属 → 返回 {session: uuid}
          未命中的 session 自动重试
    """
    import json as _json

    found: dict[str, str] = {}
    remaining = list(sessions)

    for attempt in range(retries + 1):
        if not remaining:
            break

        before_dirs = _all_task_dirs()

        # 给未发现的实例发探测指令（每轮换不同标记避免缓存）
        for sess in remaining:
            marker = f"__probe_{attempt}_{sess}__"
            tmux_send(sess,
                      f"请立即用 TaskCreate 创建一个主题为'{marker}'的任务，不要做其他事")

        # 轮询
        deadline = time.time() + timeout
        while time.time() < deadline:
            time.sleep(1)
            current_dirs = _all_task_dirs()
            newly_found: dict[str, str] = {}

            for d in current_dirs:
                if d in before_dirs or d in found.values():
                    continue
                task_file = TASKS_DIR / d / "1.json"
                if not task_file.exists():
                    continue
                try:
                    data = _json.loads(task_file.read_text())
                    subject = data.get("subject", "")
                    for sess in remaining:
                        if f"__probe_{attempt}_{sess}__" in subject:
                            newly_found[sess] = d
                            break
                except (json.JSONDecodeError, OSError):
                    pass

            if newly_found:
                found.update(newly_found)
                remaining = [s for s in remaining if s not in newly_found]
                if not remaining:
                    break

        # 更新 before 避免下一轮误匹配旧目录
        before_dirs = _all_task_dirs()

    # 对仍未找到的返回空字符串
    return {sess: found.get(sess, "") for sess in sessions}


def discover_task_uuid(session: str, timeout: int = 30) -> str | None:
    """单个实例的 UUID 发现（内部调用批量版）"""
    results = discover_all_uuids([session], timeout)
    return results.get(session) or None


def _latest_task_dir() -> str | None:
    dirs = _all_task_dirs_sorted()
    return dirs[0] if dirs else None


def _all_task_dirs() -> set[str]:
    """获取当前所有 task 目录名"""
    if not TASKS_DIR.exists():
        return set()
    try:
        return {e.name for e in TASKS_DIR.iterdir() if e.is_dir()}
    except Exception:
        return set()


def _all_task_dirs_sorted() -> list[str]:
    """按修改时间倒序排列的 task 目录名列表"""
    if not TASKS_DIR.exists():
        return []
    try:
        entries = sorted(
            [e for e in TASKS_DIR.iterdir() if e.is_dir()],
            key=lambda p: p.stat().st_mtime, reverse=True,
        )
        return [e.name for e in entries]
    except Exception:
        return []


# ── 消息解析 ────────────────────────────────────────────

# 需要忽略的行前缀/关键词
_IGNORE_PREFIXES = {
    "❯", "│", "╭", "╰", "├", "└", "─", "═", "✽",
}
_IGNORE_KEYWORDS = {
    "请", "轮到", "输入", "选项", "worker-", "glm", "API",
    "Welcome", "What's", "bypass", "qy113@", "Tips for",
    "Added", "Status line", "/release-notes",
    "发表看法", "简短", "存活", "speak_log", "读取",
    "选择处决", "你的投票", "刀谁", "守护谁", "查验谁",
    "女巫睁眼", "被袭", "解药", "毒药", "不用", "开枪",
    "你已死亡", "留遗言", "天亮了", "夜幕降临", "闭眼",
}


def extract_reply(output: str) -> str:
    """从 tmux 输出中提取玩家回复内容"""
    for line in reversed(output.split("\n")):
        line = line.strip()
        if not line:
            continue
        if any(line.startswith(p) for p in _IGNORE_PREFIXES):
            continue
        if any(kw in line for kw in _IGNORE_KEYWORDS):
            continue
        if len(line) > 4 and not line.startswith("*"):
            return line
    return ""


def extract_vote(output: str, candidates: list[str]) -> str | None:
    """从输出中解析投票，返回候选者名字或 None"""
    # 先尝试数字匹配
    numbers = re.findall(r'\b([1-9])\b', output)
    if numbers:
        idx = int(numbers[-1]) - 1
        if 0 <= idx < len(candidates):
            return candidates[idx]

    # 再尝试直接匹配名字（不区分大小写）
    lower_output = output.lower()
    for name in candidates:
        if name.lower() in lower_output:
            return name

    return None


def extract_number(output: str) -> int | None:
    m = re.search(r'\b([1-9])\b', output)
    return int(m.group(1)) if m else None


# ── jsonl 日志提取（替代 tmux capture）────────────────

_NOISE_TEXT_PREFIXES = (
    "__probe_", "TaskCreate", "Cogitated for", "Worked for",
    "Churned for", "Sautéed for", "Bypassed for",
)

# 缓存: session_name → jsonl Path，避免重复扫描
_jsonl_cache: dict[str, Path] = {}


def _find_projects_dir() -> Path | None:
    """找到当前 werewolf 项目对应的 Claude projects 目录"""
    # 通过 registry 或 arena 目录名匹配
    candidates = sorted(
        CLAUDE_DIR.glob("projects/*2605*werewolf*"),
        key=lambda p: p.stat().st_mtime if p.is_dir() else 0,
        reverse=True,
    )
    return candidates[0] if candidates else None


def find_jsonl_path(session_name: str, agent_name: str | None = None) -> Path | None:
    """根据 tmux session 名或 agent name 找到对应的 jsonl 文件路径"""
    if session_name in _jsonl_cache:
        p = _jsonl_cache[session_name]
        return p if p.exists() else None

    proj_dir = _find_projects_dir()
    if not proj_dir:
        return None

    target = agent_name or session_name
    for jf in glob.glob(str(proj_dir / "*.jsonl")):
        try:
            with open(jf, encoding="utf-8") as f:
                for line in f:
                    d = json.loads(line)
                    if d.get("type") == "agent-name" and d.get("agentName") == target:
                        p = Path(jf)
                        _jsonl_cache[session_name] = p
                        return p
        except (json.JSONDecodeError, OSError):
            continue
    return None


def extract_reply_from_jsonl(jsonl_path: Path,
                             since_ts: str | None = None) -> str:
    """从 jsonl 日志中提取玩家最后一条有意义的回复文本

    Args:
        jsonl_path: 玩家的对话日志文件路径
        since_ts: 只读取此时间戳之后的消息（用于获取新回复）
    Returns:
        最后一条有效的 assistant text 内容，空字符串表示无有效回复
    """
    if not jsonl_path or not jsonl_path.exists():
        return ""

    best_text = ""
    try:
        with open(jsonl_path, encoding="utf-8") as f:
            for line in f:
                d = json.loads(line)
                # 只看 assistant 消息
                if d.get("type") != "assistant":
                    continue
                # 时间过滤：只取 since_ts 之后的消息（兼容 Z/+00:00 格式）
                ts = d.get("timestamp", "")
                if since_ts:
                    ts_norm = ts.replace("Z", "+00:00").split(".")[0]
                    ref_norm = since_ts.replace("Z", "+00:00").split(".")[0]
                    if ts_norm <= ref_norm:
                        continue
                # 提取 text block
                content = d.get("message", {}).get("content", [])
                if not isinstance(content, list):
                    continue
                for block in content:
                    if not isinstance(block, dict) or block.get("type") != "text":
                        continue
                    text = block.get("text", "")
                    # 过滤噪音和过短内容
                    if len(text) <= 10:
                        continue
                    if text.startswith(_NOISE_TEXT_PREFIXES):
                        continue
                    # 过滤包含 bootstrap 探测/计时器等残留噪音
                    if any(n in text for n in ("__probe_", "Cogitated", "Worked for",
                                           "Churned", "Sautéed", "Bypassed")):
                        continue
                    # 取最后一条有效文本（最新的）
                    best_text = text
    except (json.JSONDecodeError, OSError):
        pass
    return best_text


def build_jsonl_map(registry: dict) -> dict[str, Path]:
    """批量构建 session_name → jsonl_path 映射表"""
    result = {}
    for sess, pdata in registry.get("players", {}).items():
        aname = pdata.get("display_name")
        jpath = find_jsonl_path(sess, aname)
        if jpath:
            result[sess] = jpath
    return result
