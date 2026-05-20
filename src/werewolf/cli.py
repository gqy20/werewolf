"""CLI 入口 — Game Master 控制台"""
import json
import sys
import time
from pathlib import Path

from werewolf.config import load_config, load_registry, save_registry
from werewolf.game import Game, GamePhase
from werewolf.tmux import (
    discover_task_uuid,
    extract_number,
    extract_reply,
    extract_vote,
    kill_session,
    list_sessions,
    session_exists,
    tmux_capture,
    tmux_send,
)

BASE_DIR = Path(__file__).resolve().parents[2]


def cmd_bootstrap(n: int = 8):
    cfg = load_config()
    prefix = "ww-"
    claude_cmd = "claude --dangerously-skip-permissions --name"
    project_dir = str(BASE_DIR.parent)

    print(f"🎮 启动 {n} 个 Claude Code 实例...")
    registry = {"players": {}, "config": {"total": n}}

    for i in range(1, n + 1):
        name = f"{prefix}{i}"
        if session_exists(name):
            print(f"  ⚠️  {name} 已存在，跳过")
            continue
        print(f"  [{i}/{n}] {name} ...")
        import subprocess
        subprocess.run(
            ["tmux", "new-session", "-d", "-s", name, "-c", project_dir,
             f"{claude_cmd} player-{i}"],
            capture_output=True,
        )

    print("⏳ 等待初始化...")
    time.sleep(12)

    # 收集所有活跃的 session 名
    active = [f"{prefix}{i}" for i in range(1, n + 1) if session_exists(f"{prefix}{i}")]

    print(f"🔍 并行发现 {len(active)} 个 Task UUID ...")
    from werewolf.tmux import discover_all_uuids
    uuids = discover_all_uuids(active)

    for i, name in enumerate(active):
        uuid = uuids.get(name, "")
        if uuid:
            registry["players"][name] = {
                "display_name": f"player-{i+1}",
                "task_uuid": uuid,
                "alive": True,
                "role": None,
            }
            print(f"  ✅ {name}: {uuid[:8]}")
        else:
            print(f"  ❌ {name}: 超时")

    save_registry(registry)
    found = len(registry["players"])
    print(f"\n📋 {found}/{n} 就绪")


def _broadcast(targets: list[str], msg: str):
    for t in targets:
        tmux_send(t, msg)


