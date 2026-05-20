"""数据模型"""
from dataclasses import dataclass, field
from enum import Enum


class Team(Enum):
    GOOD = "good"
    WOLF = "wolf"


class GamePhase(Enum):
    SETUP = "setup"
    DAY_SPEAK = "day_speak"
    DAY_VOTE = "day_vote"
    NIGHT = "night"
    ENDED = "ended"


@dataclass
class Player:
    name: str
    role: str | None = None
    alive: bool = True

    @property
    def team(self) -> Team | None:
        if self.role is None:
            return None
        # team mapping is in config, but we can infer common roles
        wolf_roles = {"werewolf"}
        return Team.WOLF if self.role in wolf_roles else Team.GOOD


@dataclass
class VoteResult:
    executed: str | None = None
    votes: dict[str, int] = field(default_factory=dict)
