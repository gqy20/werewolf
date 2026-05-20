"""tmux 操作封装 + 消息解析"""
import re
import subprocess
import time
from pathlib import Path

TASKS_DIR = Path.home() / ".claude" / "tasks"


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
