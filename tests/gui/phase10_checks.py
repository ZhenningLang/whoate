#!/usr/bin/env python3
"""Phase 10 real-app checks, each through launch.py (never a bare exec), all
against the debug build replaying tests/gui/design-report.json.

  phase10_checks.py panel <debug Whoate> <evidence dir>   QA16 QA17 QA20 (panel)
  phase10_checks.py main <debug Whoate> <evidence dir>    QA20 (main window)
  phase10_checks.py reopen <Whoate> <evidence dir>        QA22 close → reopen → regular
  phase10_checks.py jump <debug Whoate> <evidence dir>    QA49 (R9) category and scroll
  phase10_checks.py dblclick <debug Whoate> <evidence dir>  QA55 (R14) double-click a panel row
  phase10_checks.py closesheet <debug Whoate> <evidence dir>  QA57 (R16) the close confirmation sheet

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
dblclick (R14, owner 2026-10-25): real mouse events posted into the panel window
(WHOATE_UI_PANEL_DOUBLE_CLICKS), so SwiftUI's own double-click recognition runs.
Owner run: a double-click on Brave's advice pill opens nothing, then one on Brave's
row brings the main window to the front with Brave selected under "apps", in view,
and the panel put away. Swarm run: the python3.13 process group row (no buttons)
is selected under "all". Both runs also read the window title (R15, round 7): the
window is named "WHO-ATE", the system draws it with nothing in front (it starts
within a few points of the list column; a 20 pt icon pushed it 28 pt further) and level
with the traffic lights; the toolbar keeps its height and the sort control stays at
the right edge (hiding the title measured 38 pt and 526 pt from the edge, against
52 pt and 6 pt).
closesheet (R16, owner 2026-10-25): the orphan selected in the main window, its close
confirmation opened at 4 s as the main window's sheet. Confirm run: at 6 s the
sheet's centre lies near the window's centre, the dim layer covers the whole window
(all three columns), the sheet has the Esc button left of the red one; Return at 6 s
leaves it open and sends nothing; Esc at 8.5 s closes it. Force run: the second step
(every pid still running, made by the app from the confirmation) in the same place
("完成" on Esc, red "强制结束"), same keys. Quit-while-open run: the sheet is still up
at 8 s when the app's own quit runs, and the app exits anyway (a sheet used to block it). Under this automation the app's signal sender
is a dry run that only counts calls, and the count must stay 0.
"""
import json, math, os, sys, tempfile
from pathlib import Path
import launch
from window_check import at, intersects, private_env, read_layout, states, without_input, write_receipt

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


def panel(binary, out):
    # Input from someone using the Mac reruns the scenario (window_check.without_input).
    return without_input(lambda attempt: panel_once(binary, out, attempt), "QA16")


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
    # Who had focus at check time, for the receipt; it never decides the result.
    front = after.get("frontmost", "-")
    write_receipt(out, "qa16-17-20-panel.json", {"pid": handle["pid"], "attempt": attempt, "frontmost_at_check": front,
                                                  "states": rows_state, "footer": footer, "distance": distance,
                                                  "rows": rows, "checks": checks})
    print(f"QA17 footer quit={footer.get('quit')} showMain={footer.get('showMain')} centre_distance={distance:.1f}pt")
    print(f"QA20 panel rows_with_buttons={len(overlap)} {overlap}")
    print(f"QA16 attempt={attempt} frontmost_at_check={front}")
    for name, ok in checks.items():
        print(f"P10 {name}={ok}")
    return all(checks.values()), os.path.join(out, "qa16-17-20-panel.json")


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


# R13 layout (measured 2026-10-25): 52 pt and 6 pt with the system title; 38 and 526 without it.
TOOLBAR_MIN_H, SORT_RIGHT_GAP_MAX = 48, 40
# R15 (round 7): title start from the list column's left edge and its centre's offset from the
# traffic lights'. Measured values are in qa.md (round 7); a 20 pt icon in front adds 28 pt.
TITLE, TITLE_LEAD_MAX, TITLE_DY_MAX = "WHO-ATE", 20, 3
DOUBLE_CLICK_RUNS = {
    # name: (clicks, expected selection, expected category)
    "owner": ("4 pill app:com.brave.Browser;6 row app:com.brave.Browser", "app:com.brave.Browser", "apps"),
    "swarm": ("4 row swarm:exe:python3.13", "swarm:exe:python3.13", "all"),
}