def cmd_run():
    registry = load_registry()
    players_raw = registry.get("players", {})
    if not players_raw:
        print("❌ 没有注册的实例。先运行: python -m werewolf bootstrap")
        sys.exit(1)

    cfg = load_config()
    rules = cfg.get("rules", {})
    game = Game(config=cfg)

    # 从 registry 构建玩家列表
    names = [p["display_name"] for p in players_raw.values()]
    # 用固定 seed 保证可复现（实际中可用 random）
    game.setup(names, seed=42)

    # 建立 session ↔ display_name 映射
    name_to_session: dict[str, str] = {}
    session_to_name: dict[str, str] = {}
    for sess, pdata in players_raw.items():
        dname = pdata["display_name"]
        name_to_session[dname] = sess
        session_to_name[sess] = dname
        # 同步角色到 registry
        player = game.get_player(dname)
        if player:
            pdata["role"] = player.role

    alive_sessions = [s for s in players_raw if players_raw[s].get("alive", True)]
    n = len(alive_sessions)

    print(f"\n{'='*50}")
    print(f"🐺 🌙 狼人杀开始！{n} 人局")
    print(f"{'='*50}")

    # === 角色分配 ===
    print("\n🎲 分配角色...")
    for dname, player in game.players.items():
        sess = name_to_session[dname]
        role_disp = game.role_display(player.role)
        team_str = "🐺 狼人阵营" if player.team.name == "wolf" else "👼 好人阵营"

        extra = ""
        if player.role == "werewolf":
            mates = [g.name for g in game.players_by_role("werewolf") if g.name != dname]
            extra = f"\n🤝 同伴: {', '.join(mates)}"

        card = (
            f"\n{'='*40}\n"
            f"🎭 身份: {role_disp}\n"
            f"🏷️ 阵营: {team_str}{extra}\n"
            f"{'='*40}\n"
            f"记住身份，不要泄露。回复'收到'。"
        )
        tmux_send(sess, card)
        print(f"  {dname}: {role_disp}")

    save_registry(registry)
    time.sleep(rules.get("speak_timeout_sec", 60))

    # 公布配置摘要
    role_counts = {}
    for p in game.players.values():
        role_counts[p.role] = role_counts.get(p.role, 0) + 1
    summary = ", ".join(f"{game.role_display(r)}×{c}" for r, c in role_counts.items())
    _broadcast(alive_sessions, f"\n📢 本局配置: {summary}")

    # === 主循环 ===
    max_rounds = 20
    round_num = 1

    while round_num <= max_rounds:
        alive = [s for s in players_raw if players_raw[s].get("alive", True)]
        alive_names_list = [session_to_name[s] for s in alive]

        # 胜负判定
        winner = game.check_winner()
        if winner:
            team_str = "🐺 狼人" if winner.name == "wolf" else "👼 好人"
            _broadcast(alive, f"\n🎉 {team_str}阵营胜利！")
            break
        if len(alive) <= 2:
            _broadcast(alive, "\n⚖️ 游戏结束，存活不足。")
            break

        # ── 白天 ──
        print(f"\n{'━'*50}")
        print(f"☀️ 第 {round_num} 天 — 白天")
        print(f"{'━'*50}")

        _broadcast(alive, f"\n{'='*45}\n☀️ 第 {round_num} 天 — 天亮了\n{'='*45}")

        import random as _rnd
        _rnd.shuffle(alive)

        speak_timeout = rules.get("speak_timeout_sec", 60)
        vote_timeout = rules.get("vote_timeout_sec", 45)

        for sess in alive:
            pname = session_to_name[sess]
            player = game.get_player(pname)
            rdisp = game.role_display(player.role) if player and _rnd.random() < 0.3 else "???"

            others = [session_to_name[s] for s in alive if s != sess]
            tmux_send(sess,
                      f"\n🗣️ 轮到你 ({pname})\n"
                      f"   存活: {', '.join(others)}\n"
                      f"   发表看法（简短）:")
            time.sleep(speak_timeout)

            out = tmux_capture(sess, 15)
            reply = extract_reply(out)
            target_others = [s for s in alive if s != sess]
            if reply:
                for t in target_others:
                    tmux_send(t, f"💬 {pname}({rdisp}): {reply}")
                print(f"  💬 {pname}: {reply[:60]}")
            else:
                for t in target_others:
                    tmux_send(t, f"💬 {pname}: （沉默）")

        # 投票（并行：同时发 → 统一等 → 批量收）
        print(f"  🗳️ 投票...")
        options = [(i+1, session_to_name[s]) for i, s in enumerate(alive)]
        opts_str = "\n".join(f"   {i}. {n}" for i, n in options)

        _broadcast(alive, f"\n🗳️ 投票！选择处决对象:\n{opts_str}\n\n你的投票:")
        time.sleep(vote_timeout)

        votes_raw: dict[str, str | None] = {}
        candidates = [session_to_name[s] for s in alive]
        for sess in alive:
            pname = session_to_name[sess]
            out = tmux_capture(sess, 10)
            pick = extract_vote(out, candidates)
            votes_raw[pname] = pick
            if pick:
                print(f"    {pname} → {pick}")
            else:
                print(f"    {pname} → 弃票")

        result = game.count_votes(votes_raw)
        dead = game.execute_vote(result)

        if dead:
            dname = dead[0] if dead else None
            if dname:
                dsess = name_to_session[dname]
                drole = game.get_player(dname).role if game.get_player(dname) else "?"
                _broadcast(alive, f"💀 {dname} 被处决！身份: {game.role_display(drole)}")
                players_raw[dsess]["alive"] = False

                # 猎人开枪
                if drole == "hunter" and rules.get("hunter_can_shoot_when_dead"):
                    htargets = [s for s in alive if s != dsess]
                    if htargets:
                        hopts = "\n".join(f"   {i+1}. {session_to_name[s]}"
                                           for i, s in enumerate(htargets))
                        tmux_send(dsess, f"🐫 开枪！\n{hopts}\n输入编号(0=不开):")
                        time.sleep(30)
                        hout = tmux_capture(dsess, 10)
                        hpick = extract_vote(hout,
                            [session_to_name[s] for s in htargets] + ["skip"])
                        if hpick and hpick != "skip":
                            hsess = name_to_session.get(hpick)
                            if hsess and hsess in players_raw:
                                players_raw[hsess]["alive"] = False
                                hr = game.get_player(hpick).role if game.get_player(hpick) else "?"
                                _broadcast(alive,
                                    f"🔫 猎人带走了 {hpick}({game.role_display(hr)})!")
        else:
            _broadcast(alive, "\n📊 平票/弃票，平安度过。")

        save_registry(registry)

        # ── 夜晚 ──
        alive = [s for s in players_raw if players_raw[s].get("alive", True)]
        winner = game.check_winner()
        if winner:
            break

        print(f"\n{'━'*50}")
        print(f"🌙 第 {round_num} 天 — 夜晚")
        print(f"{'━'*50}")

        _broadcast(alive, f"\n{'='*45}\n🌙 夜幕降临，请闭眼\n{'='*45}")

        night_timeout = rules.get("night_action_timeout_sec", 40)
        witch_save_used = registry.get("witch_save_used", False)
        witch_poison_used = registry.get("witch_poison_used", False)

        # ═══ 第一波并行：狼人 + 守卫 + 预言家（三者无信息依赖）════

        # --- 狼人 ---
        wolves = [s for s in alive if players_raw[s].get("role") == "werewolf"]
        wolf_target_name = None
        _wolf_candidates: list[str] = []
        if wolves:
            goods = [s for s in alive if players_raw[s].get("role") != "werewolf"]
            _wolf_candidates = [session_to_name[s] for s in goods]
            gopts = "\n".join(f"   {i+1}. {n}" for i, n in enumerate(_wolf_candidates))
            for w in wolves:
                tmux_send(w, f"🐺 刀谁？\n{gopts}\n输入编号:")

        # --- 守卫 ---
        guards = [s for s in alive if players_raw[s].get("role") == "guard"]
        last_guarded = registry.get("last_guarded")
        guarded_tonight = None
        _guard_candidates: list[str] = []
        if guards:
            guard = guards[0]
            guardables = [s for s in alive if s != guard and s != last_guarded]
            _guard_candidates = [session_to_name[s] for s in guardables]
            gopts = "\n".join(f"   {i+1}. {n}" for i, n in enumerate(_guard_candidates))
            hint = "(不能连续守同一人)" if last_guarded else ""
            tmux_send(guard, f"🛡️ 守护谁？{hint}\n{gopts}\n输入(0=不守):")

        # --- 预言家 ---
        seers = [s for s in alive if players_raw[s].get("role") == "seer"]
        _seer_candidates: list[str] = []
        for seer in seers:
            others = [s for s in alive if s != seer]
            _seer_candidates = [session_to_name[s] for s in others]
            oopts = "\n".join(f"   {i+1}. {n}" for i, n in enumerate(_seer_candidates))
            tmux_send(seer, f"🔮 查验谁？\n{oopts}\n输入:")

        # 统一等待第一波完成
        print(f"  🌙 第一波行动(狼人+守卫+预言家)...")
        time.sleep(night_timeout)

        # 收集狼人结果
        if wolves:
            for w in wolves:
                out = tmux_capture(w, 10)
                pick = extract_vote(out, _wolf_candidates)
                if pick:
                    wolf_target_name = pick
                    print(f"  🐺 → {pick}")
                    break

        # 收集守卫结果
        if guards:
            out = tmux_capture(guards[0], 10)
            pick = extract_vote(out, _guard_candidates + ["skip"])
            if pick and pick != "skip":
                gsess = name_to_session.get(pick)
                if gsess:
                    guarded_tonight = gsess
                    print(f"  🛡️ 守 {pick}")

        # 收集预言家结果 & 私发查验反馈
        for seer in seers:
            out = tmux_capture(seer, 10)
            pick = extract_vote(out, _seer_candidates)
            if pick:
                target_sess = name_to_session.get(pick)
                if target_sess:
                    trole = players_raw[target_sess].get("role", "?")
                    tteam = game.team_of(trole)
                    result_str = "🐺 狼人！" if tteam.name == "wolf" else "👼 好人"
                    tmux_send(seer, f"🔮 {pick} → {result_str}")
                    print(f"  🔮 {pick} → {result_str}")

        # ═══ 第二波：女巫（依赖狼人刀人结果）════

        witches = [s for s in alive if players_raw[s].get("role") == "witch"]
        poison_target = None
        wolf_saved = False

        for witch in witches:
            actions = []
            if not witch_save_used and wolf_target_name:
                actions.append("救人")
            if not witch_poison_used:
                actions.append("毒人")
            actions.append("不用")
            act_str = ", ".join(f"{i+1}.{a}" for i, a in enumerate(actions))
            msg = f"🦨️ 女巫睁眼\n"
            if wolf_target_name:
                msg += f"今晚 {wolf_target_name} 被袭\n"
            msg += f"解药{'❌' if witch_save_used else '✅'} | 毒药{'❌' if witch_poison_used else '✅'}\n"
            msg += f"{act_str}\n输入:"
            tmux_send(witch, msg)

        print(f"  🌙 第二波行动(女巫)...")
        time.sleep(night_timeout)

        # 收集女巫结果
        for witch in witches:
            out = tmux_capture(witch, 10)
            choice = extract_number(out)
            if choice is not None and 0 < choice <= len(actions):
                act = actions[choice - 1]
                if act == "救人":
                    witch_save_used = True
                    wolf_saved = True
                    print(f"  🦨️ 救人")
                elif act == "毒人":
                    witch_poison_used = True
                    poisons = [s for s in alive if s != witch]
                    pnames = [session_to_name[s] for s in poisons]
                    popts = "\n".join(f"   {i+1}. {n}" for i, n in enumerate(pnames))
                    tmux_send(witch, f"毒谁？\n{popts}\n输入:")
                    time.sleep(night_timeout)
                    pout = tmux_capture(witch, 10)
                    ppick = extract_vote(pout, pnames)
                    if ppick:
                        psess = next((s for s in players_raw
                                      if session_to_name.get(s) == ppick), None)
                        if psess:
                            poison_target = psess
                            print(f"  ☠️ 毒 {ppick}")

        # 结算夜晚
        wolf_sess = name_to_session.get(wolf_target_name) if wolf_target_name else None
        if not wolf_saved and wolf_sess:
            deaths = game.resolve_night(
                wolf_target=session_to_name.get(wolf_sess, wolf_target_name),
                guarded=session_to_name.get(guarded_tonight, guarded_tonight),
                poison=session_to_name.get(poison_target),
            )
        else:
            deaths = game.resolve_night(
                wolf_target=None,
                guarded=None,
                poison=session_to_name.get(poison_target),
            )

        registry["last_guarded"] = guarded_tonight
        save_registry(registry)

        # 死讯
        alive = [s for s in players_raw if players_raw[s].get("alive", True)]
        if deaths:
            dinfo = []
            for d in deaths:
                dsess = name_to_session.get(d)
                if dsess and dsess in players_raw:
                    players_raw[dsess]["alive"] = False
                    dr = players_raw[dsess].get("role", "?")
                    dinfo.append(f"{d}({game.role_display(dr)})")
                    tmux_send(dsess, "你已死亡，留遗言:")
                    time.sleep(20)
            _broadcast(alive, f"\n☠️ 昨晚死亡: {', '.join(dinfo)}")
        else:
            _broadcast(alive, "\n🌅 平安夜。")

        round_num += 1

    # 结束
    print(f"\n{'='*50}")
    print("🏁 游戏结束！")
    print(f"{'='*50}")
    for sess, pdata in sorted(players_raw.items()):
        role = pdata.get("role", "?")
        status = "✅" if pdata.get("alive") else "💀"
        rd = game.role_display(role)
        print(f"  {pdata['display_name']:12s} {rd:20s} {status}")
    save_registry(registry)


