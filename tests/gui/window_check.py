#!/usr/bin/env python3
"""Phase 08 real-app checks, each through launch.py (never a bare exec).

  window_check.py activation <Whoate executable> <evidence dir>   QA12
  window_check.py quit <Whoate executable> <evidence dir>         QA13
  window_check.py layout <debug Whoate executable> <evidence dir>             QA16 QA34
  window_check.py hidden-open <Whoate executable> <evidence dir>              QA31 (real app)

activation: the app starts accessory; the main window makes it regular and
Foreground; `open -a Finder` takes focus once, and 4 s later the window is
still visible and unoccluded while full refreshes continue at <= 3 s; showing
again keeps one window and makes it key; closing it returns to accessory and
hidden refresh (host, hidden-full; no full or resume). quit: with the main window open, QuitAction ends the app
within 5 s and leaves no LaunchServices residue. layout: in every panel row
with buttons, the buttons and the curve do not overlap (debug replay of
panel-act-report.json: one closable orphan row, since a live machine may have none);
QA34 then captures only that panel window (`screencapture -l<window>`) for the
owner to look at. hidden-open: hidden for 65 s (paused full ticks every 30 s),
then the panel shows a line within 4 s; the 5 s control shows only
"not enough samples".
"""
import json, os, re, subprocess, sys, tempfile, time
from pathlib import Path
import launch, ls_residue, screenshots

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


# Someone using the Mac decides a focus check: a click into another app raises it
# over whoate. Full run 2026-10-25 00:48: each of seven focus losses came right
# after a trackpad click (unified log), and an A/B run of c60cd58 and 8969b17+ with
# no input passed both. A run with keyboard or pointer input is evidence of
# neither; its receipt is kept under another name and the scenario runs again.
# Without input the assertions decide, whoever is frontmost.
QUIET_S, QUIET_WAIT_S, FOCUS_RUNS = 3, 120, 3


def hid_idle(run=subprocess.run):
    """Seconds since the last keyboard, mouse or trackpad input (IOHIDSystem
    HIDIdleTime); None when it cannot be read."""
    out = run(["/usr/sbin/ioreg", "-c", "IOHIDSystem", "-d", "4", "-k", "HIDIdleTime"],
              capture_output=True, text=True, timeout=10).stdout
    m = re.search(r'"HIDIdleTime" = (\d+)', out)
    return int(m.group(1)) / 1e9 if m else None


def without_input(scenario, label, idle=hid_idle, sleep=time.sleep, clock=time.monotonic):
    """scenario(attempt) -> (ok, receipt path). Waits for QUIET_S seconds without
    input (at most QUIET_WAIT_S), runs, and reruns up to FOCUS_RUNS times while input
    arrived during the run. All runs touched: not passed, reported as blocked."""
    for attempt in range(1, FOCUS_RUNS + 1):
        waited = clock()
        while (idle() or 0) < QUIET_S and clock() - waited < QUIET_WAIT_S:
            sleep(0.5)
        began = clock()
        ok, receipt = scenario(attempt)
        quiet = idle()
        touched = quiet is not None and quiet < clock() - began
        print(f"{label} attempt={attempt} input_during_run={touched} idle_at_end={quiet}")
        if not touched:
            return ok
        stem, ext = os.path.splitext(receipt)
        os.replace(receipt, f"{stem}.human-input-{attempt}{ext}")
    print(f"BLOCKED {label} human-input: keyboard or pointer input during all {FOCUS_RUNS} runs")
    return False


def activation(binary, out):
    return without_input(lambda attempt: activation_once(binary, out), "QA12")


