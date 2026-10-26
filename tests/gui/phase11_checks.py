#!/usr/bin/env python3
"""Phase 11 real-app checks (settings window), each through launch.py, all
against the debug build replaying tests/gui/design-report.json. Every run is
automation with a settings suite at an absolute path under its own temporary
root (never the owner's UserDefaults), a fake login item and a fake notifier.

  phase11_checks.py settings-window <debug Whoate> <evidence dir>   QA18
  phase11_checks.py persist <debug Whoate> <evidence dir>           QA19
  phase11_checks.py first-launch <debug Whoate> <evidence dir>      QA20
  phase11_checks.py notify <debug Whoate> <evidence dir>            QA27
  phase11_checks.py shots <debug Whoate> <evidence dir> <cli>       QA22
  phase11_checks.py settle <debug Whoate> <evidence dir> <cli>      QA22 (tab heights hold)
  phase11_checks.py owner-snapshot <file>                           QA23 before (no login items: see owner_state)
  phase11_checks.py owner-compare <file>                            QA23 after

settings-window: opened at 3 s with no main window, then every tab in turn, closed
at 12.5 s; zh and en (LC_ALL). persist: four settings set by WHOATE_UI_SET, quit;
a second run on the same suite reads them back and runs hidden for 130 s on the
power-saving cadence. first-launch: an empty suite with WHOATE_UI_FIRST_LAUNCH=1
opens the main window once and points at the menu bar; the same suite again does
not; an empty suite without the switch does not either. notify: a replay with
tight pressure and one growing row, notifications switched on in the suite; one
notification of each kind, then the fake's click on the growing one opens the
main window on that row.
"""
import ctypes, glob, json, os, plistlib, shutil, statistics, subprocess, sys, tempfile, time
from pathlib import Path
import launch
import screenshots
from window_check import at, private_env, schedule, states, without_input, write_receipt

DESIGN = Path(__file__).with_name("design-report.json")
TABS = ["general", "menubar", "notifications", "cli", "about"]
# The keys Phase 11 owns in a settings domain (plan step 1).
SETTING_KEYS = ["appearance", "language", "menuBarDisplay", "refreshProfile", "notifications.enabled",
                "notifications.pressure", "notifications.growing", "firstLaunchDone"]
OWNER_DOMAIN = "io.github.zhenninglang.whoate"
REAL_LINKS = ["/opt/homebrew/bin/whoate", os.path.expanduser("~/.local/bin/whoate"), "/usr/local/bin/whoate"]


def root(prefix):
    return tempfile.mkdtemp(prefix=prefix, dir=os.environ.get("WHOATE_P11_TMP", "/private/tmp"))


def env_for(base, fixture=None, **extra):
    """private_env plus the Phase 11 isolation: a suite at an absolute path in `base`."""
    return private_env(base, WHOATE_UI_DEFAULTS=f"{base}/suite", WHOATE_UI_FIXTURE=fixture or os.path.realpath(DESIGN),
                       WHOATE_UI_CLI_DIRS=f"{base}/bin", WHOATE_UI_CLI_PATH=f"{base}/bin", **extra)


def home_free(env, home=os.path.expanduser("~")):
    """QA22: nothing the app is given points into the owner's home (screenshots may be published)."""
    return not any(home in str(value) for value in env.values())


_cg = ctypes.CDLL("/System/Library/Frameworks/CoreGraphics.framework/CoreGraphics")
_cf = ctypes.CDLL("/System/Library/Frameworks/CoreFoundation.framework/CoreFoundation")
_cg.CGWindowListCopyWindowInfo.restype, _cg.CGWindowListCopyWindowInfo.argtypes = ctypes.c_void_p, [ctypes.c_uint32, ctypes.c_uint32]
_cf.CFArrayGetCount.restype, _cf.CFArrayGetCount.argtypes = ctypes.c_long, [ctypes.c_void_p]
_cf.CFArrayGetValueAtIndex.restype, _cf.CFArrayGetValueAtIndex.argtypes = ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_long]
_cf.CFDictionaryGetValue.restype, _cf.CFDictionaryGetValue.argtypes = ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_void_p]
_cf.CFStringCreateWithCString.restype, _cf.CFStringCreateWithCString.argtypes = ctypes.c_void_p, [ctypes.c_void_p, ctypes.c_char_p, ctypes.c_uint32]
_cf.CFNumberGetValue.restype, _cf.CFNumberGetValue.argtypes = ctypes.c_bool, [ctypes.c_void_p, ctypes.c_int, ctypes.c_void_p]
_cg.CGRectMakeWithDictionaryRepresentation.restype = ctypes.c_bool
_cf.CFRelease.argtypes = [ctypes.c_void_p]


