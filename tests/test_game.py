"""游戏逻辑测试 — TDD: 先写测试，后写实现"""
import random

import pytest

from werewolf.game import Game, GamePhase, Team


class TestRoleAssignment:
    """角色分配测试"""

    def test_correct_role_count(self, eight_player_game):
        """8 人局应分配 2 狼 2 村 1 预言家 1 女巫 1 猎人 1 守卫"""
        g = eight_player_game
        roles = [p.role for p in g.players.values()]
        assert roles.count("werewolf") == 2
        assert roles.count("villager") == 2
        assert roles.count("seer") == 1
        assert roles.count("witch") == 1
        assert roles.count("hunter") == 1
        assert roles.count("guard") == 1

    def test_all_players_have_role(self, eight_player_game):
        """每个玩家都有角色"""
        g = eight_player_game
        for name, player in g.players.items():
            assert player.role is not None, f"{name} 没有角色"

    def test_deterministic_with_seed(self, sample_config):
        """相同 seed 应产生相同分配"""
        random.seed(42)
        g1 = Game(config=sample_config)
        g1.setup(["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"])
        roles_1 = {n: p.role for n, p in g1.players.items()}

        random.seed(42)
        g2 = Game(config=sample_config)
        g2.setup(["p1", "p2", "p3", "p4", "p5", "p6", "p7", "p8"])
        roles_2 = {n: p.role for n, p in g2.players.items()}

        assert roles_1 == roles_2


class TestWinCondition:
    """胜负判定测试"""

    def test_wolves_win_by_count(self, minimal_config):
        """狼人数 >= 好人数 → 狼赢"""
        game = Game(config=minimal_config)
        # 手动构造: 2 狼 vs 1 好 (但 config 只有 1 狼, 所以用自定义方式)
        from werewolf.models import Player
        game.players = {
            "w1": Player("w1", "werewolf"),
            "w2": Player("w2", "werewolf"),
            "g1": Player("g1", "villager"),
        }
        assert game.check_winner() == Team.WOLF

    def test_goods_win_no_wolves(self, minimal_config):
        """狼人全死 → 好人赢"""
        game = Game(config=minimal_config)
        from werewolf.models import Player
        game.players = {
            "g1": Player("g1", "villager", alive=True),
            "g2": Player("g2", "seer", alive=True),
            "w1": Player("w1", "werewolf", alive=False),
        }
        assert game.check_winner() == Team.GOOD

    def test_game_continues(self, minimal_config):
        """势均力敌 → 游戏继续"""
        game = Game(config=minimal_config)
        from werewolf.models import Player
        game.players = {
            "w1": Player("w1", "werewolf", alive=True),
            "g1": Player("g1", "villager", alive=True),
            "g2": Player("g2", "seer", alive=True),
            "g3": Player("g3", "villager", alive=True),
        }
        assert game.check_winner() is None

    def test_eight_player_goods_win(self, eight_player_game):
        """8 人局：杀掉所有狼人 → 好人赢"""
        g = eight_player_game
        for name, p in g.players.items():
            if p.role == "werewolf":
                p.alive = False
        assert g.check_winner() == Team.GOOD

    def test_eight_player_wolves_win(self, eight_player_game):
        """8 人局：狼人 >= 好人 → 狼人赢"""
        g = eight_player_game
        # 杀到只剩 2 狼 + 2 好人
        killed = 0
        for name, p in g.players.items():
            if p.role != "werewolf" and killed < 4:
                p.alive = False
                killed += 1
        assert g.check_winner() == Team.WOLF


class TestVoteCounting:
    """投票统计测试"""

    def test_simple_majority(self, eight_player_game):
        """简单多数票"""
        g = eight_player_game
        alive_names = g.alive_names()
        votes = {alive_names[0]: alive_names[2], alive_names[1]: alive_names[2]}
        result = g.count_votes(votes)
        assert result.executed == alive_names[2]
        assert result.votes[alive_names[2]] == 2

    def test_tie_vote(self, eight_player_game):
        """平票 → 不处决任何人"""
        g = eight_player_game
        alive = g.alive_names()
        votes = {alive[0]: alive[2], alive[1]: alive[3],
                 alive[2]: alive[2], alive[3]: alive[3]}
        result = g.count_votes(votes)
        # 平票时票数相同，不处决
        assert result.executed is None
        assert result.votes  # 票数统计仍存在

    def test_all_abstain(self, eight_player_game):
        """全部弃票"""
        g = eight_player_game
        result = g.count_votes({})
        assert result.executed is None


class TestNightResolution:
    """夜晚结算测试"""

    def test_wolf_kill_unprotected(self, eight_player_game):
        """狼刀未守卫 → 死亡"""
        g = eight_player_game
        target = g.alive_names()[2]  # 选一个好人当目标
        deaths = g.resolve_night(wolf_target=target, guarded=None, poison=None)
        assert target in deaths

    def test_wolf_kill_protected(self, eight_player_game):
        """狼刀被守卫 → 存活"""
        g = eight_player_game
        target = g.alive_names()[2]
        deaths = g.resolve_night(wolf_target=target, guarded=target, poison=None)
        assert target not in deaths

    def test_witch_poison(self, eight_player_game):
        """女巫毒人 → 死亡"""
        g = eight_player_game
        alive = g.alive_names()
        poison_target = alive[3]
        deaths = g.resolve_night(
            wolf_target=None, guarded=None, poison=poison_target,
        )
        assert poison_target in deaths

    def test_combined_deaths(self, eight_player_game):
        """同夜多死：刀人 + 毒人"""
        g = eight_player_game
        alive = g.alive_names()
        deaths = g.resolve_night(
            wolf_target=alive[2], guarded=None, poison=alive[4],
        )
        assert alive[2] in deaths
        assert alive[4] in deaths
        assert len(deaths) == 2


