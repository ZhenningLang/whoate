#!/usr/bin/env python3
"""Measure whoate GUI CPU (U-ACC-6 / U-SPK-6).

The external observer is never a product child. The threshold includes live
descendants plus reaped-child credits, measured over the same top window.
Every run, including incomplete and failed runs, retains its raw evidence.
"""
import argparse, ctypes, hashlib, json, math, os, platform, signal, subprocess, sys, threading, time
from pathlib import Path

libproc = ctypes.CDLL("/usr/lib/libproc.dylib")
libc = ctypes.CDLL("/usr/lib/libSystem.dylib")


class TopFailure(ValueError):
    def __init__(self, message, raw):
        super().__init__(message)
        self.raw = raw


class TreeSnapshotError(ValueError):
    def __init__(self, message, children=None, parent=None):
        super().__init__(message)
        self.children = children or []
        self.parent = parent


class RusageV2(ctypes.Structure):
    _fields_ = [("uuid", ctypes.c_uint8 * 16)] + [(name, ctypes.c_uint64) for name in (
        "user", "system", "pkg_idle_wkups", "interrupt_wkups", "pageins", "wired_size",
        "resident_size", "phys_footprint", "start_abstime", "exit_abstime", "child_user",
        "child_system", "child_pkg_idle_wkups", "child_interrupt_wkups", "child_pageins",
        "child_elapsed_abstime", "diskio_read", "diskio_written")]


class Timebase(ctypes.Structure):
    _fields_ = [("numer", ctypes.c_uint32), ("denom", ctypes.c_uint32)]


def timebase_ratio():
    tb = Timebase()
    if libc.mach_timebase_info(ctypes.byref(tb)) != 0 or not tb.numer or not tb.denom:
        raise ValueError("timebase_unavailable")
    return tb.numer, tb.denom


def rusage(pid):
    info = RusageV2()
    if libproc.proc_pid_rusage(pid, 2, ctypes.byref(info)) != 0:
        raise ValueError(f"rusage_unavailable pid={pid}")
    numer, denom = timebase_ratio()
    # Preserve cumulative time as integer ns; subtracting large float totals can
    # manufacture a negative sub-ns child delta when its credit did not change.
    return {"self_ns": (info.user + info.system) * numer // denom,
            "child_ns": (info.child_user + info.child_system) * numer // denom,
            "identity": info.start_abstime}


def descendants(pid):
    rows = subprocess.run(["/bin/ps", "-A", "-o", "pid=,ppid=,uid=,stat=,comm="], check=True,
                          capture_output=True, text=True, timeout=10).stdout
    table = [r.split(None, 4) for r in rows.splitlines() if r.strip()]
    found, frontier, seen = [], {pid}, {pid}
    while frontier:
        nxt = {int(r[0]) for r in table if int(r[1]) in frontier and int(r[0]) not in seen}
        found += [{"pid": int(r[0]), "ppid": int(r[1]), "uid": int(r[2]), "state": r[3], "comm": r[4]}
                  for r in table if int(r[0]) in nxt]
        seen.update(nxt)
        frontier = nxt
    return found


def tree_sample(pid):
    # A live parent's child counters credit waited children, not live children.
    # Thus each live node contributes its own CPU plus its disjoint reaped subtree.
    root = str(pid)
    discarded = []
    # ceiling: at most 8 coherent-cut acquisitions, never a retry of a CPU
    # window; persistent churn/missing counters fail rather than lowering CPU.
    for attempt in range(1, 9):
        children = descendants(pid)
        if any(c.get("state", "").startswith("Z") for c in children):
            discarded.append("zombie_transfer_pending")
            continue
        members = {pid, *(c["pid"] for c in children)}
        try:
            before = {str(p): rusage(p) for p in sorted(members)}
            middle = descendants(pid)
            if members != {pid, *(c["pid"] for c in middle)}:
                discarded.append("membership_changed")
                continue
            after = {str(p): rusage(p) for p in sorted(members)}
            end = descendants(pid)
            if (members != {pid, *(c["pid"] for c in end)}
                    or any(c.get("state", "").startswith("Z") for c in middle + end)):
                discarded.append("membership_changed")
                continue
            if before[root]["identity"] != after[root]["identity"]:
                raise ValueError("root_pid_reused")
            if any(before[p]["identity"] != after[p]["identity"]
                   or before[p]["child_ns"] != after[p]["child_ns"]
                   or after[p]["self_ns"] < before[p]["self_ns"] for p in before):
                discarded.append("credit_or_identity_changed")
                continue
            final_root = rusage(pid)
            if final_root["identity"] != after[root]["identity"]:
                raise ValueError("root_pid_reused")
            if final_root["child_ns"] != after[root]["child_ns"]:
                discarded.append("parent_credit_changed")
                continue
            after[root] = dict(final_root, snapshot_attempts=attempt, discarded_cuts=discarded)
            return after
        except ValueError as error:
            if str(error) in ("root_pid_reused", f"rusage_unavailable pid={pid}"):
                raise
            discarded.append(str(error))
            protected = [c for c in children if str(error) == f"rusage_unavailable pid={c['pid']}"
                         and c.get("uid") == 0 and c.get("ppid") == pid and c.get("comm") == "/usr/bin/top"]
            if protected:
                raise TreeSnapshotError("privileged_top_counter_pending", protected, rusage(pid)) from error
    raise TreeSnapshotError("cpu_snapshot_unstable: " + ";".join(discarded))