def cmd_status():
    registry = load_registry()
    players = registry.get("players", {})
    if not players:
        print("没有注册的实例")
        return
    print(f"\n{'='*55}")
    print(f"📋 实例状态")
    print(f"{'='*55}")
    for sess, pdata in sorted(players.items()):
        name = pdata.get("display_name", "?")
        alive = "🟢" if pdata.get("alive") else "💀"
        role = pdata.get("role") or "—"
        uuid = (pdata.get("task_uuid") or "?")[:10]
        ok = "✅" if session_exists(sess) else "❌"
        print(f"  {ok} {sess:<14s} {name:<12s} {alive} {role:<10s} {uuid}")


def cmd_kill():
    sessions = list_sessions("ww-")
    # 也清理可能带数字前缀的残留
    if not sessions:
        import subprocess as _sp
        r = _sp.run(["tmux", "list-sessions", "-F", "#{session_name}"],
                    capture_output=True, text=True)
        sessions = [s for s in r.stdout.strip().split("\n") if "ww-" in s]
    if not sessions:
        print("没有狼人杀实例")
        return
    for s in sessions:
        kill_session(s)
        print(f"  终止: {s}")
    reg_path = __import__("werewolf.config", fromlist=["REGISTRY_PATH"]).REGISTRY_PATH
    if reg_path.exists():
        reg_path.unlink()