def activation_once(binary, out):
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
    # Hidden again: host and the 30 s paused hidden-full only, never full or resume.
    checks["close_no_full_or_resume"] = not any(m in ("full", "resume") for t, m in ticks if t > began + CLOSE_AT + 0.5)
    # R15: the release bundle's main window is named and drawn as the wordmark.
    checks["qa54_bundle_title_wordmark"] = opened.get("title") == "WHO-ATE" and opened.get("title_drawn") == "true"
    checks["quit_after_exited"] = exited
    evidence = {"pid": handle["pid"], "isolation": handle["isolation"], "finder_at": finder, "ls_types": types,
                "states": rows, "schedule": ticks, "full_after_focus": full_after, "gaps": gaps, "checks": checks}
    write_receipt(out, "qa12-activation.json", evidence)
    for name, ok in checks.items():
        print(f"QA12 {name}={ok}")
    print(f"QA12 full_after_focus={len(full_after)} max_gap={max(gaps, default=None)}")
    launch.cleanup(handle)
    return all(checks.values()), os.path.join(out, "qa12-activation.json")


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


def read_layout(path):
    """{row: {part: (x, y, w, h)}} and {row: (curve kind, epoch written)} from WHOATE_UI_LAYOUT_FILE."""
    rows, curves, text = {}, {}, ""
    if os.path.exists(path):
        with open(path) as f:
            text = f.read()
    for line in text.splitlines():
        row, part, *rest = line.split("\t")
        if part == "curve":
            curves[row] = (rest[0], float(rest[1]))
        else:
            rows.setdefault(row, {})[part] = tuple(map(float, rest))
    return rows, curves


def capture_window(window_file, pid, png):
    """QA34: only the panel window this run started, never the whole screen."""
    with open(window_file) as f:
        _, number, width, height, scale, owner, _ = f.read().rstrip("\n").split("\t")
    shot = subprocess.run(["/usr/sbin/screencapture", f"-l{number}", "-x", "-o", png], capture_output=True, text=True)
    result = {"window": int(number), "window_pid": int(owner), "launched_pid": pid, "file": png,
              "screencapture_rc": shot.returncode, "stderr": shot.stderr.strip()}
    if shot.returncode != 0 or not os.path.exists(png):
        print(f"QA34 blocked screen-recording-permission: {shot.stderr.strip()}")
        return result, False
    stats = screenshots.stats(png)
    expected = (round(int(width) * float(scale)), round(int(height) * float(scale)))
    result.update(stats, expected_size=expected, pid_ok=int(owner) == pid,
                  size_ok=(stats["width"], stats["height"]) == expected, not_solid=stats["distinct_colours"] > 8)
    return result, result["pid_ok"] and result["size_ok"] and result["not_solid"]


def layout(binary, out, fixture=ACT_REPORT):
    root = tempfile.mkdtemp(prefix="wlay-", dir="/private/tmp")
    path, window_file = f"{root}/layout", f"{root}/window"
    env = private_env(root, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_LAYOUT_FILE=path,
                      WHOATE_UI_WINDOW_FILE=window_file,
                      WHOATE_UI_FIXTURE=os.path.realpath(fixture), WHOATE_UI_PANEL_CLOSE_AFTER=30)
    runner = launch.Launcher()
    handle = runner.start(binary, env=env)
    rows, curves, shot, shot_ok = {}, {}, None, False
    try:
        for _ in range(150):
            time.sleep(0.2)
            rows, curves = read_layout(path)
            if any("buttons" in parts for parts in rows.values()) and os.path.exists(window_file):
                time.sleep(1)
                rows, curves = read_layout(path)
                shot, shot_ok = capture_window(window_file, handle["pid"], os.path.join(out, "qa16-panel.png"))
                break
    finally:
        handle["stop"] = runner.stop(handle)
    checked = {row: not intersects(parts["buttons"], parts["trend"])
               for row, parts in rows.items() if "buttons" in parts and "trend" in parts}
    act_curves = {row: curves.get(row, ("missing",))[0] for row in checked}
    write_receipt(out, "qa16-layout.json",
                  {"source": f"debug fixture replay {fixture}", "rows": rows, "checked": checked})
    write_receipt(out, "qa34-panel-shot.json", {"shot": shot, "act_row_curves": act_curves, "ok": shot_ok})
    for row, ok in checked.items():
        print(f"QA16 row={row} buttons={rows[row]['buttons']} trend={rows[row]['trend']} no_overlap={ok}")
    print(f"QA16 rows_with_buttons={len(checked)} source=fixture:{os.path.basename(fixture)}")
    if shot:
        print(f"QA34 file={shot['file']} window={shot['window']} pid_ok={shot.get('pid_ok')} "
              f"size={shot.get('width')}x{shot.get('height')} expected={shot.get('expected_size')} "
              f"colours={shot.get('distinct_colours')} act_row_curves={act_curves}")
    else:
        print("QA34 no panel window to capture")
    launch.cleanup(handle)
    qa16 = bool(checked) and all(checked.values())
    qa34 = shot_ok and bool(act_curves) and all(kind == "line" for kind in act_curves.values())
    print(f"QA16 ok={qa16} QA34 ok={qa34}")
    return qa16 and qa34