def reconcile_gaps(gaps, tree, root, at):
    parent = tree[str(root)]
    for gap in gaps:
        if gap.get("reconciled_at") is not None:
            continue
        if parent["identity"] != gap["parent_identity"]:
            raise ValueError("root_identity_changed_during_privileged_gap")
        if str(gap["pid"]) not in tree and parent["child_ns"] > gap["credit_floor_ns"]:
            # No guessed child CPU: the same parent's kernel wait ledger has
            # advanced after its known direct, non-daemonizing top child exited.
            gap["reconciled_at"] = at
            gap["parent_credit_after_ns"] = parent["child_ns"]
    return all(gap.get("reconciled_at") is not None for gap in gaps)


def tree_delta(before, after, root):
    root = str(root)
    if root not in before or root not in after:
        raise ValueError("root_missing")
    for pid in before.keys() & after.keys():
        if before[pid]["identity"] != after[pid]["identity"]:
            raise ValueError("pid_reused")
    children_total = lambda rows: sum(v["child_ns"] + (v["self_ns"] if p != root else 0)
                                      for p, v in rows.items())
    own = after[root]["self_ns"] - before[root]["self_ns"]
    children = children_total(after) - children_total(before)
    if own < 0 or children < 0:
        raise ValueError("cpu_counter_regressed_or_unreaped_exit")
    lost = before.keys() - after.keys()
    credited = sum(v["child_ns"] for v in after.values()) - sum(v["child_ns"] for v in before.values())
    if credited < sum(before[p]["self_ns"] for p in lost):
        raise ValueError("descendant_exit_without_wait_credit")
    return own, children


def top_samples(pid, count):
    try:
        completed = subprocess.run(["/usr/bin/top", "-l", str(count), "-s", "1", "-pid", str(pid), "-stats", "pid,cpu"],
                                   capture_output=True, text=True, timeout=count + 30)
    except subprocess.TimeoutExpired as error:
        raw = error.stdout or b""
        if isinstance(raw, bytes): raw = raw.decode(errors="replace")
        raise TopFailure("top_timeout", raw) from error
    out = completed.stdout
    if completed.returncode:
        raise TopFailure(f"top_failed rc={completed.returncode}", out)
    values = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) == 2 and parts[0] == str(pid):
            values.append(float(parts[1]))
    return values, out


def evaluate(values, count, children_pct, limit):
    if len(values) != count or count < 2:
        raise ValueError("cpu_samples_incomplete")
    if (children_pct is None or not math.isfinite(children_pct) or children_pct < 0
            or any(not math.isfinite(v) or v < 0 for v in values)):
        raise ValueError("cpu_measurement_invalid")
    mean = math.fsum(values[1:]) / len(values[1:])
    total = mean + children_pct
    return {"mean": mean, "total": total, "pass": total < limit}


