#!/usr/bin/env python3
"""QA15 negative control, in a throwaway coalition (orchestrator 2026-10-20).

NEGATIVE CONTROL — the only GUI process not started by launch.py: the release
executable runs bare, the old way, as a child process. Never as a child of
tmux: `open -n` starts ls_control_wrapper.c as a checked-in accessory app with
its own coalition, and the wrapper starts a `sleep` of its own plus the bare
app. Checks:

  - the coalition is new: not this test's and not any tmux server's, and no
    entry involved lists a tmux server (else stop at once and fail);
  - the bare app's entry is type UIElement: the accessory policy set first
    thing in main means no Dock exec icon even without Info.plist;
  - the checker reads the coalition of a real entry: with the owned sleep
    standing in for a tmux server, it flags the wrapper's entry;
  - the checker flags a real `exited-with-subordinates` whoate entry when the
    machine has one (it is only reported, never touched);
  - ending only this test's processes leaves no new entry behind.

Measured (Phase 08 HANDOFF): no public way found to make LaunchServices record
a terminal-style coalition for a bare app, so the post-exit residue itself
only appears under a terminal or tmux. Owner ruling 2026-10-20 16:14: that
case is covered only by real entries from this Mac in
lsappinfo-real-20261020.txt (test_launch_residue.RealSampleTests), not live.

Only pids this script started, re-identified by path and start time, are signalled.

  ls_negative_control.py <Whoate executable> --baseline <snapshot> --out <dir>
"""
import argparse, ctypes, json, os, plistlib, subprocess, sys, tempfile, time
from pathlib import Path
import launch, ls_residue

_libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
WRAPPER_SOURCE = Path(__file__).with_name("ls_control_wrapper.c")


def coalition(pid):
    """Resource coalition id (proc_pidinfo PROC_PIDCOALITIONINFO), or None."""
    buf = (ctypes.c_uint64 * 8)()
    return buf[0] if _libproc.proc_pidinfo(pid, 20, 0, buf, ctypes.sizeof(buf)) > 0 else None


def poll(predicate, timeout_s, step=0.5):
    deadline = time.monotonic() + timeout_s
    while True:
        value = predicate()
        if value or time.monotonic() > deadline:
            return value
        time.sleep(step)


def read_pid(path, timeout_s=15):
    def value():
        text = open(path).read().strip() if os.path.exists(path) else ""
        return int(text) if text else None
    return poll(value, timeout_s, 0.1)


def owned(pid, path):
    """A handle for a process this script started; launch.Launcher signals it only while it still matches."""
    now = launch.process(pid)
    return {"pid": pid, "path": path, "start": now["start"]} if now and now["path"] == path else None


def wrapper_bundle(workdir):
    bundle = Path(workdir) / "WhoateNegativeControl.app"
    (bundle / "Contents/MacOS").mkdir(parents=True)
    executable = bundle / "Contents/MacOS/wrapper"
    subprocess.run(["/usr/bin/clang", "-Wall", "-Werror", "-o", str(executable), str(WRAPPER_SOURCE),
                    "-framework", "AppKit"], check=True, timeout=120)
    plist = {"CFBundleIdentifier": f"test.whoate.negative-control.{os.getpid()}", "CFBundleExecutable": "wrapper",
             "CFBundlePackageType": "APPL", "LSUIElement": True}
    (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(plist))
    return bundle, os.path.realpath(executable)


