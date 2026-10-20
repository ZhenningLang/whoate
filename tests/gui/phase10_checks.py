#!/usr/bin/env python3
"""Phase 10 real-app checks, each through launch.py (never a bare exec), all
against the debug build replaying tests/gui/design-report.json.

  phase10_checks.py panel <debug Whoate> <evidence dir>   QA16 QA17 QA20 (panel)
  phase10_checks.py main <debug Whoate> <evidence dir>    QA20 (main window)
  phase10_checks.py reopen <Whoate> <evidence dir>        QA22 close → reopen → regular

panel: the panel's own "show main window" action runs at 4 s; before it the
panel window is visible, after it the panel window is hidden and one main
window is visible and key. The footer's quit and main-window buttons are both
laid out with centres at least 60 pt apart; every row with buttons keeps its
buttons clear of its curve. main: the detail action bar and the detail chart
do not overlap. reopen: main window → close → show again → regular again.
"""
import math, os, sys, tempfile
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


def panel(binary, out):
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
    write_receipt(out, "qa16-17-20-panel.json", {"pid": handle["pid"], "states": rows_state, "footer": footer,
                                                  "distance": distance, "rows": rows, "checks": checks})
    print(f"QA17 footer quit={footer.get('quit')} showMain={footer.get('showMain')} centre_distance={distance:.1f}pt")
    print(f"QA20 panel rows_with_buttons={len(overlap)} {overlap}")
    for name, ok in checks.items():
        print(f"P10 {name}={ok}")
    return all(checks.values())


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


def main(argv):
    command, *rest = argv
    ok = {"panel": panel, "main": main_window, "reopen": reopen}[command](*rest)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
