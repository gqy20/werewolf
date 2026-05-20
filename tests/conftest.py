"""pytest fixtures"""
import json
import os
from pathlib import Path

import pytest

BASE_DIR = Path(__file__).parent.parent
CONFIG_PATH = BASE_DIR / "config.json"


@pytest.fixture
def sample_config():
    """8 人局标准配置"""
    return {
        "total_players": 8,
        "roles": {
            "werewolf": {"count": 2, "team": "wolf", "emoji": "🐺", "name_cn": "狼人"},
            "villager": {"count": 2, "team": "good", "emoji": "🙍‍⅌️", "name_cn": "村民"},
            "seer": {"count": 1, "team": "good", "emoji": "🔮", "name_cn": "预言家"},
            "witch": {"count": 1, "team": "good", "emoji": "🦨️", "name_cn": "女巫"},
            "hunter": {"count": 1, "team": "good", "emoji": "🐫️", "name_cn": "猎人"},
            "guard": {"count": 1, "team": "good", "emoji": "🛡️", "name_cn": "守卫"},
        },
        "rules": {
            "speak_timeout_sec": 60,
            "vote_timeout_sec": 45,
            "night_action_timeout_sec": 40,
            "witch_save_count": 1,
            "witch_poison_count": 1,
            "hunter_can_shoot_when_dead": True,
        },
    }


@pytest.fixture
def eight_player_game(sample_config):
    """一个已分配好角色的 8 人游戏状态"""
    from werewolf.game import Game

    game = Game(config=sample_config)
    game.setup(["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"])
    return game


@pytest.fixture
def minimal_config():
    """最小 4 人测试配置（用于快速测试）"""
    return {
        "total_players": 4,
        "roles": {
            "werewolf": {"count": 1, "team": "wolf", "emoji": "🐺", "name_cn": "狼人"},
            "villager": {"count": 2, "team": "good", "emoji": "🙍‍⅌️", "name_cn": "村民"},
            "seer": {"count": 1, "team": "good", "emoji": "🔮", "name_cn": "预言家"},
        },
        "rules": {
            "speak_timeout_sec": 5,
            "vote_timeout_sec": 5,
            "night_action_timeout_sec": 5,
            "witch_save_count": 0,
            "witch_poison_count": 0,
            "hunter_can_shoot_when_dead": False,
        },
    }
