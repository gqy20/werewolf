"""werewolf-bridge Python 客户端

通过 stdin/stdout JSON-RPC 与 Rust bridge 二进制通信。
替代原来的 tmux subprocess 调用。

用法:
    bridge = RmuxBridge()
    bridge.send_text("ww-1", "hello")
    text = bridge.capture("ww-1")
    ok = bridge.wait_for("ww-1", "ready")
"""

import json
import subprocess
import os
import sys
from pathlib import Path
from typing import Optional


class RmuxBridge:
    """werewolf-bridge JSON-RPC 客户端"""

    def __init__(self, binary_path: Optional[str] = None):
        self.binary_path = binary_path or self._find_binary()
        self._proc: Optional[subprocess.Popen] = None

    def _find_binary(self) -> str:
        """自动定位 werewolf-bridge 二进制"""
        # 优先: 项目 target/debug 或 target/release
        candidates = [
            Path(__file__).parent / "target" / "debug" / "werewolf-bridge",
            Path(__file__).parent / "target" / "release" / "werewolf-bridge",
        ]
        for p in candidates:
            if p.exists():
                return str(p)
        # 回退: PATH 中查找
        which = shutil.which("werewolf-bridge")
        if which:
            return which
        raise FileNotFoundError(
            "werewolf-bridge binary not found. "
            "Run: cargo build --manifest-path ../../rmux/crates/rmux-sdk "
            "(or install rmux and set up the bridge separately)"
        )

    def _ensure_running(self) -> subprocess.Popen:
        if self._proc and self._proc.poll() is None:
            return self._proc
        self._proc = subprocess.Popen(
            [self.binary_path],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=False,  # binary mode for NDJSON
        )
        return self._proc

    def _call(self, method: str, params: dict) -> dict:
        """发送 RPC 请求并返回解析后的响应"""
        proc = self._ensure_running()
        req = {"id": _RequestId.next_id(), "method": method, "params": params}
        try:
            proc.stdin.write((json.dumps(req) + "\n").encode())
            proc.stdin.flush()
            line = proc.stdout.readline()
            if not line:
                raise BridgeError("empty response from bridge")
            resp = json.loads(line)
        except (json.JSONDecodeError, OSError, BrokenPipeError) as e:
            raise BridgeError(f"bridge communication error: {e}")
        finally:
            pass  # 保持进程存活供后续调用

        if resp.get("error") is not None:
            err = resp["error"]
            raise BridgeError(f"[{err.get('code')}] {err['message']}")
        return resp.get("result")

    # ── 公开 API（兼容原 tmux.py 接口）─────────────

    def send_text(self, session: str, text: str):
        """向 session 发送文字（含回车）"""
        return self._call("send_text", {"session": session, "text": text})

    def capture(self, session: str, lines: int = 50) -> dict:
        """读取 session 的屏幕输出"""
        return self._call("capture", {"session": session, "lines": lines})

    def wait_for(self, session: str, text: str, timeout_sec: int = 30) -> None:
        """等待 session 输出包含指定文本，超时返回 False"""
        result = self._call("wait_for", {
            "session": session, "text": text, "timeout_sec": timeout_sec,
        })
        return True  # 成功匹配

    def new_session(self, name: str, cwd: Optional[str] = None) -> dict:
        """创建新 session（detached）"""
        params = {"name": name}
        if cwd:
            params["cwd"] = cwd
        return self._call("new_session", params)

    def list_sessions(self) -> list[dict]:
        """列出所有 session"""
        result = self._call("list_sessions", {})
        return result if isinstance(result, list) else []

    def kill_session(self, name: str):
        """销毁 session"""
        self._call("kill_session", {"name": name})

    def session_exists(self, name: str) -> bool:
        """检查 session 是否存在"""
        result = self._call("session_exists", {"name": name})
        return result.get("exists", False)

    def shutdown(self):
        """关闭 bridge 进程"""
        if self._proc:
            self._proc.terminate()
            self._proc = None

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        self.shutdown()


class BridgeError(Exception):
    """Bridge 通信错误"""


class _RequestId:
    """简单的自增 ID 生成器（线程不安全，单线程够用）"""
    _counter = 0

    @classmethod
    def next_id(cls) -> int:
        cls._counter += 1
        return cls._counter


import shutil
