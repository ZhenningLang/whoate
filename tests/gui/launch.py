#!/usr/bin/env python3
"""Isolated GUI launcher (Phase 08 incident fix).

A GUI process exec'd from a tmux pane inherits the tmux server's resource
coalition; LaunchServices then treats tmux as the app's subordinates, and a
Dock force quit of that app ended the whole tmux server (2026-10-20). Every
GUI test therefore starts the app through LaunchServices with `open -n`, which
gives it its own coalition (measured in phases/08_gui_window HANDOFF).

Bare executables (app/.build/*/Whoate, WhoateChecks) are copied into a private
wrapper bundle with LSUIElement=true. `open` forwards its own environment to
the app (measured: a pane's TMUX and PATH arrived), so `open` itself runs with
a launchd-like environment (LAUNCHD_ENV) and every test variable goes through
--env. `open -W` exits 0 whatever the app's status, so callers judge the app
by its --stdout file.

Only the pid this launcher resolved is ever signalled, and only after its
identity (start time and executable path) is checked again. No force-quit API.

  launch.py start <target> [--env K=V ...] [--arg A ...] [--stdout F] [--stderr F]
  launch.py stop '<handle json>'
  launch.py run <target> [--env K=V ...] [--arg A ...] [--timeout S]   (prints the app's stdout)
"""
import argparse, ctypes, json, os, plistlib, shutil, signal, subprocess, sys, tempfile, time
from pathlib import Path

ISOLATION = "open-n"
# open -W's own message when the app exited before it could start waiting (measured).
EXITED_BEFORE_WAIT = "Unable to block on applications"
LAUNCHD_PATH = "/usr/bin:/bin:/usr/sbin:/sbin"
# Never TMUX/TMUX_PANE: the app would otherwise query the caller's tmux server.
LAUNCHD_KEYS = ("HOME", "USER", "LOGNAME", "TMPDIR")
_libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
PROC_PIDTBSDINFO = 3


class LaunchError(RuntimeError):
    pass


def _bsdinfo(pid):
    buf = ctypes.create_string_buffer(136)
    if _libproc.proc_pidinfo(pid, PROC_PIDTBSDINFO, 0, buf, 136) != 136:
        return None
    raw = buf.raw
    return {"ppid": int.from_bytes(raw[16:20], "little"),
            "start": int.from_bytes(raw[120:128], "little") * 1_000_000 + int.from_bytes(raw[128:136], "little")}


def _path(pid):
    buf = ctypes.create_string_buffer(4096)
    n = _libproc.proc_pidpath(pid, buf, 4096)
    return os.path.realpath(buf.value.decode(errors="replace")) if n > 0 else None


def process(pid):
    """{pid, ppid, start (µs since epoch), path} or None when gone."""
    info, path = _bsdinfo(pid), _path(pid)
    if info is None or path is None:
        return None
    return dict(info, pid=pid, path=path)


def processes(prefix):
    """Live processes whose executable is inside `prefix`."""
    count = _libproc.proc_listallpids(None, 0)
    pids = (ctypes.c_int * (count + 64))()
    n = _libproc.proc_listallpids(pids, ctypes.sizeof(pids))
    found = []
    for pid in pids[:n]:
        path = _path(pid) if pid > 0 else None
        if path and path.startswith(prefix):
            row = process(pid)
            if row:
                found.append(row)
    return found


def bundle_for(target, workdir):
    """(bundle, executable): the bundle containing target, or a new LSUIElement wrapper."""
    target = Path(os.path.realpath(target))
    if target.suffix == ".app" and target.is_dir():
        info = plistlib.loads((target / "Contents/Info.plist").read_bytes())
        return target, target / "Contents/MacOS" / info["CFBundleExecutable"]
    if target.parent.name == "MacOS" and target.parent.parent.name == "Contents" \
            and target.parents[2].suffix == ".app":
        return target.parents[2], target
    if not (target.is_file() and os.access(target, os.X_OK)):
        raise LaunchError(f"not_executable {target}")
    name = target.name
    bundle = Path(workdir) / f"{name}.app"
    (bundle / "Contents/MacOS").mkdir(parents=True)
    executable = bundle / "Contents/MacOS" / name
    shutil.copy2(target, executable)
    plist = {"CFBundleIdentifier": f"test.whoate.{name.lower()}.{os.getpid()}.{time.monotonic_ns()}",
             "CFBundleName": name, "CFBundleExecutable": name, "CFBundlePackageType": "APPL",
             "LSUIElement": True}
    (bundle / "Contents/Info.plist").write_bytes(plistlib.dumps(plist))
    return Path(os.path.realpath(bundle)), Path(os.path.realpath(executable))


# Test settings a caller may pass through its own environment (P09's
# verify_driver does): WHOATE_* plus the private tmux root and language.
INHERITED_KEYS = ("TMUX_TMPDIR", "LC_ALL", "LC_MESSAGES", "LANG")


def inherited_env(environ=os.environ, overrides=()):
    """The caller's test variables for the app, then `overrides`; never TMUX or TMUX_PANE."""
    env = {k: v for k, v in environ.items() if k.startswith("WHOATE_") or k in INHERITED_KEYS}
    env.update(overrides)
    return env


def launchd_env(environ=os.environ):
    """Environment for the `open` process itself, which it passes on to the app."""
    return dict({k: environ[k] for k in LAUNCHD_KEYS if k in environ}, PATH=LAUNCHD_PATH)


def open_command(bundle, env, args, stdout, stderr, wait):
    command = ["/usr/bin/open", "-n", "-g"] + (["-W"] if wait else [])
    for key, value in sorted(env.items()):
        command += ["--env", f"{key}={value}"]
    command += ["--stdout", str(stdout), "--stderr", str(stderr), str(bundle)]
    return command + (["--args", *args] if args else [])