def warmup(seconds):
    began = time.monotonic()
    while True:
        remaining = seconds - (time.monotonic() - began)
        if remaining <= 0: return
        time.sleep(min(60, remaining))
        print(f"CPU_WARMUP elapsed_s={time.monotonic() - began:.1f} target_s={seconds}", flush=True)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--binary", required=True)
    ap.add_argument("--label", required=True)
    ap.add_argument("--warmup", type=float, default=5)
    ap.add_argument("--samples", type=int, default=61)
    ap.add_argument("--env", action="append", default=[], help="KEY=VALUE for the app")
    ap.add_argument("--out", required=True)
    ap.add_argument("--limit", type=float, default=1.0)
    ap.add_argument("--report-only", action="store_true", help="visible CPU has no <1%% gate; accounting must still be valid")
    args = ap.parse_args()

    env = dict(os.environ)
    for item in args.env:
        key, _, value = item.partition("=")
        env[key] = value
    if Path(args.out).exists():
        raise SystemExit("CPU_EVIDENCE_ALREADY_EXISTS: choose a new run path")
    app = None
    def interrupted(signum, frame):
        raise KeyboardInterrupt(f"signal={signum}")
    signal.signal(signal.SIGTERM, interrupted)
    result = {"label": args.label, "binary": args.binary, "warmup_s": args.warmup,
              "pass": False, "loadavg": os.getloadavg(), "machine": platform.machine(),
              "ncpu": os.cpu_count(), "limit": args.limit, "raw_top": None}
    try:
        result["binary_sha256"] = hashlib.sha256(Path(args.binary).read_bytes()).hexdigest()
        app = subprocess.Popen([args.binary], env=env, stdin=subprocess.DEVNULL,
                               stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        result["pid"] = app.pid
        print(f"CPU_START label={args.label} pid={app.pid} warmup_s={args.warmup}", flush=True)
        warmup(args.warmup)
        if app.poll() is not None:
            raise ValueError(f"cpu_app_exited rc={app.returncode}")
        before = tree_sample(app.pid)
        began = time.monotonic()
        timeline = [{"at": began, "tree": before}]
        failures = []
        gaps = []
        done = threading.Event()
        def observe():
            while not done.wait(1):
                try:
                    current = tree_sample(app.pid)
                    tree_delta(timeline[-1]["tree"], current, app.pid)
                    reconcile_gaps(gaps, current, app.pid, time.monotonic())
                    timeline.append({"at": time.monotonic(), "tree": current})
                except TreeSnapshotError as error:
                    if error.children and error.parent is not None:
                        for child in error.children:
                            gaps.append(dict(child, at=time.monotonic(), parent_identity=error.parent["identity"],
                                             credit_floor_ns=timeline[-1]["tree"][str(app.pid)]["child_ns"]))
                        continue
                    failures.append(str(error))
                    return
                except Exception as error:
                    failures.append(str(error))
                    return
        observer = threading.Thread(target=observe)
        observer.start()
        try:
            values, raw = top_samples(app.pid, args.samples)
            elapsed = time.monotonic() - began
        finally:
            done.set()
            observer.join()
            result["rusage_timeline"] = timeline
            result["privileged_child_gaps"] = gaps
        # Save raw top before any subsequent fallible accounting check.
        result.update(samples=values, raw_top=raw, duration=elapsed, before=before)
        after = tree_sample(app.pid)
        result["after"] = after
        if failures: raise ValueError("cpu_timeline_invalid: " + failures[0])
        tree_delta(timeline[-1]["tree"], after, app.pid)
        if not reconcile_gaps(gaps, after, app.pid, time.monotonic()):
            raise ValueError("unreconciled_privileged_child_gap")
        own, children = tree_delta(before, after, app.pid)
        child_pct = children / (elapsed * 1e9) * 100
        verdict = evaluate(values, args.samples, child_pct, args.limit)
        result.update(mean_excluding_first=verdict["mean"], rusage_self_pct=own / (elapsed * 1e9) * 100,
                      rusage_children_pct=child_pct, total_pct=verdict["total"], **{"pass": verdict["pass"]})
        result["threshold_pass"] = verdict["pass"]
        if args.report_only: result["pass"] = True
    except (Exception, KeyboardInterrupt) as error:
        result["error"] = str(error)
        if isinstance(error, TopFailure): result["raw_top"] = error.raw
    finally:
        if app is not None and app.poll() is None:
            app.send_signal(signal.SIGTERM)
            try:
                app.wait(timeout=10)
            except subprocess.TimeoutExpired:
                app.kill()
                app.wait()
        with open(args.out, "x") as f:
            json.dump(result, f, indent=1)
    print(f"CPU {args.label} self={result.get('mean_excluding_first')}% "
          f"children={result.get('rusage_children_pct')}% total={result.get('total_pct')}% pass={result['pass']}")
    sys.exit(0 if result["pass"] else 1)


if __name__ == "__main__":
    main()
