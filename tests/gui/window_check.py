#!/usr/bin/env python3
"""Phase 08 real-app checks, each through launch.py (never a bare exec).

  window_check.py activation <Whoate executable> <evidence dir>   QA12
  window_check.py quit <Whoate executable> <evidence dir>         QA13
  window_check.py layout <debug Whoate executable> <evidence dir>             QA16

activation: the app starts accessory; the main window makes it regular and
Foreground; `open -a Finder` takes focus once, and 4 s later the window is
still visible and unoccluded while full refreshes continue at <= 3 s; showing
again keeps one window and makes it key; closing it returns to accessory and
host-only refresh. quit: with the main window open, QuitAction ends the app
within 5 s and leaves no LaunchServices residue. layout: in every panel row
with buttons, the buttons and the curve do not overlap (debug replay of
panel-act-report.json: one closable orphan row, since a live machine may have none).
"""
import json, os, subprocess, sys, tempfile, time
from pathlib import Path
import launch, ls_residue

# Seconds on the app's own automation clock (WHOATE_UI_*_AFTER): since its
# process started, as each state line's since_launch reports.
FINDER_AT, SHOW_AT, CLOSE_AT, QUIT_AT = 6, 14, 20, 26
ACT_REPORT = Path(__file__).with_name("panel-act-report.json")


def private_env(root, **extra):
    for name in ("home", "cache"):
        os.makedirs(os.path.join(root, name), exist_ok=True)
    env = {"HOME": f"{root}/home", "WHOATE_HOME_OVERRIDE": f"{root}/home", "WHOATE_CACHE_DIR": f"{root}/cache",
           "WHOATE_RULES": f"{root}/rules.toml", "TMUX_TMPDIR": root}
    env.update({k: str(v) for k, v in extra.items()})
    return env


def write_receipt(out, name, data):
    with open(os.path.join(out, name), "x") as f:
        json.dump(data, f, indent=1)


def states(path):
    """[(epoch, {field: value})] from WHOATE_UI_STATE_FILE."""
    rows = []
    text = ""
    if os.path.exists(path):
        with open(path) as f:
            text = f.read()
    for line in text.splitlines():
        at, *fields = line.split("\t")
        rows.append((float(at), dict(f.split("=", 1) for f in fields)))
    return rows


def schedule(path):
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [(float(a), m) for a, m in (l.split("\t") for l in f.read().splitlines())]


def at(rows, moment):
    """The state in effect at `moment`."""
    current = None
    for when, state in rows:
        if when <= moment:
            current = state
    return current


def ls_type(pid):
    entry = ls_residue.entry_for(pid)
    return entry and entry["type"]


def activation(binary, out):
    root = tempfile.mkdtemp(prefix="wwin-", dir="/private/tmp")
    state, log = f"{root}/state", f"{root}/schedule"
    env = private_env(root, WHOATE_UI_AUTOMATION="main-window", WHOATE_UI_STATE_FILE=state,
                      WHOATE_UI_SCHEDULE_LOG=log, WHOATE_UI_SHOW_MAIN_AFTER=SHOW_AT,
                      WHOATE_UI_MAIN_CLOSE_AFTER=CLOSE_AT, WHOATE_UI_QUIT_AFTER=QUIT_AT)
    runner = launch.Launcher()
    handle = runner.start(binary, env=env)
    checks, types = {}, {}

    def until(offset):
        time.sleep(max(0, began + offset - time.time()))

    try:
        for _ in range(100):
            if states(state):
                break
            time.sleep(0.1)
        # The app's timers count from its process start; the state line says how long ago that was.
        first = states(state)[0] if states(state) else None
        began = first[0] - float(first[1].get("since_launch", 0)) if first else time.time()
        until(FINDER_AT - 1)
        types["main_open"] = ls_type(handle["pid"])
        until(FINDER_AT)
        finder = time.time()
        # The one focus switch this check makes (QA12): Finder, no Apple Events.
        subprocess.run(["/usr/bin/open", "-a", "Finder"], check=True, timeout=30)
        print(f"QA12 focus_switch open -a Finder at={finder:.3f}")
        time.sleep(4)
        after_focus = time.time()
        until(SHOW_AT + 0.6)
        after_show = time.time()
        until(CLOSE_AT + 2)
        after_close = time.time()
        exited = runner.wait(handle, QUIT_AT + 15 - (time.time() - began))
    finally:
        if runner.alive(handle):
            handle["stop"] = runner.stop(handle)
    rows, ticks = states(state), schedule(log)
    first = rows[0][1] if rows else {}
    opened = next((s for _, s in rows if s.get("main_windows") == "1"), {})
    focus = at(rows, after_focus) or {}
    shown = at(rows, after_show) or {}
    closed = at(rows, after_close) or {}
    full_after = [t for t, m in ticks if m == "full" and finder < t <= began + SHOW_AT]
    gaps = [b - a for a, b in zip(full_after, full_after[1:])]
    checks["starts_accessory"] = first.get("policy") == "accessory" and first.get("main_windows") == "0"
    checks["main_window_regular"] = opened.get("policy") == "regular"
    checks["ls_type_foreground"] = types["main_open"] == "Foreground"
    checks["visible_after_focus_loss"] = focus.get("active") == "false" and focus.get("visible") == "true" \
        and focus.get("occlusion_visible") == "true"
    checks["full_refresh_after_focus_loss"] = len(full_after) >= 2 and all(g <= 3 for g in gaps)
    checks["show_again_one_key_window"] = shown.get("main_windows") == "1" and shown.get("key") == "true"
    checks["close_returns_accessory"] = closed.get("policy") == "accessory" and closed.get("main_windows") == "0"
    checks["close_returns_host"] = any(m == "host" for t, m in ticks if t > began + CLOSE_AT)
    checks["quit_after_exited"] = exited
    evidence = {"pid": handle["pid"], "isolation": handle["isolation"], "finder_at": finder, "ls_types": types,
                "states": rows, "schedule": ticks, "full_after_focus": full_after, "gaps": gaps, "checks": checks}
    write_receipt(out, "qa12-activation.json", evidence)
    for name, ok in checks.items():
        print(f"QA12 {name}={ok}")
    print(f"QA12 full_after_focus={len(full_after)} max_gap={max(gaps, default=None)}")
    launch.cleanup(handle)
    return all(checks.values())