class _CGRect(ctypes.Structure):
    _fields_ = [("x", ctypes.c_double), ("y", ctypes.c_double), ("w", ctypes.c_double), ("h", ctypes.c_double)]


_cg.CGRectMakeWithDictionaryRepresentation.argtypes = [ctypes.c_void_p, ctypes.POINTER(_CGRect)]


def window_owner(number):
    """(owner pid, width, height in points) of window `number`, from the window server
    (not from the app's own state file); None when there is no such window. The size is
    the one screencapture captures. Needs no screen recording permission."""
    info = _cg.CGWindowListCopyWindowInfo(1 << 3, int(number))  # kCGWindowListOptionIncludingWindow
    if not info:
        return None
    keys = [_cf.CFStringCreateWithCString(None, k, 0x08000100) for k in (b"kCGWindowOwnerPID", b"kCGWindowBounds")]
    try:
        if _cf.CFArrayGetCount(info) != 1:
            return None
        window = _cf.CFArrayGetValueAtIndex(info, 0)
        value, bounds = (_cf.CFDictionaryGetValue(window, k) for k in keys)
        pid, rect = ctypes.c_int32(), _CGRect()
        if not (value and bounds and _cf.CFNumberGetValue(value, 3, ctypes.byref(pid))  # kCFNumberSInt32Type
                and _cg.CGRectMakeWithDictionaryRepresentation(bounds, ctypes.byref(rect))):
            return None
        return pid.value, rect.w, rect.h
    finally:
        for k in keys:
            _cf.CFRelease(k)
        _cf.CFRelease(info)


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


def since(rows, seconds):
    """The state `seconds` after launch (the state file's own clock)."""
    if not rows:
        return {}
    first = rows[0][0] - float(rows[0][1].get("since_launch", 0))
    return at(rows, first + seconds) or {}


def defaults_read(suite):
    """The suite's plist as a dict ({} when it does not exist)."""
    path = suite + ".plist"
    return plistlib.loads(Path(path).read_bytes()) if os.path.exists(path) else {}


# QA18 ------------------------------------------------------------------------

OPEN_AT, STEP, CLOSE_AT, QUIT_AT = 3, 1.5, 12.5, 15


def settings_window(binary, out):
    ok = True
    for language, title in (("zh_CN.UTF-8", "设置…"), ("en_US.UTF-8", "Settings…")):
        ok &= without_input(lambda attempt, l=language, t=title: settings_once(binary, out, l, t, attempt), "QA18")
    return ok


def settings_once(binary, out, language, title, attempt):
    base = root("wp11s-")
    state = f"{base}/state"
    env = env_for(base, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=state, LC_ALL=language,
                  WHOATE_UI_OPEN_SETTINGS_AFTER=OPEN_AT, WHOATE_UI_SETTINGS_TAB_STEP=STEP,
                  WHOATE_UI_SETTINGS_CLOSE_AFTER=CLOSE_AT, WHOATE_UI_QUIT_AFTER=QUIT_AT)
    handle, exited = run(binary, env, QUIT_AT + 20)
    rows = states(state)
    opened = since(rows, OPEN_AT + 1)
    closed = since(rows, CLOSE_AT + 1.5)
    tabs = []
    for _, row in rows:
        if row.get("settings_visible") == "true" and (not tabs or tabs[-1] != row.get("settings_tab")):
            tabs.append(row.get("settings_tab"))
    menu = next((r.get("app_menu_settings") for _, r in reversed(rows) if r.get("app_menu_settings", "-") != "-"), "-")
    checks = {
        "qa18_open_visible_key": opened.get("settings_visible") == "true" and opened.get("settings_key") == "true",
        "qa18_open_regular_front": opened.get("policy") == "regular" and opened.get("frontmost") == "self",
        "qa18_not_a_main_window": opened.get("main_windows") == "0" and opened.get("settings_windows") == "1",
        "qa18_every_tab_shown": [t for t in TABS if t in tabs] == TABS,
        "qa18_closed_accessory": closed.get("settings_visible") == "false" and closed.get("policy") == "accessory",
        "qa18_menu_item": menu == f"{title}|cmd+,",
        "qa18_fake_backends_and_suite": opened.get("login_backend") == "fake" and opened.get("notify_backend") == "fake"
        and opened.get("store") == "suite",
        "quit_after_exited": exited,
    }
    name = f"qa18-settings-{language[:2]}.json"
    write_receipt(out, name, {"pid": handle["pid"], "attempt": attempt, "tabs": tabs, "menu": menu,
                              "opened": opened, "closed": closed, "checks": checks, "states": rows})
    for key, value in checks.items():
        print(f"P11 {language[:2]} {key}={value}")
    return all(checks.values()), os.path.join(out, name)


