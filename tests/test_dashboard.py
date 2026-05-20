"""Dashboard API + 前端 TDD 测试"""
import json
import threading
import time
import urllib.request
import urllib.error
from urllib.parse import quote

import pytest


def _get(url, timeout=5):
    """用标准库发 GET 请求，返回 (status_code, json_data_or_text)"""
    try:
        resp = urllib.request.urlopen(url, timeout=timeout)
        body = resp.read().decode("utf-8")
        try:
            return resp.status, json.loads(body)
        except json.JSONDecodeError:
            return resp.status, body
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace")
        try:
            return e.code, json.loads(body)
        except json.JSONDecodeError:
            return e.code, body


@pytest.fixture
def logger_with_data(tmp_path, eight_player_game):
    """一个已有状态和日志的 GameLogger 实例"""
    from werewolf.logging import GameLogger

    gl = GameLogger(run_dir=tmp_path / "test_run")
    gl.save_state(eight_player_game)
    gl.log("role_assigned", roles={"p1": "villager", "p2": "werewolf"})
    gl.log("speak", player="p1", msg="我觉得p2很可疑")
    gl.log("vote", voter="p1", target="p2")
    gl.log("vote_result", executed="p2", votes={"p1": "p2", "p3": "p2"})
    return gl


@pytest.fixture
def dashboard_server(logger_with_data):
    """启动 Dashboard HTTP 服务器（后台线程），返回 base_url"""
    from werewolf.dashboard import create_app
    from http.server import HTTPServer

    handler_cls = create_app(logger_with_data)
    port = _find_free_port()
    httpd = HTTPServer(("127.0.0.1", port), handler_cls)
    t = threading.Thread(target=httpd.serve_forever, daemon=True)
    t.start()
    time.sleep(0.3)
    yield f"http://127.0.0.1:{port}"
    httpd.shutdown()


class TestStateAPI:
    """GET /api/state — 游戏状态"""

    def test_returns_state_json(self, dashboard_server):
        status, data = _get(f"{dashboard_server}/api/state")
        assert status == 200
        assert "round" in data
        assert "phase" in data
        assert "alive" in data
        assert "dead" in data
        assert "roles" in data

    def test_alive_list_matches_game(self, dashboard_server, eight_player_game):
        _, data = _get(f"{dashboard_server}/api/state")
        expected_alive = [n for n, p in eight_player_game.players.items() if p.alive]
        assert data["alive"] == expected_alive

    def test_roles_present(self, dashboard_server):
        _, data = _get(f"{dashboard_server}/api/state")
        assert len(data["roles"]) > 0
        for name, info in data["roles"].items():
            assert "role" in info
            assert "alive" in info


class TestLogAPI:
    """GET /api/log — 事件日志流"""

    def test_returns_log_entries(self, dashboard_server):
        status, data = _get(f"{dashboard_server}/api/log")
        assert status == 200
        assert isinstance(data, list)

    def test_entries_have_required_fields(self, dashboard_server):
        _, entries = _get(f"{dashboard_server}/api/log")
        for e in entries:
            assert "event" in e
            assert "ts" in e

    def test_since_parameter_filters(self, dashboard_server):
        _, all_entries = _get(f"{dashboard_server}/api/log")
        if len(all_entries) >= 2:
            # 用一个未来的时间戳，确保能过滤掉所有条目
            future_ts = "2099-12-31T23:59:59+00:00"
            _, filtered = _get(f"{dashboard_server}/api/log?since={future_ts}")
            assert len(filtered) == 0
            # 用一个过去的时间戳，不过滤任何条目
            past_ts = "2020-01-01T00:00:00+00:00"
            _, all_again = _get(f"{dashboard_server}/api/log?since={quote(past_ts)}")
            assert len(all_again) == len(all_entries)

    def test_speak_event_content(self, dashboard_server):
        _, entries = _get(f"{dashboard_server}/api/log")
        speak_events = [e for e in entries if e["event"] == "speak"]
        assert len(speak_events) >= 1
        assert "player" in speak_events[0]
        assert "msg" in speak_events[0]


class TestScreensAPI:
    """GET /api/screens — 玩家屏幕捕获"""

    def test_returns_screen_dict(self, dashboard_server):
        status, data = _get(f"{dashboard_server}/api/screens")
        assert status == 200
        assert isinstance(data, dict)


class TestStaticServing:
    """静态文件服务"""

    def test_serves_dashboard_html(self, dashboard_server):
        status, body = _get(dashboard_server)
        assert status == 200
        assert isinstance(body, str)


class TestGameLoggerIntegration:
    """GameLogger 本身的正确性（dashboard 依赖的数据源）"""

    def test_save_state_produces_valid_json(self, tmp_path, eight_player_game):
        from werewolf.logging import GameLogger

        gl = GameLogger(run_dir=tmp_path / "run1")
        gl.save_state(eight_player_game)
        raw = json.loads(gl.state_path.read_text())
        assert raw["round"] == eight_player_game.round_num
        assert "alive" in raw
        assert "roles" in raw

    def test_log_appends_to_jsonl(self, tmp_path):
        from werewolf.logging import GameLogger

        gl = GameLogger(run_dir=tmp_path / "run2")
        gl.log("test_event", key="value")
        entries = gl._read_log()
        assert len(entries) == 1
        assert entries[0]["event"] == "test_event"
        assert entries[0]["key"] == "value"

    def test_multiple_logs_append(self, tmp_path):
        from werewolf.logging import GameLogger

        gl = GameLogger(run_dir=tmp_path / "run3")
        gl.log("e1")
        gl.log("e2")
        gl.log("e3")
        assert len(gl._read_log()) == 3

    def test_write_report_creates_file(self, tmp_path, eight_player_game):
        from werewolf.logging import GameLogger

        gl = GameLogger(run_dir=tmp_path / "run4")
        path = gl.write_report(eight_player_game)
        assert path.exists()
        content = path.read_text()
        assert "# " in content


def _find_free_port():
    import socket
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    s.bind(("", 0))
    port = s.getsockname()[1]
    s.close()
    return port