def cmd_send(name: str, msg: str):
    registry = load_registry()
    target = None
    for sess, pdata in registry.get("players", {}).items():
        if sess == name or pdata.get("display_name") == name:
            target = sess
            break
    if not target and session_exists(name):
        target = name
    if not target:
        print(f"找不到: {name}")
        return
    tmux_send(target, msg)
    print(f"✅ → {target}: {msg}")


def main():
    args = sys.argv[1:]
    if not args or args[0] in ("-h", "--help"):
        print(__doc__)
        return

    cmd = args[0]
    if cmd == "bootstrap":
        n = int(args[1]) if len(args) > 1 else 8
        cmd_bootstrap(n)
    elif cmd == "run":
        cmd_run()
    elif cmd == "status":
        cmd_status()
    elif cmd == "kill":
        cmd_kill()
    elif cmd == "send":
        if len(args) < 3:
            print("用法: python -m werewolf send <name> <msg>")
            return
        cmd_send(args[1], " ".join(args[2:]))
    else:
        print(f"未知命令: {cmd}")


__doc__ = """werewolf — Claude Code 狼人杀 Game Master

用法:
  python -m werewolf bootstrap [N]   启动 N 个实例（默认 8）
  python -m werewolf run             开始游戏
  python -m werewolf status          查看状态
  python -m werewolf kill            终止所有实例
  python -m werewolf send <n> <msg>  调试发消息
"""
