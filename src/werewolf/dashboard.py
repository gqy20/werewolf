"""Dashboard HTTP 服务 — 实时监控狼人杀对局"""
import json
from http.server import HTTPServer, SimpleHTTPRequestHandler
from pathlib import Path
from urllib.parse import urlparse, parse_qs

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

        def _api_screens(self, _query):
            screens: dict[str, str] = {}
            logger = self._logger
            if logger and hasattr(logger, "_screens"):
                screens = dict(logger._screens)
            self._json(screens)

        def _api_speak_log(self, _query):
            logger = self._logger
            if not logger or not logger.speak_log_path.exists():
                self._json({"content": "", "exists": False})
                return
            content = logger.speak_log_path.read_text(encoding="utf-8")
            self._json({"content": content, "exists": True})

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
    server = HTTPServer((host, port), handler_cls)
    t = threading.Thread(target=server.serve_forever, daemon=True)
    t.start()
    return server, f"http://{host}:{port}"
