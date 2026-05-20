"""游戏引擎 — 纯逻辑，无 I/O 依赖"""
import random
from typing import Optional

from werewolf.models import GamePhase, Player, Team, VoteResult


class Game:
    def __init__(self, config: dict | None = None):
        self.config = config or {}
        self.players: dict[str, Player] = {}
        self.phase = GamePhase.SETUP
        self.round_num = 0
        self._witch_save_used = False
        self._witch_poison_used = False
        self._last_guarded: str | None = None

    def setup(self, player_names: list[str], seed: int | None = None):
        """初始化游戏：分配角色"""
        if seed is not None:
            random.seed(seed)

        roles_cfg = self.config.get("roles", {})
        role_pool = []
        for role, info in roles_cfg.items():
            role_pool.extend([role] * info["count"])

        random.shuffle(role_pool)
        names = list(player_names)
        random.shuffle(names)

        for i, name in enumerate(names):
            role = role_pool[i % len(role_pool)]
            self.players[name] = Player(name=name, role=role)

    # ── 查询 ──

    def alive_players(self) -> list[Player]:
        return [p for p in self.players.values() if p.alive]

    def alive_names(self) -> list[str]:
        return [p.name for p in self.alive_players()]

    def get_player(self, name: str) -> Player | None:
        return self.players.get(name)

    def players_by_role(self, role: str) -> list[Player]:
        return [p for p in self.alive_players() if p.role == role]

    # ── 胜负判定 ──

    def check_winner(self) -> Team | None:
        alive = self.alive_players()
        wolves = [p for p in alive if p.role == "werewolf"]
        goods = [p for p in alive if p.role != "werewolf"]

        if not wolves:
            return Team.GOOD
        if len(wolves) >= len(goods):
            return Team.WOLF
        if len(alive) <= 2:
            return Team.GOOD  # 存活太少，好人判胜（简化规则）
        return None

    def is_over(self) -> bool:
        return self.check_winner() is not None or self.phase == GamePhase.ENDED

    # ── 投票 ──

    def count_votes(self, votes: dict[str, str | None]) -> VoteResult:
        """
        votes: {voter_name: target_name_or_None}
        返回 VoteResult
        """
        counts: dict[str, int] = {}
        for voter, target in votes.items():
            if target is not None and target in self.players:
                counts[target] = counts.get(target, 0) + 1

        if not counts:
            return VoteResult(executed=None, votes={})

        max_votes = max(counts.values())
        top_targets = [t for t, c in counts.items() if c == max_votes]

        # 平票不处决；唯一最高票处决
        executed = top_targets[0] if len(top_targets) == 1 else None

        return VoteResult(executed=executed, votes=counts)

    def execute_vote(self, result: VoteResult) -> list[str]:
        """执行投票结果，返回死亡名单"""
        dead = []
        if result.executed and result.executed in self.players:
            player = self.players[result.executed]
            player.alive = False
            dead.append(result.executed)

            # 猎人技能
            if player.role == "hunter":
                # 猎人开枪在 controller 层处理，这里只标记
                pass
        return dead

    # ── 夜晚结算 ──

    def resolve_night(
        self,
        wolf_target: str | None,
        guarded: str | None,
        poison: str | None,
    ) -> list[str]:
        """结算夜晚，返回死亡名单"""
        deaths = []

        # 狼刀
        if wolf_target and wolf_target in self.players:
            p = self.players[wolf_target]
            if p.alive and wolf_target != guarded:
                p.alive = False
                deaths.append(wolf_target)

        # 女巫毒
        if poison and poison in self.players:
            p = self.players[poison]
            if p.alive:
                p.alive = False
                if poison not in deaths:
                    deaths.append(poison)

        return deaths

    # ── 阶段流转 ──

    def advance_phase(self):
        phase_order = [
            GamePhase.SETUP,
            GamePhase.DAY_SPEAK,
            GamePhase.DAY_VOTE,
            GamePhase.NIGHT,
        ]
        current_idx = phase_order.index(self.phase)
        next_idx = (current_idx + 1) % len(phase_order)
        self.phase = phase_order[next_idx]
        if self.phase == GamePhase.DAY_SPEAK:
            self.round_num += 1

    # ── 角色信息 ──

    def role_display(self, role: str) -> str:
        roles_cfg = self.config.get("roles", {})
        info = roles_cfg.get(role, {})
        emoji = info.get("emoji", "?")
        name_cn = info.get("name_cn", role)
        return f"{emoji} {name_cn}"

    def team_of(self, role: str) -> Team:
        roles_cfg = self.config.get("roles", {})
        info = roles_cfg.get(role, {})
        return Team.WOLF if info.get("team") == "wolf" else Team.GOOD

    # ── 游戏状态序列化 ──

    def to_dict(self) -> dict:
        return {
            "phase": self.phase.value,
            "round": self.round_num,
            "players": {
                name: {"role": p.role, "alive": p.alive}
                for name, p in self.players.items()
            },
            "witch_save_used": self._witch_save_used,
            "witch_poison_used": self._witch_poison_used,
            "last_guarded": self._last_guarded,
        }
