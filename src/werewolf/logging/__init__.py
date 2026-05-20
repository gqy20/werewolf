"""游戏日志 + 状态持久化"""
from __future__ import annotations

import json
import os
import time
from datetime import datetime, timezone
from pathlib import Path

from werewolf.config import _BASE_DIR


class GameLogger:
    """按局隔离的日志系统"""

    def __init__(self, run_dir: Path | None = None):
        self.run_dir = run_dir or (_BASE_DIR / "data" / "runs" / _ts())
        self.run_dir.mkdir(parents=True, exist_ok=True)
        self.state_path = self.run_dir / "game_state.json"
        self.log_path = self.run_dir / "game_log.jsonl"
        self._log_file: list[dict] | None = []
        self._state: dict = {}

    # ── 状态快照（每阶段覆盖写入）─────────────────

    def save_state(self, game) -> None:
        """保存当前游戏状态到 JSON，方便外部 cat 查看"""
        players_state = {}
        for name, player in game.players.items():
            players_state[name] = {
                "role": player.role,
                "alive": player.alive,
            }
        self._state = {
            "run_id": self.run_dir.name,
            "ts": _now_iso(),
            "round": game.round_num,
            "phase": game.phase.value,
            "alive": [n for n, p in game.players.items() if p.alive],
            "dead": [n for n, p in game.players.items() if not p.alive],
            "roles": players_state,
            "witch_save_used": getattr(game, "_witch_save_used", False),
            "witch_poison_used": getattr(game, "_witch_poison_used", False),
            "last_guarded": getattr(game, "_last_guarded", None),
        }
        self.state_path.write_text(json.dumps(self._state, ensure_ascii=False, indent=2))

    # ── 事件日志（只追加，不可变）─────────────────────

    def log(self, event: str, **data) -> None:
        """追加一条事件到 jsonl 日志"""
        entry = {
            "ts": _now_iso(),
            "round": self._state.get("round", "?"),
            "phase": self._state.get("phase", "?"),
            "event": event,
            **data,
        }
        with open(self.log_path, "a") as f:
            f.write(json.dumps(entry, ensure_ascii=False) + "\n")

    # ── 结算报告 ────────────────────────────────────────────

    def write_report(self, game) -> Path:
        """游戏结束时生成 markdown 报告"""
        report = self._build_report(game)
        path = self.run_dir / "final_report.md"
        path.write_text(report)
        return path

    def _build_report(self, game) -> str:
        lines = [
            f"# 🐺🌙 狼人杀 #{self._state.get('run_id', '?')}",
            f"\n**时间**: {self._state.get('ts', '?')} ~ {_now_iso()}",
            f"**结果**: {self._winner_label(game)}",
            "",
            "## 身份揭晓",
            "",
            "| 玩家 | 角色 | 存活 | 死因 |",
            "|------|------|:--:|:--:|",
        ]
        death_reasons = getattr(self, "_death_reasons", {})

        for name, player in game.players.items():
            status = "✅" if player.alive else "💀"
            reason = death_reasons.get(name, "")
            role_disp = game.role_display(player.role)
            lines.append(f"| {name} | {role_disp} | {status} | {reason} |")

        lines += [
            "",
            f"## Day {game.round_num} 时间线",
            "",
        ]

        # 从 log 重建时间线
        events = self._read_log()
        by_round: dict[str, list[dict]] = {}
        for e in events:
            r = e.get("round", "?")
            by_round.setdefault(r, []).append(e)

        for rnd in sorted(by_round):
            lines.append(f"### Round {rnd}")
            lines.append("")
            for e in by_round[rnd]:
                evt = e.get("event", "?")
                ts = e.get("ts", "")[11:16]
                if evt == "speak":
                    lines.append(f"- **{ts}** `{e.get('player','?')}` 发言: "
                               f"{e.get('msg','')[:80]}")
                elif evt == "vote":
                    lines.append(f"- **{ts}** `{e.get('voter','?')}` → "
                               f"`{e.get('target','?')}`")
                elif evt == "vote_result":
                    exec_type = e.get("executed", "平票/弃票" if not e.get("executed")
                                else f"`{e['executed']}` 被处决")
                    lines.append(f"- **{ts}** 投票结果: {exec_type}")
                elif evt == "night_resolve":
                    deaths = e.get("deaths", [])
                    saved = e.get("saved", False)
                    poisoned = e.get("poisoned", "")
                    guarded = e.get("guarded", "")
                    parts = []
                    if deaths:
                        parts.append(f"💀 {', '.join(deaths)}")
                    if saved:
                        parts.append("🦨️ 救人成功")
                    if poisoned:
                        parts.append(f"☠️ 毒 {poisoned}")
                    if guarded:
                        parts.append(f"🛡️ 守卫守护 {guarded}")
                    lines.append(f"- **{ts}** 夜晚结算: {'; '.join(parts) or '平安夜'}")
                elif evt == "seer_check":
                    lines.append(f"- **{ts}** 🔮查验 `{e.get('target','?')}` → "
                               f"`{e.get('result','?')}`")
                elif evt == "witch_action":
                    act = e.get("action", "?")
                    lines.append(f"- **{ts}** 🦨️女巫: {act}")
                elif evt == "guard_action":
                    target = e.get("target", "?")
                    lines.append(f"- **{ts}** 🛡️守卫: 守{target}" if target else
                               f"- **{ts}** 🛡️守卫: 不守")
                elif evt == "hunter_shoot":
                    lines.append(f"- **{ts}** 🔫猎人 `{e.get('hunter','?')}` 开枪带走了 "
                               f"`{e.get('target','?')}`")
                elif evt == "game_end":
                    lines.append(f"- **{ts}** **{e.get('result','?')}**")

        return "\n".join(lines)

    def _winner_label(self, game) -> str:
        w = game.check_winner()
        if w is None:
            return "🎮 进行中..."
        from werewolf.models import Team
        return "🐺 狼人胜利！" if w == Team.WOLF else "👼 好人阵营胜利！"

    def _read_log(self) -> list[dict]:
        if not self.log_path.exists():
            return []
        entries = []
        with open(self.log_path) as f:
            for line in f:
                line = line.strip()
                if line:
                    try:
                        entries.append(json.loads(line))
                    except json.JSONDecodeError:
                        pass
        return entries


def _ts() -> str:
    return datetime.now(timezone.utc).strftime("%H:%M:%S")


def _now_iso() -> str:
    return datetime.now(timezone.utc).isoformat(timespec="seconds")


def _ts_dir() -> str:
    return datetime.now(timezone.utc).strftime("%Y%m%d_%H%M%S")