def dblclick(binary, out):
    # Both runs, even after a failure: each receipt is evidence of its own.
    return all([without_input(lambda attempt, name=name: dblclick_once(binary, out, name, attempt), f"QA55 {name}")
                for name in DOUBLE_CLICK_RUNS])


def dblclick_once(binary, out, name, attempt):
    clicks, target, category = DOUBLE_CLICK_RUNS[name]
    last = float(clicks.split(";")[-1].split()[0])
    root = tempfile.mkdtemp(prefix="wd10-", dir="/private/tmp")
    state = f"{root}/state"
    env = private_env(root, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_FIXTURE=os.path.realpath(DESIGN), WHOATE_UI_PANEL_DOUBLE_CLICKS=clicks,
                      WHOATE_UI_QUIT_AFTER=last + 5)
    handle, exited = run(binary, env, last + 25)
    rows = states(state)
    first = rows[0][0] - float(rows[0][1].get("since_launch", 0)) if rows else 0
    before = at(rows, first + last - 0.3) or {}
    after = at(rows, first + last + 2.5) or {}
    checks = {
        "qa55_panel_open_before": before.get("panel_visible") == "true" and before.get("main_windows") == "0",
        "qa55_panel_put_away": after.get("panel_visible") == "false",
        "qa55_main_in_front": after.get("main_windows") == "1" and after.get("visible") == "true"
        and after.get("key") == "true" and after.get("active") == "true",
        "qa55_selected_this_row": after.get("selected") == target,
        "qa55_category": after.get("category") == category,
        "qa55_row_visible": after.get("list_row_visible") == "true",
        "qa54_window_named_wordmark": after.get("title") == TITLE,
        "qa54_title_still_drawn": after.get("title_shown") == "true" and after.get("title_drawn") == "true",
        "qa54_nothing_before_title": after.get("title_lead", "-").lstrip("-").isdigit()
        and 0 <= int(after["title_lead"]) <= TITLE_LEAD_MAX,
        "qa54_title_level_with_traffic_lights": after.get("title_dy", "-").isdigit() and int(after["title_dy"]) <= TITLE_DY_MAX,
        "qa54_toolbar_not_compact": int(after.get("toolbar_h", "0")) >= TOOLBAR_MIN_H,
        "qa54_sort_stays_right": after.get("sort_right_gap", "-").isdigit() and int(after["sort_right_gap"]) <= SORT_RIGHT_GAP_MAX,
        "quit_after_exited": exited,
    }
    receipt = f"qa55-dblclick-{name}.json"
    write_receipt(out, receipt, {"pid": handle["pid"], "attempt": attempt, "clicks": clicks, "before": before,
                                 "after": after, "states": rows, "checks": checks})
    print(f"QA55 {name} attempt={attempt} selected={after.get('selected')} category={after.get('category')} "
          f"row={after.get('list_row')}/{after.get('list_rows')} visible={after.get('list_row_visible')} "
          f"frontmost_at_check={after.get('frontmost', '-')}")
    for check, ok in checks.items():
        print(f"P10 {name} {check}={ok}")
    return all(checks.values()), os.path.join(out, receipt)


# R16: how far the sheet's centre may sit from the window's centre (pt), and when things happen.
SHEET_CENTRE_MAX, CLOSE_AT, RETURN_AT, ESC_AT, CLOSE_QUIT = 24, 4, 6, 8.5, 11.5
CLOSE_RUNS = {
    # name: (WHOATE_UI_CLOSE_STEP, expected step, keys pressed)
    "confirm": ("", "confirm", True),
    "force": ("force", "result_still_alive", True),
    # Quit while the sheet is up: a sheet made NSApp.terminate do nothing (a test
    # instance outlived its quit by 30 minutes, 2026-10-25); the app must still exit.
    "quit-while-open": ("", "confirm", False),
}


def closesheet(binary, out):
    return all([without_input(lambda attempt, name=name: closesheet_once(binary, out, name, attempt), f"QA57 {name}")
                for name in CLOSE_RUNS])


