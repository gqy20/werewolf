"""Dashboard HTTP 服务 — 实时监控狼人杀对局"""
import json
import time
from http.server import HTTPServer, SimpleHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import urlparse, parse_qs

from werewolf.tmux import build_jsonl_map, extract_token_usage

STATIC_DIR = Path(__file__).parent / "static"

_DEFAULT_PORT = 9876


def _make_handler(logger):
    """创建绑定 logger 的请求处理器类"""

    class H(SimpleHTTPRequestHandler):
        _logger = logger

        def do_GET(self):
            parsed = urlparse(self.path)
            path = parsed.path.rstrip("/") or "/"
            query = parse_qs(parsed.query)

            routes = {
                "/": self._serve_index,
                "/api/state": self._api_state,
                "/api/log": self._api_log,
                "/api/screens": self._api_screens,
                "/api/speak-log": self._api_speak_log,
                "/api/usage": self._api_usage,
                "/api/events": self._api_events,
            }
            handler = routes.get(path)
            if handler:
                handler(query)
            else:
                self._serve_static(path)

        def _json(self, data, status=200):
            body = json.dumps(data, ensure_ascii=False).encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "application/json; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _html(self, content, status=200):
            body = content.encode("utf-8")
            self.send_response(status)
            self.send_header("Content-Type", "text/html; charset=utf-8")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def _serve_index(self, _query):
            index = STATIC_DIR / "dashboard.html"
            if index.exists():
                self._html(index.read_text(encoding="utf-8"))
            else:
                self._json({"error": "dashboard.html not found"}, 404)

        def _serve_static(self, path):
            safe = path.lstrip("/")
            file_path = STATIC_DIR / safe
            if file_path.is_file() and str(file_path).startswith(str(STATIC_DIR)):
                self._html(file_path.read_text(encoding="utf-8"))
            else:
                self._json({"error": "not found"}, 404)

        def _api_state(self, _query):
            logger = self._logger
            if not logger or not logger.state_path.exists():
                self._json({"error": "no state"}, 404)
                return
            raw = json.loads(logger.state_path.read_text(encoding="utf-8"))
            self._json(raw)

        def _api_log(self, query):
            logger = self._logger
            if not logger or not logger.log_path.exists():
                self._json([], 200)
                return
            entries = logger._read_log()
            offset_raw = query.get("offset", [None])[0]
            if offset_raw is not None:
                try:
                    offset = max(0, int(offset_raw))
                except ValueError:
                    self._json({"error": "offset must be an integer"}, 400)
                    return
                self._json({
                    "offset": offset,
                    "next_offset": len(entries),
                    "entries": entries[offset:],
                    "total": len(entries),
                })
                return
            since = query.get("since", [None])[0]
            if since:
                entries = [e for e in entries if e.get("ts", "") > since]
            self._json(entries)

        def _log_page(self, offset: int) -> dict:
            logger = self._logger
            entries = logger._read_log() if logger and logger.log_path.exists() else []
            offset = min(max(0, offset), len(entries))
            return {
                "offset": offset,
                "next_offset": len(entries),
                "entries": entries[offset:],
                "total": len(entries),
            }

        def _state_payload(self):
            logger = self._logger
            if not logger or not logger.state_path.exists():
                return None
            return json.loads(logger.state_path.read_text(encoding="utf-8"))

        def _screens_payload(self) -> dict[str, str]:
            logger = self._logger
            if logger and hasattr(logger, "_screens"):
                return dict(logger._screens)
            return {}

        def _speak_log_payload(self) -> dict:
            logger = self._logger
            if not logger or not logger.speak_log_path.exists():
                return {"content": "", "exists": False}
            return {
                "content": logger.speak_log_path.read_text(encoding="utf-8"),
                "exists": True,
            }

        def _api_screens(self, _query):
            self._json(self._screens_payload())

        def _api_speak_log(self, _query):
            self._json(self._speak_log_payload())

        def _api_usage(self, _query):
            usage = self._usage_payload()
            if usage is None:
                self._json({"error": "no logger"}, 404)
                return
            self._json(usage)

        def _usage_payload(self):
            logger = self._logger
            if not logger:
                return None
            registry = getattr(logger, "_registry", {})
            jsonl_map = build_jsonl_map(registry) if registry else {}
            per_player: dict[str, dict] = {}
            total = {"input_tokens": 0, "output_tokens": 0,
                     "cache_input": 0, "cache_read": 0,
                     "api_calls": 0}
            for sess, jpath in jsonl_map.items():
                u = extract_token_usage(jpath)
                pname = registry.get("players", {}).get(sess, {}).get("display_name", sess)
                per_player[pname] = u
                for k in total:
                    total[k] += u.get(k, 0)
            total["cost_estimate_usd"] = round(
                (total["input_tokens"] + total["cache_input"]) / 1_000_000 * 3
                + total["output_tokens"] / 1_000_000 * 15, 4)
            return {
                "total": total,
                "players": per_player,
                "player_count": len(per_player),
            }

        def _events_payload(self, offset: int) -> dict:
            return {
                "state": self._state_payload(),
                "log": self._log_page(offset),
                "screens": self._screens_payload(),
                "speak_log": self._speak_log_payload(),
                "usage": self._usage_payload(),
            }

        def _api_events(self, query):
            try:
                offset = max(0, int(query.get("offset", ["0"])[0]))
            except ValueError:
                self._json({"error": "offset must be an integer"}, 400)
                return
            once = query.get("once", ["0"])[0] in ("1", "true", "yes")
            interval_raw = query.get("interval", ["1"])[0]
            try:
                interval = max(0.2, float(interval_raw))
            except ValueError:
                interval = 1.0

            self.send_response(200)
            self.send_header("Content-Type", "text/event-stream; charset=utf-8")
            self.send_header("Cache-Control", "no-cache")
            self.send_header("Connection", "close" if once else "keep-alive")
            self.send_header("X-Accel-Buffering", "no")
            self.end_headers()

            while True:
                payload = self._events_payload(offset)
                offset = payload["log"]["next_offset"]
                body = (
                    "event: snapshot\n"
                    f"data: {json.dumps(payload, ensure_ascii=False)}\n\n"
                ).encode("utf-8")
                try:
                    self.wfile.write(body)
                    self.wfile.flush()
                except (BrokenPipeError, ConnectionResetError):
                    return
                if once:
                    self.close_connection = True
                    return
                time.sleep(interval)

        def log_message(self, format, *args):
            pass

    return H


def create_app(logger=None):
    """返回 (handler_class, server_factory) 用于测试"""
    return _make_handler(logger)


def start_dashboard(logger, port=_DEFAULT_PORT, host="0.0.0.0"):
    """在后台线程启动 dashboard，返回 (server, url)"""
    import threading

    handler_cls = _make_handler(logger)
    server = ThreadingHTTPServer((host, port), handler_cls)
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()
    return server, f"http://{host}:{port}"
