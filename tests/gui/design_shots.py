#!/usr/bin/env python3
"""Phase 10 design gate: renders the real SwiftUI views against
tests/gui/design-report.json and captures whoate's own windows.

The debug Whoate executable (the only build that replays a report) is started
through launch.py (`open -n`, isolated, never a bare exec) with an explicit
environment: no WHOATE_UI_* is inherited from the caller. Each shot captures
only the window number the app itself wrote, with `screencapture -l<id> -x -o`.

Shots, each in light and dark: menu bar item gallery, panel, panel in CPU
mode (D4), main window on the agent session, the orphan, the browser and the
overview; the agent screen and the panel also in English (light). Round 4
adds the agent's process tree and Brave's CPU curve, scrolled into view.

The report's taken_at / as_of are moved to now before each run, so the footer
age reads like a live one; owner data and history are unchanged.

  design_shots.py <debug Whoate executable> <out dir> [--strict]
Exit 0: every PNG exists, is not one colour, and each dark shot is darker
than its light one. --strict (QA21) also needs dark at least 40 darker, the
PNG size equal to the window size times its scale, and each main window shot
showing an owner of the kind it asked for. Exit 77: screen recording denied
(report `blocked screen-recording-permission`).
"""
import datetime, json, os, subprocess, sys, tempfile, time
import launch
from screenshots import stats

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FIXTURE = os.path.join(ROOT, "tests/gui/design-report.json")
ICON = os.path.join(ROOT, "assets/logo/menubar-template.pdf")
ZH, EN = "zh_CN.UTF-8", "en_US.UTF-8"

# name, window kind, automation mode, extra env, seconds to settle after the window line
SHOTS = [
    ("menubar-gallery", "gallery", "design-gallery", {}, 1.5),
    ("panel", "panel", "panel-window", {}, 1.0),
    ("panel-cpu", "panel", "panel-window", {"WHOATE_UI_SORT": "cpu"}, 1.0),
    ("main-agent", "main", "main-window", {"WHOATE_UI_SELECT": "kind:agent"}, 5.0),
    ("main-orphan", "main", "main-window", {"WHOATE_UI_SELECT": "kind:orphan"}, 5.0),
    ("main-browser", "main", "main-window", {"WHOATE_UI_SELECT": "kind:browser"}, 5.0),
    ("main-overview", "main", "main-window", {"WHOATE_UI_SELECT": "overview"}, 5.0),
    # Round 3 (R3): a pane running two agent sessions.
    ("main-pane", "main", "main-window", {"WHOATE_UI_SELECT": "pane:%22"}, 5.0),
    # Round 4: the process tree (R10) and the CPU curve with readable-only CPU (R11, R12),
    # scrolled into view in the details.
    ("main-agent-tree", "main", "main-window", {"WHOATE_UI_SELECT": "kind:agent", "WHOATE_UI_DETAIL_SCROLL": "relation"}, 6.0),
    ("main-browser-cpu", "main", "main-window", {"WHOATE_UI_SELECT": "kind:browser", "WHOATE_UI_DETAIL_SCROLL": "cpu"}, 6.0),
]
ENGLISH = {"panel", "main-agent"}
# Owner id prefix (or test) the selected row must have, per main window shot.
SELECTED = {"main-agent": lambda i: i.startswith("agent:"), "main-orphan": lambda i: i.startswith("orphan:"),
            "main-browser": lambda i: i == "app:com.brave.Browser", "main-overview": lambda i: i == "-",
            "main-pane": lambda i: i == "pane:%22", "main-agent-tree": lambda i: i.startswith("agent:"),
            "main-browser-cpu": lambda i: i == "app:com.brave.Browser"}


def fresh_fixture(path):
    """A copy of the design report with taken_at and as_of set to now."""
    report = json.load(open(FIXTURE))
    now = datetime.datetime.now().astimezone().replace(microsecond=0).isoformat()
    report["taken_at"] = report["system_procs"]["as_of"] = now
    for key in ("kernel_zones", "gpu_memory"):
        report["host"][key]["as_of"] = now
    json.dump(report, open(path, "w"), ensure_ascii=False)