# QA19 ------------------------------------------------------------------------

HIDDEN_RUN = 130


def persist(binary, out):
    base = root("wp11p-")
    first_state, second_state, log = f"{base}/state1", f"{base}/state2", f"{base}/schedule"
    env = env_for(base, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=first_state, WHOATE_UI_QUIT_AFTER=5,
                  WHOATE_UI_SET="appearance=dark,language=en,menuBarDisplay=iconOnly,refreshProfile=powerSaving")
    _, first_exited = run(binary, env, 30)
    saved = defaults_read(f"{base}/suite")
    env = env_for(base, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=second_state,
                  WHOATE_UI_SCHEDULE_LOG=log, WHOATE_UI_QUIT_AFTER=HIDDEN_RUN)
    _, second_exited = run(binary, env, HIDDEN_RUN + 30)
    second = states(second_state)
    last = second[-1][1] if second else {}
    events = schedule(log)
    gaps = lambda mode: [b - a for a, b in zip(*(lambda t: (t, t[1:]))([w for w, m in events if m == mode]))]
    host, full = gaps("host"), gaps("hidden-full")
    checks = {
        "qa19_saved_in_suite": saved.get("appearance") == "dark" and saved.get("language") == "en"
        and saved.get("menuBarDisplay") == "iconOnly" and saved.get("refreshProfile") == "powerSaving",
        "qa19_read_back": last.get("appearance") == "NSAppearanceNameDarkAqua" and last.get("language") == "en"
        and last.get("menubar_text") == "" and last.get("profile") == "powerSaving" and last.get("store") == "suite",
        "qa19_host_every_10s": bool(host) and 9 <= statistics.median(host) <= 11,
        "qa19_full_every_60s": bool(full) and 55 <= statistics.median(full) <= 65,
        "first_exited": first_exited, "second_exited": second_exited,
    }
    write_receipt(out, "qa19-persist.json", {"saved": saved, "last": last, "host_gaps": host, "full_gaps": full,
                                              "checks": checks})
    print(f"QA19 host_gaps_median={statistics.median(host) if host else None} "
          f"full_gaps={[round(g, 1) for g in full]}")
    for key, value in checks.items():
        print(f"P11 {key}={value}")
    return all(checks.values())


# QA20 ------------------------------------------------------------------------

def first_launch(binary, out):
    return without_input(lambda attempt: first_launch_once(binary, out, attempt), "QA20")


