#!/usr/bin/env python3
"""Measure whoate GUI CPU (U-ACC-6 / U-SPK-6).

Starts the app binary, waits, then runs `top -l 61 -s 1 -pid <pid> -stats cpu`
and averages samples 2..61 (the first top sample has no interval). Child CPU
(top, zprint, docker the engine spawns) is reported separately from
proc_pid_rusage child times and is not part of the threshold. Every run is
written out in full; nothing is retried or filtered.
"""
import argparse, ctypes, json, os, platform, signal, subprocess, sys, time

libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
libc = ctypes.CDLL("/usr/lib/libSystem.dylib")


class RusageV2(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in (
        "user", "system", "pkg_idle_wkups", "interrupt_wkups", "pageins", "wired_size",
        "resident_size", "phys_footprint", "start_abstime", "exit_abstime", "child_user",
        "child_system", "child_pkg_idle_wkups", "child_interrupt_wkups", "child_pageins",
        "child_elapsed_abstime", "diskio_read", "diskio_written")]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


def ns_per_tick():
    tb = Timebase()
    libc.mach_timebase_info(ctypes.byref(tb))
    return tb.numer / tb.denom


def rusage(pid):
    info = RusageV2()
    if libproc.proc_pid_rusage(pid, 2, ctypes.byref(info)) != 0:
        return None
    k = ns_per_tick()
    return {"self_ns": (info.user + info.system) * k, "child_ns": (info.child_user + info.child_system) * k}


def descendants(pid):
    rows = subprocess.run(["/bin/ps", "-A", "-o", "pid=,ppid=,%cpu=,comm="], capture_output=True, text=True).stdout
    table = [r.split(None, 3) for r in rows.splitlines() if r.strip()]
    found, frontier = [], {pid}
    while frontier:
        nxt = {int(r[0]) for r in table if int(r[1]) in frontier}
        found += [{"pid": int(r[0]), "cpu": float(r[2]), "comm": r[3]} for r in table if int(r[0]) in nxt]
        frontier = nxt
    return found


def top_samples(pid, count):
    out = subprocess.run(["/usr/bin/top", "-l", str(count), "-s", "1", "-pid", str(pid), "-stats", "pid,cpu"],
                         capture_output=True, text=True, timeout=count + 30).stdout
    values = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] == str(pid):
            values.append(float(parts[1]))
    return values, out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--warmup", type=float, default=5)
    ap.add_argument("--samples", type=int, default=61)
    ap.add_argument("--env", action="append", default=[], help="KEY=VALUE for the app")
    ap.add_argument("--out", required=True)
    ap.add_argument("--limit", type=float, default=1.0)
    args = ap.parse_args()

    env = dict(os.environ)
    for item in args.env:
        key, _, value = item.partition("=")
        env[key] = value
    app = subprocess.Popen([args.binary], env=env, stdin=subprocess.DEVNULL,
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    result = {"label": args.label, "binary": args.binary, "pid": app.pid, "warmup_s": args.warmup}
    try:
        time.sleep(args.warmup)
        if app.poll() is not None:
            raise SystemExit(f"CPU_APP_EXITED rc={app.returncode}")
        before = rusage(app.pid)
        began = time.monotonic()
        values, raw = top_samples(app.pid, args.samples)
        elapsed = time.monotonic() - began
        after = rusage(app.pid)
        kids = descendants(app.pid)
        load = os.getloadavg()
    finally:
        app.send_signal(signal.SIGTERM)
        try:
            app.wait(timeout=10)
        except subprocess.TimeoutExpired:
            app.kill()
            app.wait()
    if len(values) != args.samples:
        raise SystemExit(f"CPU_SAMPLES_INCOMPLETE got={len(values)} want={args.samples}")
    kept = values[1:]
    mean = sum(kept) / len(kept)
    result.update({
        "samples": values,
        "mean_excluding_first": round(mean, 3),
        "max_excluding_first": max(kept),
        "rusage_self_pct": round((after["self_ns"] - before["self_ns"]) / (elapsed * 1e9) * 100, 3) if before and after else None,
        "rusage_children_pct": round((after["child_ns"] - before["child_ns"]) / (elapsed * 1e9) * 100, 3) if before and after else None,
        "live_children_at_end": kids,
        "loadavg": load,
        "machine": platform.machine(),
        "ncpu": os.cpu_count(),
        "limit": args.limit,
        "pass": mean < args.limit,
    })
    with open(args.out, "w") as f:
        json.dump(result, f, indent=1)
    print(f"CPU {args.label} mean={mean:.3f}% max={max(kept):.1f}% children={result['rusage_children_pct']}% "
          f"load={load[0]:.2f} pass={result['pass']}")
    sys.exit(0 if result["pass"] else 1)


if __name__ == "__main__":
    main()
