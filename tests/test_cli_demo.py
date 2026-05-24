"""CLI demo-mode tests."""
import copy


def test_build_demo_config_shortens_waits(sample_config):
    from werewolf.cli import build_demo_config

    original = copy.deepcopy(sample_config)
    demo = build_demo_config(sample_config)

    assert sample_config == original
    assert demo["rules"]["warmup_sec"] == 1
    assert demo["rules"]["speak_timeout_sec"] <= 15
    assert demo["rules"]["min_speak_wait_sec"] == 0
    assert demo["rules"]["speak_poll_interval_sec"] <= 6
    assert demo["rules"]["vote_timeout_sec"] <= 10
    assert demo["rules"]["night_action_timeout_sec"] <= 10


def test_reset_registry_for_demo_revives_players():
    from werewolf.cli import reset_registry_for_demo

    registry = {
        "players": {
            "ww-1": {"display_name": "player-1", "alive": False, "role": "guard"},
            "ww-2": {"display_name": "player-2", "alive": True, "role": "werewolf"},
        },
        "wolf_kill_history": ["player-1"],
        "seer_check_history": {"player-2": "狼"},
        "witch_save_used": True,
        "witch_poison_used": True,
        "last_guarded": "ww-1",
    }

    demo = reset_registry_for_demo(registry)

    assert registry["players"]["ww-1"]["alive"] is False
    for pdata in demo["players"].values():
        assert pdata["alive"] is True
        assert pdata["role"] is None
    assert "wolf_kill_history" not in demo
    assert "seer_check_history" not in demo
    assert "witch_save_used" not in demo
    assert "witch_poison_used" not in demo
    assert "last_guarded" not in demo
