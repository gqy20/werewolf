"""werewolf-bridge Python 客户端集成测试

测试 RmuxBridge 与编译好的 werewolf-bridge 二进制之间的通信。
这些测试需要 cargo 编译出的二进制存在。
"""

import json
import subprocess
import pytest
import sys
from pathlib import Path

# 确保项目根目录在 sys.path 中
PROJECT_ROOT = Path(__file__).parent.parent
sys.path.insert(0, str(PROJECT_ROOT / "src"))

from werewolf.rmux_bridge import RmuxBridge, BridgeError, _RequestId


# ── Fixtures ──────────────────────────────────────────

@pytest.fixture(scope="module")
def bridge_binary():
    """定位或编译 werewolf-bridge 二进制"""
    candidates = [
        PROJECT_ROOT / "target" / "debug" / "werewolf-bridge",
        PROJECT_ROOT / "target" / "release" / "werewolf-bridge",
    ]
    for p in candidates:
        if p.exists():
            return str(p)
    pytest.skip("werewolf-bridge binary not found (run: cargo build)")


@pytest.fixture
def bridge(bridge_binary):
    """创建新的 RmuxBridge 实例"""
    b = RmuxBridge(binary_path=bridge_binary)
    yield b
    b.shutdown()


@pytest.fixture
def raw_bridge_proc(bridge_binary):
    """直接启动 bridge 子进程（用于底层测试）"""
    proc = subprocess.Popen(
        [bridge_binary],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    yield proc
    proc.terminate()
    proc.wait()


def _send_raw(proc, request_dict):
    """向 bridge 进程发送原始 JSON-RPC 请求，返回解析后的响应"""
    proc.stdin.write((json.dumps(request_dict) + "\n").encode())
    proc.stdin.flush()
    line = proc.stdout.readline()
    return json.loads(line)


# ── 底层协议测试（直接子进程）──────────────────────

class TestRawProtocol:
    """测试二进制进程的 NDJSON 协议层"""

    def test_list_sessions_returns_array(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {"id": 1, "method": "list_sessions", "params": {}})
        assert resp["id"] == 1
        assert "result" in resp
        assert isinstance(resp["result"], list)

    def test_unknown_method_error_code(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {"id": 2, "method": "bad_method", "params": {}})
        assert "error" in resp
        assert resp["error"]["code"] == -32601

    def test_invalid_json_error(self, raw_bridge_proc):
        raw_bridge_proc.stdin.write(b"not json\n")
        raw_bridge_proc.stdin.flush()
        line = raw_bridge_proc.stdout.readline()
        resp = json.loads(line)
        assert "error" in resp

    def test_send_text_ok(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {
            "id": 3, "method": "send_text",
            "params": {"session": "s1", "text": "hello"},
        })
        assert resp["id"] == 3
        assert "error" not in resp or resp["error"] is None

    def test_capture_ok(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {
            "id": 4, "method": "capture",
            "params": {"session": "s1", "lines": 10},
        })
        assert resp["id"] == 4

    def test_wait_for_ok(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {
            "id": 5, "method": "wait_for",
            "params": {"session": "s1", "text": "ready"},
        })
        assert resp["id"] == 5

    def test_new_session_requires_name(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {
            "id": 6, "method": "new_session",
            "params": {},  # 缺少 name
        })
        assert "error" in resp
        assert resp["error"]["code"] == -32602

    def test_kill_session_requires_name(self, raw_bridge_proc):
        resp = _send_raw(raw_bridge_proc, {
            "id": 7, "method": "kill_session",
            "params": {},
        })
        assert "error" in resp

    def test_batch_requests_sequential(self, raw_bridge_proc):
        """多个请求顺序处理，每个返回独立响应"""
        results = []
        for i in range(5):
            resp = _send_raw(raw_bridge_proc, {
                "id": i + 10, "method": "list_sessions", "params": {},
            })
            results.append(resp)
        assert len(results) == 5
        for i, r in enumerate(results):
            assert r["id"] == i + 10


# ── Python 客户端测试 ────────────────────────────────

class TestRmuxBridgeClient:
    """测试 RmuxBridge Python 封装类"""

    def test_init_with_binary_path(self, bridge_binary):
        b = RmuxBridge(binary_path=bridge_binary)
        assert b.binary_path == bridge_binary
        b.shutdown()

    def test_list_sessions(self, bridge):
        result = bridge.list_sessions()
        assert isinstance(result, list)

    def test_send_text_no_exception(self, bridge):
        # 不应该抛异常（即使 session 不存在也返回 ok）
        result = bridge.send_text("nonexistent-session", "test input")
        assert result is None  # send_text 返回 None/空

    def test_capture_returns_dict(self, bridge):
        result = bridge.capture("any-session")
        assert isinstance(result, dict)

    def test_new_session_valid(self, bridge):
        result = bridge.new_session("test-integration-sess")
        assert "name" in result

    def test_kill_session_no_exception(self, bridge):
        bridge.kill_session("nonexistent")

    def test_session_exists_false_for_missing(self, bridge):
        exists = bridge.session_exists("surely-not-exists-xyz")
        assert exists is False

    def test_context_manager(self, bridge_binary):
        with RmuxBridge(binary_path=bridge_binary) as b:
            result = b.list_sessions()
            assert isinstance(result, list)
        # exit 后进程应已关闭

    def test_shutdown_idempotent(self, bridge):
        bridge.shutdown()
        bridge.shutdown()  # 第二次不应报错

    def test_unknown_method_raises(self, bridge):
        with pytest.raises(BridgeError):
            bridge._call("totally_fake_method", {})


# ── ID 生成器测试 ────────────────────────────────────

class TestRequestIdGenerator:
    """测试自增 ID 生成器"""

    def test_ids_are_unique(self):
        ids = [_RequestId.next_id() for _ in range(100)]
        assert len(set(ids)) == 100

    def test_ids_are_sequential(self):
        _RequestId._counter = 0
        assert _RequestId.next_id() == 1
        assert _RequestId.next_id() == 2
        assert _RequestId.next_id() == 3

    def test_counter_persists(self):
        start = _RequestId.next_id()
        next_val = _RequestId.next_id()
        assert next_val == start + 1


# ── BridgeError 测试 ─────────────────────────────────

class TestBridgeError:
    def test_str_format(self):
        err = BridgeError("[-32001] method not found")
        assert "-32001" in str(err)
        assert "method not found" in str(err)

    def test_is_exception(self):
        assert issubclass(BridgeError, Exception)


# ── API 兼容性测试（与 tmux.py 接口对比）────────────

class TestTmuxApiCompatibility:
    """验证 RmuxBridge 提供的方法签名与原 tmux.py 兼容"""

    REQUIRED_METHODS = [
        "send_text", "capture", "wait_for",
        "new_session", "list_sessions",
        "kill_session", "session_exists",
    ]

    def test_all_required_methods_exist(self, bridge):
        for method_name in self.REQUIRED_METHODS:
            assert hasattr(bridge, method_name), f"Missing method: {method_name}"
            assert callable(getattr(bridge, method_name)), f"Not callable: {method_name}"

    def test_send_text_signature(self, bridge):
        import inspect
        sig = inspect.signature(RmuxBridge.send_text)  # unbound method
        params = list(sig.parameters.keys())
        assert "self" in params
        assert "session" in params
        assert "text" in params

    def test_capture_default_lines(self, bridge):
        import inspect
        sig = inspect.signature(bridge.capture)
        assert sig.parameters.get("lines").default == 50

    def test_wait_for_default_timeout(self, bridge):
        import inspect
        sig = inspect.signature(bridge.wait_for)
        assert sig.parameters.get("timeout_sec").default == 30