OPEN_AFTER, CONTROL_AFTER, WATCH_S = 65, 5, 4


def hidden_open_run(binary, out, after):
    """One launch: hidden `after` s, then the panel window; the layout is polled
    from the moment the panel's first full refresh is logged until 4 s later."""
    root = tempfile.mkdtemp(prefix="whid-", dir="/private/tmp")
    path, log = f"{root}/layout", f"{root}/schedule"
    env = private_env(root, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_LAYOUT_FILE=path,
                      WHOATE_UI_SCHEDULE_LOG=log, WHOATE_UI_PANEL_OPEN_AFTER=after)
    runner = launch.Launcher()
    handle = runner.start(binary, env=env)
    seen, opened = [], None
    try:
        deadline = time.time() + after + 60
        while time.time() < deadline:
            time.sleep(0.2)
            ticks = schedule(log)
            opened = opened or next((t for t, m in ticks if m in ("resume", "full")), None)
            if opened is None:
                continue
            _, curves = read_layout(path)
            seen.append({"at": time.time(), "curves": curves})
            if time.time() > opened + WATCH_S + 0.5:
                break
    finally:
        handle["stop"] = runner.stop(handle)
    ticks = schedule(log)
    before = [m for t, m in ticks if opened is None or t < opened]
    window = [s for s in seen if opened is not None and s["at"] <= opened + WATCH_S]
    lines = sorted(written for s in window for kind, written in s["curves"].values() if kind == "line")
    receipt = {"open_after_s": after, "pid": handle["pid"], "isolation": handle["isolation"], "opened_at": opened,
               "schedule": ticks, "polls": seen,
               "hidden_full_before_open": before.count("hidden-full"), "full_before_open": before.count("full"),
               "rows_seen": sorted({row for s in window for row in s["curves"]}),
               "first_line_written_s_after_open": round(lines[0] - opened, 3) if lines else None}
    write_receipt(out, f"qa31-open-after-{after}.json", receipt)
    launch.cleanup(handle)
    return receipt


def hidden_open(binary, out):
    """QA31 real app: after 65 s hidden the curve is there within 4 s of opening; the 5 s control is not."""
    late, early = hidden_open_run(binary, out, OPEN_AFTER), hidden_open_run(binary, out, CONTROL_AFTER)
    first = late["first_line_written_s_after_open"]
    checks = {
        "opened": late["opened_at"] is not None and early["opened_at"] is not None,
        "two_hidden_full_before_open": late["hidden_full_before_open"] >= 2 and late["full_before_open"] == 0,
        "line_within_4s": first is not None and 0 <= first <= WATCH_S,
        "control_rows_seen": bool(early["rows_seen"]),
        "control_all_insufficient": early["first_line_written_s_after_open"] is None,
    }
    for name, ok in checks.items():
        print(f"QA31 {name}={ok}")
    print(f"QA31 open_after={OPEN_AFTER} hidden_full_before={late['hidden_full_before_open']} "
          f"first_line_s={first} control_after={CONTROL_AFTER} control_rows={len(early['rows_seen'])}")
    return all(checks.values())


def main(argv):
    command, *rest = argv
    ok = {"activation": activation, "quit": quit_check, "layout": layout, "hidden-open": hidden_open}[command](*rest)
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