def first_launch_once(binary, out, attempt, shot=None):
    results = {}
    base = root("wp11f-")
    for name, flag, quit_at in (("first", "1", 8), ("again", "1", 6)):
        state = f"{base}/state-{name}"
        env = env_for(base, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=state, WHOATE_UI_QUIT_AFTER=quit_at,
                      WHOATE_UI_FIRST_LAUNCH=flag)
        if name == "first" and shot:
            env.update(shot["env"])
            shot["seen_env"] = env
        runner = launch.Launcher()
        handle = runner.start(binary, env=env)
        try:
            if name == "first" and shot:
                shot["capture"](state, handle)
            exited = runner.wait(handle, quit_at + 20)
        finally:
            if runner.alive(handle):
                handle["stop"] = runner.stop(handle)
        launch.cleanup(handle)
        rows = [r for _, r in states(state)]
        results[name] = {"rows": rows, "exited": exited, "main_max": max((int(r.get("main_windows", 0)) for r in rows), default=0),
                         "hints": sorted({r.get("hint") for r in rows}), "done": defaults_read(f"{base}/suite").get("firstLaunchDone")}
    control = root("wp11c-")
    state = f"{control}/state"
    env = env_for(control, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=state, WHOATE_UI_QUIT_AFTER=6)
    _, exited = run(binary, env, 26)
    rows = [r for _, r in states(state)]
    results["control"] = {"rows": rows, "exited": exited, "main_max": max((int(r.get("main_windows", 0)) for r in rows), default=0),
                          "hints": sorted({r.get("hint") for r in rows})}
    first, again, ctl = results["first"], results["again"], results["control"]
    checks = {
        "qa20_first_opens_main": first["main_max"] == 1,
        "qa20_first_hint": bool({"statusItem", "banner"} & set(first["hints"])),
        "qa20_first_saved": first["done"] is True,
        "qa20_again_no_main": again["main_max"] == 0 and again["hints"] == ["-"],
        "qa20_control_no_main": ctl["main_max"] == 0 and ctl["hints"] == ["-"],
        "exited": first["exited"] and again["exited"] and ctl["exited"],
    }
    name = "qa20-first-launch.json"
    write_receipt(out, name, {"attempt": attempt, "results": results, "checks": checks})
    for key, value in checks.items():
        print(f"P11 {key}={value}")
    return all(checks.values()), os.path.join(out, name)


# QA27 ------------------------------------------------------------------------

def notify(binary, out):
    return without_input(lambda attempt: notify_once(binary, out, attempt), "QA27")


def notify_once(binary, out, attempt):
    base = root("wp11n-")
    report = json.loads(DESIGN.read_text())
    growing = [o["id"] for o in report["owners"] if o["verdict"]["rule"] in ("fast-growth", "leak-suspect")]
    keep = growing[0]
    # One growing row: the others read as ordinary for this run.
    for owner in report["owners"]:
        if owner["id"] in growing[1:]:
            owner["verdict"] = dict(owner["verdict"], rule="default", level="ok")
    report["host"]["pressure"] = "warn"
    fixture = f"{base}/notify-report.json"
    Path(fixture).write_text(json.dumps(report))
    suite = f"{base}/suite"
    for key in ("notifications.enabled", "notifications.pressure", "notifications.growing"):
        subprocess.run(["/usr/bin/defaults", "write", suite, key, "-bool", "true"], check=True)
    state, log = f"{base}/state", f"{base}/notify-log"
    env = env_for(base, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=state, WHOATE_UI_NOTIFY_LOG=log,
                  WHOATE_UI_NOTIFY_CLICK_AFTER=6, WHOATE_UI_QUIT_AFTER=10)
    env["WHOATE_UI_FIXTURE"] = fixture
    handle, exited = run(binary, env, 30)
    lines = Path(log).read_text().splitlines() if os.path.exists(log) else []
    posts = [l.split("\t") for l in lines if l.startswith("post\t")]
    rows = states(state)
    clicked = since(rows, 8)
    checks = {
        "qa27_one_pressure": sum(p[1] == "pressure" for p in posts) == 1,
        "qa27_one_growing": [p[2] for p in posts if p[1] == "growing"] == [keep],
        "qa27_click_logged": f"click\tgrowing\t{keep}" in lines,
        "qa27_click_opens_row": clicked.get("main_windows") == "1" and clicked.get("selected") == keep,
        "qa27_fake_backend": clicked.get("notify_backend") == "fake",
        "qa27_text_safe": all("/" not in p[3] + p[4] and "http" not in p[3] + p[4] for p in posts),
        "quit_after_exited": exited,
    }
    name = "qa27-notify.json"
    write_receipt(out, name, {"pid": handle["pid"], "attempt": attempt, "log": lines, "clicked": clicked,
                              "checks": checks})
    for key, value in checks.items():
        print(f"P11 {key}={value}")
    return all(checks.values()), os.path.join(out, name)


# QA22 ------------------------------------------------------------------------

def shoot(number, png):
    shot = subprocess.run(["/usr/sbin/screencapture", f"-l{number}", "-x", "-o", png], capture_output=True, text=True)
    return shot.returncode == 0 and os.path.exists(png), shot.stderr.strip()


HELD_READS, HELD_INTERVAL = 5, 0.1


