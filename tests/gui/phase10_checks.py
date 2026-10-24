#!/usr/bin/env python3
"""Phase 10 real-app checks, each through launch.py (never a bare exec), all
against the debug build replaying tests/gui/design-report.json.

  phase10_checks.py panel <debug Whoate> <evidence dir>   QA16 QA17 QA20 (panel)
  phase10_checks.py main <debug Whoate> <evidence dir>    QA20 (main window)
  phase10_checks.py reopen <Whoate> <evidence dir>        QA22 close → reopen → regular
  phase10_checks.py jump <debug Whoate> <evidence dir>    QA49 (R9) category and scroll

panel: the panel's own "show main window" action runs at 4 s; before it the
panel window is visible, after it the panel window is hidden and one main
window is visible and key. The footer's quit and main-window buttons are both
laid out with centres at least 60 pt apart; every row with buttons keeps its
buttons clear of its curve. main: the detail action bar and the detail chart
do not overlap. reopen: main window → close → show again → regular again.
jump (R9, owner 2026-10-24): the design report plus 30 made-up agent sessions.
Pane %22 is selected when data arrives (sidebar "tmux"), then at 6 s, with the
window open, the smallest session, as a click in the pane's session card does:
the sidebar moves to "agent sessions" and the list, rebuilt for that category,
scrolls to a row past 20. Both times the selected row lies wholly inside the
list's visible part (AppKit table geometry).
"""
import json, math, os, sys, tempfile
from pathlib import Path
import launch
from window_check import at, intersects, private_env, read_layout, states, write_receipt

DESIGN = Path(__file__).with_name("design-report.json")
SHOW_AT, QUIT_AT = 4, 9


def clip(r, box):
    """r ∩ box as (x, y, w, h), None when empty."""
    x, y = max(r[0], box[0]), max(r[1], box[1])
    w, h = min(r[0] + r[2], box[0] + box[2]) - x, min(r[1] + r[3], box[1] + box[3]) - y
    return (x, y, w, h) if w > 0 and h > 0 else None


def run(binary, env, until):
    runner = launch.Launcher()
    handle = runner.start(binary, env=env)
    try:
        exited = runner.wait(handle, until)
    finally:
        if runner.alive(handle):
            handle["stop"] = runner.stop(handle)
    launch.cleanup(handle)
    return handle, exited


# Another app may take focus while the check runs (full run 2026-10-25 00:16: the
# unified log shows One Markdown and then a Space switch to Ghostty becoming
# frontmost). When the state at check time says another app is frontmost, that run
# says nothing about whoate's key window: its receipt is kept and the scenario runs
# again, at most PANEL_RUNS times. With whoate frontmost the assertion is unchanged.
PANEL_RUNS = 3


def panel(binary, out):
    for attempt in range(1, PANEL_RUNS + 1):
        ok, focus_lost = panel_once(binary, out, attempt)
        if not focus_lost:
            return ok
        print(f"QA16 attempt {attempt}: another app was frontmost at check time ({focus_lost}); running again")
    print(f"QA16 another app held focus in all {PANEL_RUNS} runs")
    return False


def panel_once(binary, out, attempt):
    root = tempfile.mkdtemp(prefix="wp10-", dir="/private/tmp")
    state, layout_file = f"{root}/state", f"{root}/layout"
    env = private_env(root, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_LAYOUT_FILE=layout_file, WHOATE_UI_FIXTURE=os.path.realpath(DESIGN),
                      WHOATE_UI_PANEL_SHOW_MAIN_AFTER=SHOW_AT, WHOATE_UI_QUIT_AFTER=QUIT_AT)
    handle, exited = run(binary, env, QUIT_AT + 20)
    rows_state = states(state)
    first = rows_state[0][0] - float(rows_state[0][1].get("since_launch", 0)) if rows_state else 0
    before = at(rows_state, first + SHOW_AT - 0.5) or {}
    after = at(rows_state, first + SHOW_AT + 2) or {}
    rows, _ = read_layout(layout_file)
    footer = rows.get("footer", {})
    centre = lambda r: (r[0] + r[2] / 2, r[1] + r[3] / 2)
    laid_out = {"quit", "showMain"} <= footer.keys()
    distance = math.dist(centre(footer["quit"]), centre(footer["showMain"])) if laid_out else 0
    overlap = {row: not intersects(p["buttons"], p["trend"]) for row, p in rows.items() if {"buttons", "trend"} <= p.keys()}
    checks = {
        "qa16_panel_visible_before": before.get("panel_visible") == "true",
        "qa16_panel_hidden_after": after.get("panel_visible") == "false",
        "qa16_main_visible_and_key": after.get("main_windows") == "1" and after.get("visible") == "true"
        and after.get("key") == "true",
        "qa17_quit_and_show_main_laid_out": laid_out,
        "qa17_centres_at_least_60pt": distance >= 60,
        "qa20_panel_rows_checked": bool(overlap) and all(overlap.values()),
        "quit_after_exited": exited,
    }
    front = after.get("frontmost", "self")
    focus_lost = front if front != "self" else None
    name = "qa16-17-20-panel.json" if not focus_lost else f"qa16-17-20-panel.focus-lost-{attempt}.json"
    write_receipt(out, name, {"pid": handle["pid"], "attempt": attempt, "frontmost_at_check": front,
                              "states": rows_state, "footer": footer, "distance": distance, "rows": rows,
                              "checks": checks})
    print(f"QA17 footer quit={footer.get('quit')} showMain={footer.get('showMain')} centre_distance={distance:.1f}pt")
    print(f"QA20 panel rows_with_buttons={len(overlap)} {overlap}")
    print(f"QA16 attempt={attempt} frontmost_at_check={front}")
    for name, ok in checks.items():
        print(f"P10 {name}={ok}")
    return all(checks.values()), focus_lost