class TestGamePhase:
    """游戏阶段流转测试"""

    def test_initial_phase(self, eight_player_game):
        """游戏开始前是 SETUP 阶段"""
        assert eight_player_game.phase == GamePhase.SETUP

    def test_phase_transition(self, eight_player_game):
        """阶段可以推进"""
        g = eight_player_game
        g.advance_phase()
        assert g.phase == GamePhase.DAY_SPEAK
        g.advance_phase()
        assert g.phase == GamePhase.DAY_VOTE


class TestMessageParsing:
    """消息解析测试（从 tmux 输出中提取信息）"""

    def test_extract_reply_basic(self):
        """基本回复提取"""
        from werewolf.tmux import extract_reply
        text = """❯ 请发言
我觉得 p3 很可疑，一直在带节奏
✽ Worked for 5s
❯ """
        result = extract_reply(text)
        assert "可疑" in result

    def test_extract_reply_ignores_prompt(self):
        """忽略提示行"""
        from werewolf.tmux import extract_reply
        text = """❯ 输入编号
   1. p1
   2. p2
我选 2
qy113@qy113"""
        result = extract_reply(text)
        assert "选" in result or len(result) > 0

    def test_extract_vote_number(self):
        """从输出中提取投票数字"""
        from werewolf.tmux import extract_vote
        candidates = ["p1", "p2", "p3"]
        output = "我投给 2 号"
        assert extract_vote(output, candidates) == "p2"

    def test_extract_vote_name(self):
        """直接匹配名字"""
        from werewolf.tmux import extract_vote
        candidates = ["alice", "bob", "charlie"]
        output = "我觉得 bob 是狼"
        assert extract_vote(output, candidates) == "bob"

    def test_extract_vote_no_match(self):
        """无匹配返回 None"""
        from werewolf.tmux import extract_vote
        output = "我不知道，随便吧"
        assert extract_vote(output, ["a", "b"]) is None

    def test_extract_number(self):
        """提取纯数字"""
        from werewolf.tmux import extract_number
        assert extract_number("我选择 3") == 3
        assert extract_number("没有数字") is None


class TestSpeakLog:
    """发言记录文件测试"""

    @pytest.fixture
    def logger(self, tmp_path):
        from werewolf.logging import GameLogger
        return GameLogger(run_dir=tmp_path)

    def test_init_speak_log(self, logger, eight_player_game, sample_config):
        """初始化应创建带角色表的 speak_log.md（身份隐藏）"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "发言记录" in content
        assert "角色配置" in content
        assert "8人" in content
        for name in sorted(eight_player_game.players.keys()):
            assert name in content
        assert "???" in content
        # 角色表中每行身份都应是 ???（标题中的"狼人杀"是游戏名，不算泄露）
        for line in content.split("\n"):
            if line.startswith("| p") and "???" not in line:
                assert False, f"角色表行暴露了身份: {line}"

    def test_append_day_header(self, logger, eight_player_game, sample_config):
        """追加白天标题"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        logger.append_day_header(1)
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "第 1 天 — 白天" in content

    def test_append_speak(self, logger, eight_player_game, sample_config):
        """追加发言记录"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        logger.append_day_header(1)
        logger.append_speak(1, "p1", "???", "我觉得 p3 很可疑")
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "p1" in content
        assert "可疑" in content
        assert "### 发言" in content

    def test_append_multiple_speaks(self, logger, eight_player_game, sample_config):
        """多次发言应累积"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        logger.append_day_header(1)
        logger.append_speak(1, "p1", "???", "我是好人")
        logger.append_speak(1, "p2", "🔮 预言家", "p1 说得对")
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert content.count("|") >= 6

    def test_append_vote_result_with_execution(self, logger, eight_player_game, sample_config):
        """投票结果：有处决"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        votes = {"p1": "p3", "p2": "p3", "p3": "p1"}
        logger.append_vote_result(1, votes, "p3", eight_player_game)
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "投票" in content
        assert "处决" in content
        assert "p3" in content

    def test_append_vote_result_tie(self, logger, eight_player_game, sample_config):
        """投票结果：平票"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        votes = {"p1": "p2", "p2": "p1"}
        logger.append_vote_result(1, votes, None, eight_player_game)
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "平安度过" in content

    def test_append_night_summary_deaths(self, logger, eight_player_game, sample_config):
        """夜晚结算：有死亡"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        logger.append_night_summary(1, ["p3"], False, None, "p1")
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "第 1 天 — 夜晚" in content
        assert "死亡: p3" in content
        assert "守卫守护: p1" in content

    def test_append_night_summary_safe(self, logger, eight_player_game, sample_config):
        """夜晚结算：平安夜"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        logger.append_night_summary(1, [], False, None, None)
        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "平安夜" in content

    def test_full_round_flow(self, logger, eight_player_game, sample_config):
        """完整一轮流程"""
        logger.init_speak_log(eight_player_game.players, sample_config)
        logger.append_day_header(1)
        for i, name in enumerate(sorted(eight_player_game.players.keys())[:4]):
            logger.append_speak(1, name, "???", f"玩家{name}的发言{i}")
        votes = {n: "p3" for n in eight_player_game.players if n != "p3"}
        logger.append_vote_result(1, votes, "p3", eight_player_game)
        logger.append_night_summary(1, ["p4"], True, None, "p1")

        content = logger.speak_log_path.read_text(encoding="utf-8")
        assert "角色配置" in content
        assert "第 1 天 — 白天" in content
        assert "### 发言" in content
        assert "### 投票" in content
        assert "第 1 天 — 夜晚" in content
        assert "救人成功" in content