def held(number, owner=window_owner, sleep=time.sleep, reads=HELD_READS, interval=HELD_INTERVAL, timeout=5):
    """(width, height) once the window server reports the same size `reads` times in a row,
    `interval` s apart; None when it never holds within `timeout` s or the window goes.
    The app's own report can lag a resize, and a page first laid out under the toolbar
    was 11 pt too tall until a later update (settingsPageResized)."""
    run = []
    for _ in range(int(timeout / interval)):
        found = owner(number)
        if not found:
            return None
        run = run + [found[1:]] if run and run[-1] == found[1:] else [found[1:]]
        if len(run) >= reads:
            return run[-1]
        sleep(interval)
    return None


def capture(number, size, pid, png, slack=0, owner=window_owner, take=shoot, measure=screenshots.stats, sleep=time.sleep):
    """One window this run's app owns, by number; screenshots.py's image checks.
    The window must belong to `pid` by the window server's account, checked before
    anything is captured, so another app's window is never photographed. Its size must
    hold before the capture (held) and still be the same after it.
    `slack` points: a popover's capture includes its arrow beyond the window frame."""
    found = owner(number)
    owner_pid = found[0] if found else None
    if owner_pid != pid:
        return {"file": png, "window": int(number), "pid": pid, "owner_pid": owner_pid, "owner_ok": False, "ok": False,
                "error": "window missing" if owner_pid is None else "window belongs to another process"}
    size_held = held(number, owner=owner, sleep=sleep)
    if size_held is None:
        return {"file": png, "window": int(number), "pid": pid, "owner_pid": owner_pid, "owner_ok": True, "ok": False,
                "error": f"window size did not hold for {HELD_READS} reads"}
    width, height = size_held
    scale = size.split("x")[-1]
    taken, error = take(number, png)
    if not taken:
        return {"file": png, "error": error, "owner_pid": owner_pid, "owner_ok": True, "ok": False}
    after = owner(number)
    s = measure(png)
    expected = (round(int(width) * float(scale)), round(int(height) * float(scale)))
    s.update(file=png, window=int(number), pid=pid, owner_pid=owner_pid, owner_ok=True, expected_size=expected,
             size_after=list(after[1:]) if after else None, held_after=bool(after) and after[1:] == size_held,
             not_solid=s["distinct_colours"] > 8)
    s["size_ok"] = s["held_after"] and all(abs(got - want) <= slack * float(scale)
                                           for got, want in zip((s["width"], s["height"]), expected))
    s["ok"] = s["size_ok"] and s["not_solid"]
    return s


def wait_state(state, test, timeout=20):
    deadline = time.time() + timeout
    while time.time() < deadline:
        rows = states(state)
        if rows and test(rows[-1][1]):
            return rows[-1][1]
        time.sleep(0.2)
    return None


def steady(state, field, settle=0.6, timeout=10):
    """The latest state once `field` has kept one value for `settle` seconds: a tab switch
    animates the window's height, and a size read mid-animation is not the captured one."""
    deadline = time.time() + timeout
    value, since = None, time.time()
    while time.time() < deadline:
        row = states(state)[-1][1]
        if row.get(field) != value:
            value, since = row.get(field), time.time()
        elif time.time() - since >= settle:
            return row
        time.sleep(0.1)
    return states(state)[-1][1]


TAB_STEP = 2.5


def shot_assets(cli):
    """Everything the app is shown lives under the temporary root, never the owner's home:
    the CLI path appears on the command line tab, and the shots may be published (P06)."""
    assets = root("wp11assets-")
    shutil.copy2(cli, f"{assets}/whoate")
    shutil.copy2(DESIGN, f"{assets}/design-report.json")
    return f"{assets}/whoate", f"{assets}/design-report.json"


def settings_env(cli, fixture, language, appearance, tab=None):
    """The settings window at 2 s, then every tab TAB_STEP s apart; or only `tab`."""
    base = root("wp11shot-")
    os.makedirs(f"{base}/bin", exist_ok=True)
    tabs = {"WHOATE_UI_SETTINGS_TAB": tab} if tab else {"WHOATE_UI_SETTINGS_TAB_STEP": TAB_STEP}
    return env_for(base, fixture, WHOATE_UI_AUTOMATION="menubar", WHOATE_UI_STATE_FILE=f"{base}/state", LC_ALL=language,
                   WHOATE_UI_APPEARANCE=appearance, WHOATE_UI_OPEN_SETTINGS_AFTER=2, WHOATE_UI_CLI_BUNDLED=cli,
                   WHOATE_UI_QUIT_AFTER=2 + (3 if tab else TAB_STEP * 6) + 3, **tabs)