def closesheet_once(binary, out, name, attempt):
    step, expected, keys = CLOSE_RUNS[name]
    root = tempfile.mkdtemp(prefix="wc10-", dir="/private/tmp")
    state, window_file = f"{root}/state", f"{root}/window"
    # A main-window confirmation under automation swaps in a sender that sends nothing
    # and only counts (kill_attempts), so the Return press can never end a process.
    env = private_env(root, WHOATE_UI_AUTOMATION="main-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_LAYOUT_FILE=f"{root}/layout", WHOATE_UI_WINDOW_FILE=window_file,
                      WHOATE_UI_FIXTURE=os.path.realpath(DESIGN), WHOATE_UI_SELECT="kind:orphan",
                      WHOATE_UI_CONFIRM="selected", WHOATE_UI_CLOSE_AT=CLOSE_AT, WHOATE_UI_CLOSE_STEP=step,
                      WHOATE_UI_QUIT_AFTER=CLOSE_QUIT if keys else ESC_AT, LC_ALL="zh_CN.UTF-8",
                      **(dict(WHOATE_UI_RETURN_AFTER=RETURN_AT, WHOATE_UI_ESC_AFTER=ESC_AT) if keys else {}))
    handle, exited = run(binary, env, CLOSE_QUIT + 25)
    rows = states(state)
    first = rows[0][0] - float(rows[0][1].get("since_launch", 0)) if rows else 0
    shown = at(rows, first + RETURN_AT - 0.5) or {}
    after_return = at(rows, first + ESC_AT - 0.5) or {}
    gone = at(rows, first + ESC_AT + 1.5) or {}
    window = next((l.split("\t") for l in Path(window_file).read_text().splitlines() if l.startswith("main\t")), None) \
        if os.path.exists(window_file) else None
    width, height = (int(window[2]), int(window[3])) if window else (0, 0)
    dim = [int(v) for v in shown["dim"].split(",")] if shown.get("dim", "-") != "-" else None
    buttons = [b for b in shown.get("sheet_buttons", "").split(",") if b]
    def centred(key):
        value = shown.get(key, "-")
        return value.lstrip("-").isdigit() and abs(int(value)) <= SHEET_CENTRE_MAX
    checks = {
        "qa57_step_shown": shown.get("close") == expected and shown.get("sheet", "none") != "none",
        # Every run must end by its own quit within the wait; quit-while-open quits with the sheet up.
        "qa57_quit_exits": exited,
        "qa57_sheet_centred_x": centred("sheet_dx"),
        "qa57_sheet_centred_y": centred("sheet_dy"),
        # The dim layer spans the window's width (sidebar, list and details) and its content height.
        "qa57_whole_window_dimmed": dim is not None and dim[0] <= 1 and dim[2] >= width - 2
        and dim[3] >= height - int(shown.get("toolbar_h", "0")) - 2,
        # The toolbar (AppKit's titlebar) is dimmed too (review round 7, spec「整窗变暗」).
        "qa57_toolbar_dimmed": int(shown.get("titlebar_dim", "0")) >= int(shown.get("toolbar_h", "999")) - 2,
        # Esc's button on the left, the red destructive one on the right (titles: WhoateChecks QA57).
        "qa57_two_buttons": buttons == ["esc", "destructive"],
        "qa57_no_signal": (rows[-1][1].get("kill_attempts") if rows else None) == "0",
    }
    if keys:
        checks["qa57_return_ends_nothing"] = after_return.get("close") == expected and after_return.get("kill_attempts") == "0"
        checks["qa57_esc_closes"] = gone.get("close") == "idle" and gone.get("sheet") == "none" \
            and gone.get("titlebar_dim") == "0"
    else:
        checks["qa57_sheet_up_at_quit"] = after_return.get("close") == expected and after_return.get("sheet", "none") != "none"
    receipt = f"qa57-closesheet-{name}.json"
    write_receipt(out, receipt, {"pid": handle["pid"], "attempt": attempt, "shown": shown, "after_return": after_return,
                                 "gone": gone, "window": [width, height], "states": rows, "checks": checks})
    print(f"QA57 {name} attempt={attempt} close={shown.get('close')} sheet={shown.get('sheet')} "
          f"dx={shown.get('sheet_dx')} dy={shown.get('sheet_dy')} buttons={buttons} dim={dim} window={width}x{height} "
          f"titlebar_dim={shown.get('titlebar_dim')} toolbar_h={shown.get('toolbar_h')} "
          f"after_return={after_return.get('close')} after_esc={gone.get('close')} kills={gone.get('kill_attempts')}")
    for check, ok in checks.items():
        print(f"P10 {name} {check}={ok}")
    return all(checks.values()), os.path.join(out, receipt)


def main(argv):
    command, *rest = argv
    ok = {"panel": panel, "main": main_window, "reopen": reopen, "jump": jump, "dblclick": dblclick,
          "closesheet": closesheet}[command](*rest)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