def main_window(binary, out):
    root = tempfile.mkdtemp(prefix="wm10-", dir="/private/tmp")
    state, layout_file = f"{root}/state", f"{root}/layout"
    env = private_env(root, WHOATE_UI_AUTOMATION="main-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_LAYOUT_FILE=layout_file, WHOATE_UI_FIXTURE=os.path.realpath(DESIGN),
                      WHOATE_UI_SELECT="kind:agent", WHOATE_UI_QUIT_AFTER=7)
    handle, exited = run(binary, env, 30)
    rows, _ = read_layout(layout_file + ".main")
    detail = rows.get("detail", {})
    selected = [s.get("selected") for _, s in states(state)]
    laid_out = {"buttons", "trend", "viewport"} <= detail.keys()
    # The chart scrolls: only its part inside the visible scroll area can meet the buttons.
    shown = clip(detail["trend"], detail["viewport"]) if laid_out else None
    checks = {
        "qa20_main_action_bar_and_chart_laid_out": laid_out,
        "qa20_main_no_overlap": laid_out and (shown is None or not intersects(detail["buttons"], shown)),
        "selected_agent": any(s and s.startswith("agent:") for s in selected),
        "quit_after_exited": exited,
    }
    write_receipt(out, "qa20-main.json", {"pid": handle["pid"], "detail": detail, "selected": selected, "checks": checks})
    print(f"QA20 main buttons={detail.get('buttons')} chart={detail.get('trend')} viewport={detail.get('viewport')}")
    for name, ok in checks.items():
        print(f"P10 {name}={ok}")
    return all(checks.values())


def reopen(binary, out):
    root = tempfile.mkdtemp(prefix="wr10-", dir="/private/tmp")
    state = f"{root}/state"
    env = private_env(root, WHOATE_UI_AUTOMATION="main-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_MAIN_CLOSE_AFTER=4, WHOATE_UI_SHOW_MAIN_AGAIN_AFTER=7, WHOATE_UI_QUIT_AFTER=11)
    handle, exited = run(binary, env, 40)
    seq = []
    for _, s in states(state):
        key = (s.get("policy"), s.get("main_windows"))
        if not seq or seq[-1] != key:
            seq.append(key)
    found = iter(seq)
    ok = all(step in found for step in [("regular", "1"), ("accessory", "0"), ("regular", "1")])
    write_receipt(out, "qa22-reopen.json", {"pid": handle["pid"], "sequence": seq, "ok": ok, "exited": exited})
    print(f"QA22 reopen sequence={seq} regular_again={ok} quit_exited={exited}")
    return ok and exited


def jump(binary, out):
    root = tempfile.mkdtemp(prefix="wj10-", dir="/private/tmp")
    report = json.loads(DESIGN.read_text())
    for i in range(30):  # made-up sessions, smaller than every design owner
        report["owners"].append({**report["owners"][-1], "id": f"agent:opencode:ses_fill{i:02d}",
                                 "label": f"填充会话 {i:02d}", "mem_bytes": (40 - i) << 20, "pids": [985000 + i],
                                 "parts": [], "history": []})
    fixture = Path(root, "report.json")
    fixture.write_text(json.dumps(report, ensure_ascii=False))
    agents = sorted((o for o in report["owners"] if o["kind"] == "agent_session"), key=lambda o: (-o["mem_bytes"], o["id"]))
    first, again = "pane:%22", agents[-1]["id"]
    state = f"{root}/state"
    env = private_env(root, WHOATE_UI_AUTOMATION="main-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_FIXTURE=str(fixture), WHOATE_UI_SELECT=first, WHOATE_UI_SELECT_AGAIN=again,
                      WHOATE_UI_SELECT_AGAIN_AFTER=6, WHOATE_UI_QUIT_AFTER=11)
    handle, exited = run(binary, env, 40)
    rows = states(state)
    def settled(target):
        """The last state with `target` selected (the scroll settles within 0.4 s)."""
        picked = [s for _, s in rows if s.get("selected") == target]
        return picked[-1] if picked else {}
    a, b = settled(first), settled(again)
    checks = {
        "qa49_first_category_tmux": a.get("category") == "tmux",
        "qa49_first_row_visible": a.get("list_row_visible") == "true",
        "qa49_again_category_agents": b.get("category") == "agents",
        "qa49_again_row_visible": b.get("list_row_visible") == "true",
        "qa49_again_row_needed_a_scroll": int(b.get("list_row", "-1")) >= 20,
        "quit_after_exited": exited,
    }
    write_receipt(out, "qa49-jump.json", {"pid": handle["pid"], "first": first, "again": again, "first_state": a,
                                          "again_state": b, "checks": checks})
    print(f"QA49 first={first} row={a.get('list_row')}/{a.get('list_rows')} visible={a.get('list_row_visible')} "
          f"category={a.get('category')}; again={again} row={b.get('list_row')} visible={b.get('list_row_visible')}")
    for name, ok in checks.items():
        print(f"P10 {name}={ok}")
    return all(checks.values())


def main(argv):
    command, *rest = argv
    ok = {"panel": panel, "main": main_window, "reopen": reopen, "jump": jump}[command](*rest)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