def shots(binary, out, cli):
    results, envs = [], []
    cli, fixture = shot_assets(cli)
    for language in ("zh_CN.UTF-8", "en_US.UTF-8"):
        for appearance in ("light", "dark"):
            env = settings_env(cli, fixture, language, appearance)
            state = env["WHOATE_UI_STATE_FILE"]
            envs.append(env)
            runner = launch.Launcher()
            handle = runner.start(binary, env=env)
            try:
                for tab in TABS:
                    row = wait_state(state, lambda r, t=tab: r.get("settings_tab") == t and r.get("settings_visible") == "true")
                    png = f"{out}/settings-{tab}-{language[:2]}-{appearance}.png"
                    if row is None:
                        results.append({"file": png, "ok": False, "error": "settings tab not shown"})
                        continue
                    row = steady(state, "settings_size")
                    results.append(dict(capture(row["settings_window"], row["settings_size"], handle["pid"], png),
                                        tab=tab, language=language[:2], appearance=appearance))
                runner.wait(handle, 30)
            finally:
                if runner.alive(handle):
                    runner.stop(handle)
            launch.cleanup(handle)
    # The panel's footer with the gear, light and dark (the panel window entry).
    for appearance in ("light", "dark"):
        base = root("wp11panel-")
        window_file = f"{base}/window"
        env = env_for(base, fixture, WHOATE_UI_AUTOMATION="panel-window", WHOATE_UI_APPEARANCE=appearance,
                      WHOATE_UI_WINDOW_FILE=window_file, LC_ALL="zh_CN.UTF-8", WHOATE_UI_QUIT_AFTER=12)
        envs.append(env)
        runner = launch.Launcher()
        handle = runner.start(binary, env=env)
        try:
            for _ in range(150):
                if os.path.exists(window_file):
                    break
                time.sleep(0.1)
            time.sleep(1)
            _, number, width, height, scale, _, _ = Path(window_file).read_text().rstrip("\n").split("\t")
            results.append(dict(capture(number, f"{width}x{height}x{scale}", handle["pid"], f"{out}/panel-gear-{appearance}.png"),
                                tab="panel", language="zh", appearance=appearance))
        finally:
            if runner.alive(handle):
                runner.stop(handle)
        launch.cleanup(handle)
    # The first-launch hint, from a first launch of its own.
    def hint_capture(state, handle):
        row = wait_state(state, lambda r: r.get("hint_window", "-") != "-" or r.get("hint") == "banner")
        png = f"{out}/first-launch-hint.png"
        if row is None:
            results.append({"file": png, "ok": False, "error": "no hint"})
            return
        if row.get("hint") == "banner":
            # No menu bar item on screen (research F11): the hint is the main window's banner.
            row = wait_state(state, lambda r: r.get("main_size", "-") != "-") or {}
            number, *size = row.get("main_size", "0x0x0x1").split("x")
        else:
            # The popover's window is laid out after it is first reported (346 pt square, then its size).
            row = steady(state, "hint_window")
            number, *size = row["hint_window"].split("x")
        slack = 0 if row.get("hint") == "banner" else 4
        results.append(dict(capture(number, "x".join(size), handle["pid"], png, slack), tab="hint", language="zh",
                            appearance="light", hint=row.get("hint")))
    hint = {"env": {"LC_ALL": "zh_CN.UTF-8", "WHOATE_UI_APPEARANCE": "light", "WHOATE_UI_FIXTURE": fixture},
            "capture": hint_capture}
    first_launch_once(binary, out, 0, shot=hint)
    envs.append(hint["seen_env"])
    by = {(r.get("tab"), r.get("language"), r.get("appearance")): r for r in results}
    for r in results:
        light = by.get((r.get("tab"), r.get("language"), "light"))
        if r.get("appearance") == "dark" and light and "mean_luma" in r and "mean_luma" in light:
            r["darker_than_light"] = r["mean_luma"] < light["mean_luma"] - 40
            r["ok"] = r["ok"] and r["darker_than_light"]
    home_ok = all(home_free(env) for env in envs)
    write_receipt(out, "qa22-screenshots.json", {"shots": results, "app_env_home_free": home_ok, "launches": len(envs)})
    for r in results:
        print(f"QA22 {os.path.basename(r['file'])} ok={r['ok']} owner_ok={r.get('owner_ok')} {r.get('error', '')}")
    count_ok = len([r for r in results if r["ok"]]) == 23 and len(results) == 23
    print(f"QA22 shots={len(results)} ok={sum(r['ok'] for r in results)} launches={len(envs)} app_env_home_free={home_ok}")
    return count_ok and home_ok