class Launcher:
    """`runner` and `lister` are injectable for tests; production uses subprocess and libproc."""

    def __init__(self, runner=subprocess.Popen, lister=processes, lookup=process, signaller=os.kill,
                 clock=time.time, sleep=time.sleep):
        self.runner, self.lister, self.lookup = runner, lister, lookup
        self.signaller, self.clock, self.sleep = signaller, clock, sleep

    def start(self, target, env=None, args=(), stdout=None, stderr=None, wait=False, resolve_s=20):
        workdir = tempfile.mkdtemp(prefix="whoate-launch-")
        bundle, executable = bundle_for(target, workdir)
        stdout = stdout or os.path.join(workdir, "stdout")
        stderr = stderr or os.path.join(workdir, "stderr")
        before = {p["pid"] for p in self.lister(str(executable))}
        began = int(self.clock() * 1_000_000) - 1_000_000
        opener = self.runner(open_command(bundle, env or {}, list(args), stdout, stderr, wait),
                             env=launchd_env(), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE,
                             stderr=subprocess.PIPE)
        deadline = self.clock() + resolve_s
        handle = {"pid": None, "start": None, "path": str(executable), "bundle": str(bundle), "workdir": workdir,
                  "stdout": str(stdout), "stderr": str(stderr), "isolation": ISOLATION, "opener": opener}
        while True:
            done = wait and opener.poll() is not None
            fresh = [p for p in self.lister(str(executable))
                     if p["pid"] not in before and p["ppid"] == 1 and p["start"] >= began]
            if len(fresh) > 1:
                raise LaunchError(f"pid_ambiguous {[p['pid'] for p in fresh]}")
            if fresh:
                return dict(handle, pid=fresh[0]["pid"], start=fresh[0]["start"], path=fresh[0]["path"])
            # `open -W` already returned: a short-lived app (dump-tick) ran and
            # exited between two scans. Nothing is left to identify or signal.
            if done and opener.returncode == 0:
                return handle
            if (done or not wait) and opener.poll() not in (None, 0):
                message = opener.stderr.read().decode(errors="replace")
                # open launched it, but the app was gone before open -W could watch it.
                if done and EXITED_BEFORE_WAIT in message:
                    return dict(handle, open_note=message.strip())
                raise LaunchError(f"open_failed rc={opener.returncode} {message}")
            if self.clock() > deadline:
                raise LaunchError("pid_unresolved")
            self.sleep(0.1)

    def alive(self, handle):
        """The handle's process still runs: same pid, start time and executable."""
        if handle["pid"] is None:
            return False
        now = self.lookup(handle["pid"])
        return bool(now) and now["start"] == handle["start"] and now["path"] == handle["path"]

    def stop(self, handle, grace_s=10):
        """SIGTERM, then SIGKILL after grace_s; each signal only if the identity still matches."""
        if not self.alive(handle):
            return "gone_or_reused"
        self.signaller(handle["pid"], signal.SIGTERM)
        deadline = self.clock() + grace_s
        while self.clock() < deadline:
            if not self.alive(handle):
                return "terminated"
            self.sleep(0.1)
        if not self.alive(handle):
            return "terminated"
        self.signaller(handle["pid"], signal.SIGKILL)
        return "killed"

    def wait(self, handle, timeout_s):
        deadline = self.clock() + timeout_s
        while self.alive(handle):
            if self.clock() > deadline:
                return False
            self.sleep(0.1)
        return True

    def run(self, target, env=None, args=(), timeout_s=600):
        """Start, wait for exit (stopping it on timeout), return (exited, stdout text, handle)."""
        handle = self.start(target, env=env, args=args, wait=True)
        exited = self.wait(handle, timeout_s)
        if not exited:
            handle["stop"] = self.stop(handle)
        try:
            handle["opener"].wait(timeout=15)
        except subprocess.TimeoutExpired:
            handle["opener"].kill()
        text = Path(handle["stdout"]).read_text(errors="replace") if os.path.exists(handle["stdout"]) else ""
        return exited, text, handle


def cleanup(handle):
    """Removes the private wrapper/output directory once the app has exited."""
    shutil.rmtree(handle["workdir"], ignore_errors=True)


def public(handle):
    return {k: v for k, v in handle.items() if k != "opener"}


def _pairs(items):
    env = {}
    for item in items:
        key, sep, value = item.partition("=")
        if not sep or not key:
            raise SystemExit(f"bad --env {item!r}")
        env[key] = value
    return env


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = ap.add_subparsers(dest="command", required=True)
    for name in ("start", "run"):
        p = sub.add_parser(name)
        p.add_argument("target")
        p.add_argument("--env", action="append", default=[])
        p.add_argument("--arg", action="append", default=[])
        if name == "start":
            p.add_argument("--stdout")
            p.add_argument("--stderr")
        else:
            p.add_argument("--timeout", type=float, default=600)
    stop = sub.add_parser("stop")
    stop.add_argument("handle")
    a = ap.parse_args(argv)
    launcher = Launcher()
    if a.command == "start":
        handle = launcher.start(a.target, env=_pairs(a.env), args=a.arg, stdout=a.stdout, stderr=a.stderr)
        print(json.dumps(public(handle)))
        return 0
    if a.command == "stop":
        print(launcher.stop(json.loads(a.handle)))
        return 0
    exited, text, handle = launcher.run(a.target, env=_pairs(a.env), args=a.arg, timeout_s=a.timeout)
    sys.stdout.write(text)
    print(f"LAUNCH isolation={handle['isolation']} pid={handle['pid']} exited={exited} "
          f"bundle={handle['bundle']}", file=sys.stderr)
    cleanup(handle)
    return 0 if exited else 1


if __name__ == "__main__":
    sys.exit(main())