QUIT_LIMIT_S = 5


def quit_check(binary, out, runner=None, clock=time.monotonic, lister=launch.processes,
               entry_for=ls_residue.entry_for, sleep=time.sleep):
    """QA13: one 5 s budget from the launch request (the app's quit timer is 3 s
    after its own start), covering pid resolution and the exit itself."""
    root = tempfile.mkdtemp(prefix="wquit-", dir="/private/tmp")
    state = f"{root}/state"
    env = private_env(root, WHOATE_UI_AUTOMATION="main-window", WHOATE_UI_STATE_FILE=state, WHOATE_UI_QUIT_AFTER=3)
    runner = runner or launch.Launcher()
    bundle_exe = os.path.realpath(binary)
    began = clock()
    handle = runner.start(binary, env=env)
    # Instances already running (the owner's own dist/whoate.app) are not ours.
    others = {p["pid"] for p in lister(os.path.dirname(handle["path"]))} - {handle["pid"]}
    exited = runner.wait(handle, max(0.0, QUIT_LIMIT_S - (clock() - began)))
    took = clock() - began
    if not exited:
        handle["stop"] = runner.stop(handle)
    left = [p["pid"] for p in lister(os.path.dirname(handle["path"])) if p["pid"] not in others]
    residue = None
    for _ in range(16):
        entry = entry_for(handle["pid"])
        residue = entry and "exited-with-subordinates" in entry["flags"]
        if not residue:
            break
        sleep(1)
    main_open = any(s.get("main_windows") == "1" for _, s in states(state))
    checks = {"main_window_was_open": main_open, "exited_within_5s": bool(exited) and took <= QUIT_LIMIT_S,
              "no_process_by_pid": not runner.alive(handle), "no_process_by_path": not left,
              "no_ls_residue": not residue}
    write_receipt(out, "qa13-quit.json", {"pid": handle["pid"], "seconds_from_launch_request": took,
                                          "limit_s": QUIT_LIMIT_S, "left": left, "other_instances": sorted(others),
                                          "binary": bundle_exe, "checks": checks})
    for name, ok in checks.items():
        print(f"QA13 {name}={ok}")
    print(f"QA13 seconds_from_launch_request={took:.2f} limit={QUIT_LIMIT_S}")
    launch.cleanup(handle)
    return all(checks.values())


def intersects(a, b):
    return a[0] < b[0] + b[2] and b[0] < a[0] + a[2] and a[1] < b[1] + b[3] and b[1] < a[1] + a[3]


def layout(binary, out, fixture=ACT_REPORT):
    root = tempfile.mkdtemp(prefix="wlay-", dir="/private/tmp")
    path = f"{root}/layout"
    env = private_env(root, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_LAYOUT_FILE=path,
                      WHOATE_UI_FIXTURE=os.path.realpath(fixture), WHOATE_UI_PANEL_CLOSE_AFTER=30)
    runner = launch.Launcher()
    handle = runner.start(binary, env=env)
    rows = {}
    try:
        for _ in range(150):
            time.sleep(0.2)
            rows = {}
            text = ""
            if os.path.exists(path):
                with open(path) as f:
                    text = f.read()
            for line in text.splitlines():
                row, part, *box = line.split("\t")
                rows.setdefault(row, {})[part] = tuple(map(float, box))
            if any("buttons" in parts for parts in rows.values()):
                time.sleep(1)
                break
    finally:
        handle["stop"] = runner.stop(handle)
    checked = {row: not intersects(parts["buttons"], parts["trend"])
               for row, parts in rows.items() if "buttons" in parts and "trend" in parts}
    write_receipt(out, "qa16-layout.json",
                  {"source": f"debug fixture replay {fixture}", "rows": rows, "checked": checked})
    for row, ok in checked.items():
        print(f"QA16 row={row} buttons={rows[row]['buttons']} trend={rows[row]['trend']} no_overlap={ok}")
    print(f"QA16 rows_with_buttons={len(checked)} source=fixture:{os.path.basename(fixture)}")
    launch.cleanup(handle)
    return bool(checked) and all(checked.values())


def main(argv):
    command, *rest = argv
    ok = {"activation": activation, "quit": quit_check, "layout": layout}[command](*rest)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