def dwell_heights(samples, skip=0.3, tail=0.4):
    """{tab: sorted window heights} while each tab is shown, from (time, tab, height)
    samples in order. Left out: the first `skip` s of a tab (the switch's own resize) and
    its last `tail` s (the state file reports a switch up to 0.2 s after the window moves)."""
    spans = []
    for t, tab, height in samples:
        if not spans or spans[-1][0] != tab:
            spans.append((tab, []))
        spans[-1][1].append((t, height))
    heights = {}
    for tab, rows in spans:
        start, end = rows[0][0] + skip, rows[-1][0] - tail
        heights.setdefault(tab, set()).update(h for t, h in rows if start <= t <= end)
    return {tab: sorted(hs) for tab, hs in heights.items()}


def settled_verdict(dwell, direct):
    """Every tab shown at one height for its whole dwell, and notifications at the height it
    has when the window opens on it directly. Before settingsPageResized, notifications
    opened 11 pt too tall and either stayed so or dropped 0.6-0.8 s later (QA22 red twice)."""
    return all(len(dwell.get(tab, [])) == 1 for tab in TABS) and dwell["notifications"] == [direct]


def settle(binary, out, cli):
    """QA22: a tab switch shows the page at its final height at once. Per language (light):
    every tab in turn, the window server's height sampled every 30 ms; and a window opened
    on notifications directly, for that tab's reference height."""
    cli, fixture = shot_assets(cli)
    report, ok = {}, True
    for language in ("zh_CN.UTF-8", "en_US.UTF-8"):
        samples, direct = [], None
        for tab in (None, "notifications"):
            env = settings_env(cli, fixture, language, "light", tab)
            state = env["WHOATE_UI_STATE_FILE"]
            runner = launch.Launcher()
            handle = runner.start(binary, env=env)
            try:
                row = wait_state(state, lambda r: r.get("settings_visible") == "true" and r.get("settings_window", "-") != "-")
                number = row and row["settings_window"]
                # The app reports the window a moment before the window server lists it;
                # never listed as this app's within 5 s: no samples, no reference (red).
                for _ in range(100):
                    if not number or (window_owner(number) or [None])[0] == handle["pid"]:
                        break
                    time.sleep(0.05)
                else:
                    number = None
                if number and tab:
                    size = held(number)
                    direct = size and size[1]
                # Past the app's own quit time plus 10 s the run is over (a hung app: red).
                deadline = time.time() + float(env["WHOATE_UI_QUIT_AFTER"]) + 10
                while number and runner.alive(handle) and not tab:
                    if time.time() > deadline:
                        samples.clear()
                        break
                    rows = states(state)
                    found = window_owner(number)
                    if not found or found[0] != handle["pid"]:
                        break
                    samples.append((time.time(), rows[-1][1].get("settings_tab"), found[2]))
                    time.sleep(0.03)
                runner.wait(handle, 30)
            finally:
                if runner.alive(handle):
                    runner.stop(handle)
            launch.cleanup(handle)
        dwell = dwell_heights(samples)
        verdict = direct is not None and settled_verdict(dwell, direct)
        ok &= verdict
        report[language[:2]] = {"dwell_heights": dwell, "notifications_direct": direct, "ok": verdict,
                                "samples": [[round(t - samples[0][0], 3), tab, h] for t, tab, h in samples] if samples else []}
        print(f"QA22 settle {language[:2]} dwell_heights={dwell} notifications_direct={direct} ok={verdict}")
    write_receipt(out, "qa22-settle.json", report)
    return ok


# QA23 ------------------------------------------------------------------------

BTM_DIR = "/private/var/db/com.apple.backgroundtaskmanagement"