def main(argv=None):
    ap = argparse.ArgumentParser()
    ap.add_argument("binary")
    ap.add_argument("--baseline", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    baseline = json.load(open(a.baseline))
    work = tempfile.mkdtemp(prefix="wnc-", dir="/private/tmp")
    for name in ("home", "cache", "run"):
        os.mkdir(os.path.join(work, name))
    run = os.path.join(work, "run")
    binary = os.path.realpath(a.binary)
    env = {"HOME": os.path.join(work, "home"), "WHOATE_HOME_OVERRIDE": os.path.join(work, "home"),
           "WHOATE_CACHE_DIR": os.path.join(work, "cache"), "WHOATE_RULES": os.path.join(work, "rules.toml"),
           "TMUX_TMPDIR": work, "WHOATE_UI_AUTOMATION": "menubar", "WHOATE_UI_QUIT_AFTER": "4"}
    bundle, wrapper_path = wrapper_bundle(work)
    subprocess.run(launch.open_command(bundle, env, [run, binary], os.path.join(a.out, "negative-control.stdout"),
                                       os.path.join(a.out, "negative-control.stderr"), wait=False),
                   env=launch.launchd_env(), check=True, timeout=30)
    pids = {name: read_pid(os.path.join(run, f"{name}.pid")) for name in ("wrapper", "sleep", "app")}
    runner = launch.Launcher()
    handles = [owned(pids[k], p) for k, p in (("app", binary), ("sleep", "/bin/sleep"), ("wrapper", wrapper_path))
               if pids[k] is not None]
    handles = [h for h in handles if h]
    try:
        if len(handles) != 3:
            print(f"NEGATIVE_CONTROL_FAIL wrapper did not start {pids}")
            return 1
        return check(pids, handles, runner, baseline)
    finally:
        stops = {h["pid"]: runner.stop(h) for h in handles}
        gone = poll(lambda: not any(ls_residue.entry_for(p) for p in pids.values() if p), 15)
        print(f"NEGATIVE_CONTROL cleanup stops={stops} new_entries_gone={gone}")
        if not gone:
            print("NEGATIVE_CONTROL_FAIL entries left behind (not touched; report to orchestrator)")
            sys.exit(1)


def check(pids, handles, runner, baseline):
    app, sleeper, wrapper = handles
    tmux, failures = baseline["tmux_pids"], []
    place = {name: coalition(pid) for name, pid in pids.items()}
    own = coalition(os.getpid())
    print(f"NEGATIVE_CONTROL started pids={pids} coalitions={place} test_coalition={own} tmux_pids={tmux}")
    if len(set(place.values())) != 1 or own in place.values() or any(coalition(t) in place.values() for t in tmux):
        print("NEGATIVE_CONTROL_FAIL not a disposable coalition")
        return 1
    # The launch registers an entry first; the type is the app's own only after check-in.
    if not poll(lambda: "checkin time" in (ls_residue.entry_for(app["pid"]) or {}).get("raw", ""), 15):
        print(f"NEGATIVE_CONTROL_FAIL no checked-in entry for the bare app pid={app['pid']}")
        return 1
    time.sleep(1)
    entries = ls_residue.parse(ls_residue.lsappinfo())
    running, host = ls_residue.entry_for(app["pid"], entries), ls_residue.entry_for(wrapper["pid"], entries)
    if running is None or host is None:
        print(f"NEGATIVE_CONTROL_FAIL entries missing app={running is not None} wrapper={host is not None}")
        return 1
    print(f"NEGATIVE_CONTROL running app pid={app['pid']} type={running['type']} "
          f"wrapper coalition={host['coalition']} members={host['members']}")
    tied = [i for e in (running, host) for i in ls_residue.problems(e, tmux)]
    if tied:
        print(f"NEGATIVE_CONTROL_FAIL tied to a tmux server {tied}\n{running['raw']}\n{host['raw']}")
        return 1
    if running["type"] != "UIElement":
        runner.stop(app)
        failures.append(f"type={running['type']}: a Dock exec icon may be showing; do not force quit it")
    stand_in = ls_residue.problems(host, [sleeper["pid"]])
    print(f"NEGATIVE_CONTROL checker_on_live_coalition stand_in={sleeper['pid']} issues={stand_in}")
    if not any(i.startswith("coalition_contains_tmux") for i in stand_in):
        failures.append("checker missed a member of a real coalition")
    if not poll(lambda: not runner.alive(app), 20):
        failures.append("quit-after did not end the app")
    residue = [e for e in entries if ls_residue.is_whoate(e) and e["asn"] in set(baseline["asns"])
               and "exited-with-subordinates" in e["flags"]]
    for entry in residue:
        issues = ls_residue.problems(entry, tmux)
        print(f"NEGATIVE_CONTROL checker_on_live_residue pid={entry['pid']} issues={issues} (reported, not touched)")
        if "exited-with-subordinates" not in issues:
            failures.append(f"checker missed live residue pid={entry['pid']}")
    if not residue:
        print("NEGATIVE_CONTROL live_residue_sample=none (parser tests cover it)")
    for message in failures:
        print(f"NEGATIVE_CONTROL_FAIL {message}")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