def capture(binary, out_dir, fixture, shot, appearance, language):
    name, kind, mode, extra, settle = shot
    window_file = tempfile.mktemp(prefix="whoate-design-window-")
    state_file = window_file + ".state"
    env = dict(WHOATE_UI_AUTOMATION=mode, WHOATE_UI_APPEARANCE=appearance, WHOATE_UI_WINDOW_FILE=window_file,
               WHOATE_UI_STATE_FILE=state_file, WHOATE_UI_FIXTURE=fixture, WHOATE_UI_MENUBAR_ICON=ICON,
               LC_ALL=language, **extra)
    launcher = launch.Launcher()
    app = launcher.start(binary, env=env)
    suffix = "" if language == ZH else "-en"
    png = os.path.join(out_dir, f"{name}{suffix}-{appearance}.png")
    try:
        line = None
        for _ in range(300):
            if os.path.exists(window_file):
                line = next((l for l in open(window_file).read().splitlines() if l.startswith(kind + "\t")), None)
                if line:
                    break
            time.sleep(0.1)
        if line is None:
            raise SystemExit(f"DESIGN_WINDOW_FILE_MISSING {name} {appearance}")
        _, number, width, height, scale, pid, _ = line.split("\t")
        if int(pid) != app["pid"]:
            raise SystemExit(f"DESIGN_WINDOW_NOT_OURS {name} pid={pid} launched={app['pid']}")
        time.sleep(settle)
        shot_run = subprocess.run(["/usr/sbin/screencapture", f"-l{number}", "-x", "-o", png],
                                  capture_output=True, text=True)
        if shot_run.returncode != 0 or not os.path.exists(png):
            print(f"DESIGN screencapture failed: {shot_run.stderr.strip()}")
            sys.exit(77)
        s = stats(png)
        lines = open(state_file).read().splitlines() if os.path.exists(state_file) else []
        selected = next((f.split("=", 1)[1] for f in reversed(lines[-1].split("\t")) if f.startswith("selected=")), None) \
            if lines else None
        # The window line truncates points to integers (559.5 → 559): a capture of a
        # fractional-width window is up to one point wider, which shows at 1× scale.
        fits = lambda pixels, points: round(int(points) * float(scale)) <= pixels <= round((int(points) + 1) * float(scale))
        s.update(selected=selected, size_ok=fits(s["width"], width) and fits(s["height"], height))
        s.update(name=name + suffix, appearance=appearance, language=language, file=os.path.basename(png),
                 window=int(number), pid=app["pid"], window_points=[int(width), int(height)], scale=float(scale),
                 not_solid=s["distinct_colours"] > 8,
                 source="tests/gui/design-report.json (fictional), debug build replay, timestamps moved to now")
        return s
    finally:
        s_stop = launcher.stop(app)
        launch.cleanup(app)
        for path in (window_file, state_file):
            if os.path.exists(path):
                os.remove(path)
        print(f"DESIGN stop {name}{suffix}-{appearance} pid={app['pid']} {s_stop}")


def main():
    binary, out_dir = sys.argv[1], sys.argv[2]
    strict = "--strict" in sys.argv[3:]
    os.makedirs(out_dir, exist_ok=True)
    fixture = os.path.join(tempfile.mkdtemp(prefix="whoate-design-"), "design-report.json")
    fresh_fixture(fixture)
    runs = [(shot, appearance, ZH) for shot in SHOTS for appearance in ("light", "dark")]
    runs += [(shot, "light", EN) for shot in SHOTS if shot[0] in ENGLISH]
    results = [capture(binary, out_dir, fixture, *run) for run in runs]
    by = {(r["name"], r["appearance"]): r for r in results}
    ok = all(r["not_solid"] for r in results)
    for r in results:
        light = by.get((r["name"], "light"))
        if r["appearance"] == "dark" and light:
            r["darker_than_light"] = r["mean_luma"] < light["mean_luma"] - (40 if strict else 0)
            ok = ok and r["darker_than_light"]
        test = SELECTED.get(r["name"].removesuffix("-en"))
        r["selection_ok"] = test is None or (r["selected"] is not None and test(r["selected"]))
        if strict:
            ok = ok and r["selection_ok"] and r["size_ok"]
    json.dump(results, open(os.path.join(out_dir, "shots.json"), "w"), indent=1, ensure_ascii=False)
    for r in results:
        print(f"DESIGN {r['file']} {r['width']}x{r['height']} not_solid={r['not_solid']} "
              f"luma={r['mean_luma']:.0f} darker={r.get('darker_than_light', '-')} size_ok={r['size_ok']} "
              f"selected={r['selected']} selection_ok={r['selection_ok']}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