def btm_entries(directory=BTM_DIR):
    """(files read, [file, identifier, disposition, url] of every login item / background
    task whose identifier names whoate). The databases are world-readable keyed archives;
    `sfltool dumpbtm` reads the same records but asks the owner for Touch ID or a password
    (coreautha, 2026-10-27 01:15), so it is never called."""
    read, entries = 0, []
    for path in sorted(glob.glob(f"{directory}/BackgroundItems-v*.btm")):
        try:
            objects = plistlib.loads(Path(path).read_bytes()).get("$objects", [])
        except (OSError, plistlib.InvalidFileException, ValueError):
            continue
        read += 1

        def value(item, key):
            v = item.get(key)
            for _ in range(3):
                if isinstance(v, plistlib.UID):
                    v = objects[v.data]
                elif isinstance(v, dict) and ("NS.string" in v or "NS.relative" in v):
                    v = v.get("NS.string", v.get("NS.relative"))
                else:
                    break
            return v if isinstance(v, (str, int)) else None

        for item in objects:
            if isinstance(item, dict) and "disposition" in item:
                identifier = value(item, "identifier")
                if identifier and "zhenninglang.whoate" in identifier:
                    entries.append([os.path.basename(path), identifier, value(item, "disposition"), value(item, "url")])
    return read, sorted(entries)


def owner_state():
    """What the owner's machine says about whoate's settings, login items and PATH links."""
    export = subprocess.run(["/usr/bin/defaults", "export", OWNER_DOMAIN, "-"], capture_output=True)
    domain = plistlib.loads(export.stdout) if export.returncode == 0 and export.stdout else {}
    links = {}
    for path in REAL_LINKS:
        links[path] = os.readlink(path) if os.path.islink(path) else ("file" if os.path.exists(path) else "-")
    prefs = os.path.expanduser("~/Library/Preferences")
    btm_read, btm = btm_entries()
    return {"settings": {k: domain.get(k) for k in SETTING_KEYS if k in domain}, "links": links,
            "prefs": sorted(os.listdir(prefs)), "btm_read": btm_read, "btm": btm}


def owner_snapshot(path):
    Path(path).write_text(json.dumps(owner_state(), sort_keys=True))
    print(f"QA23 snapshot {path}")
    return True


def owner_compare(path):
    before, after = json.loads(Path(path).read_text()), owner_state()
    prefs = os.path.expanduser("~/Library/Preferences")
    with_keys = []
    for name in sorted(set(after["prefs"]) - set(before["prefs"])):
        try:
            data = plistlib.loads(Path(prefs, name).read_bytes())
        except Exception:
            continue
        if isinstance(data, dict) and any(k in data for k in SETTING_KEYS):
            with_keys.append(name)
    checks = {
        "qa23_owner_settings_unchanged": before["settings"] == after["settings"],
        "qa23_links_unchanged": before["links"] == after["links"],
        "qa23_no_new_plist_with_setting_keys": not with_keys,
        # QA23.2: whoate's login item records (count, disposition, URL), read both times.
        "qa23_btm_read": before.get("btm_read", 0) > 0 and after["btm_read"] == before.get("btm_read"),
        "qa23_btm_unchanged": before.get("btm") == after["btm"],
    }
    for key, value in checks.items():
        print(f"P11 {key}={value}")
    if with_keys:
        print(f"QA23 new plists with setting keys: {with_keys}")
    print(f"QA23 links={after['links']} owner_setting_keys={sorted(after['settings'])}")
    print(f"QA23 btm_files_read={after['btm_read']} whoate_entries={[(e[1], e[2]) for e in after['btm']]}")
    return all(checks.values())


def main(argv):
    command = argv[1]
    if command == "owner-snapshot":
        return owner_snapshot(argv[2])
    if command == "owner-compare":
        return owner_compare(argv[2])
    binary, out = argv[2], argv[3]
    os.makedirs(out, exist_ok=True)
    if command == "shots":
        return shots(binary, out, argv[4])
    if command == "settle":
        return settle(binary, out, argv[4])
    return {"settings-window": settings_window, "persist": persist, "first-launch": first_launch,
            "notify": notify}[command](binary, out)


if __name__ == "__main__":
    sys.exit(0 if main(sys.argv) else 1)
