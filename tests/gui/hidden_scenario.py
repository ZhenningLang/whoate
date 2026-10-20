#!/usr/bin/env python3
"""QA22/QA33: was a hidden CPU run really the hidden state it gates?

  hidden_scenario.py <evidence dir> <name>...   reads cpu-<name>.json and cpu-<name>.schedule

Hidden whoate samples host every 5 s and makes one paused full tick
("hidden-full") every 30 s. Inside the measurement window a valid run has
at least two hidden-full lines 28-40 s apart, only host/hidden-full lines,
and no /usr/bin/top among the app's descendants, listed once a second over the
whole window: no stretch longer than MAX_GAP_S without a listing (window
start and end count as edges) and no failed listing. Recorded runs
(2026-10-20, 5 runs): edges <= 1.01 s, gaps <= 1.57 s.
cold-hidden never refreshed fully; hidden-after-main opened the main window,
then paused before the window began. Prints the CPU, the hidden-full
intervals and when the engine was first used (cold: inside the warm-up).
"""
import json, os, sys

ALLOWED = {"host", "hidden-full"}
MAX_GAP_S = 2.5


def coverage(listed, start, end):
    """Longest stretch of the window [start, end] without a descendant listing."""
    times = [start] + sorted(s["epoch"] for s in listed) + [end]
    return max(b - a for a, b in zip(times, times[1:]))


def schedule(path):
    if not os.path.exists(path):
        return []
    with open(path) as f:
        return [(float(a), m) for a, m in (line.split("\t") for line in f.read().splitlines())]


def judge(name, receipt, rows):
    start, end = receipt.get("measure_started_epoch"), receipt.get("measure_ended_epoch")
    if start is None or end is None:
        return {"ok": False, "checks": {"measured": False}}
    inside = [(t, m) for t, m in rows if start <= t <= end]
    hidden = [t for t, m in inside if m == "hidden-full"]
    intervals = [round(b - a, 3) for a, b in zip(hidden, hidden[1:])]
    listed = [s for s in receipt.get("descendant_comms") or [] if start <= s["epoch"] <= end]
    longest = coverage(listed, start, end)
    top = [s for s in listed if any(os.path.basename(c) == "top" for c in s.get("comms", []))]
    when = lambda mode: [t for t, m in rows if m == mode]
    checks = {
        "two_hidden_full": len(hidden) >= 2,
        "intervals_28_40": bool(intervals) and all(28 <= i <= 40 for i in intervals),
        "window_only_host_hidden_full": {m for _, m in inside} <= ALLOWED,
        "descendants_listed": longest <= MAX_GAP_S and not any("error" in s for s in listed),
        "no_top": not top,
    }
    if name == "cold-hidden":
        checks["never_full_or_resume"] = not when("full") and not when("resume")
    else:
        fulls, pauses = when("full"), when("pause")
        checks["main_then_pause_before_window"] = bool(fulls) and bool(pauses) and max(fulls + pauses) < start
    launched = receipt.get("launched_epoch")
    engine = min(when("full") + when("hidden-full"), default=None)
    return {
        "ok": all(checks.values()), "checks": checks, "hidden_full_intervals": intervals,
        "hidden_full_in_window": len(hidden), "top_seen": top[:3],
        "longest_unlisted_s": round(longest, 3),
        "engine_first_used_s": None if engine is None or launched is None else round(engine - launched, 3),
        "engine_before_window": engine is not None and engine < start,
    }


def main(argv):
    out, *names = argv
    ok = True
    for name in names:
        with open(f"{out}/cpu-{name}.json") as f:
            receipt = json.load(f)
        verdict = judge(name, receipt, schedule(f"{out}/cpu-{name}.schedule"))
        print(f"QA33 {name} total_pct={receipt.get('total_pct')} self={receipt.get('mean_excluding_first')} "
              f"children={receipt.get('rusage_children_pct')} cpu_pass={receipt.get('pass')} "
              f"hidden_full_intervals={verdict.get('hidden_full_intervals')} "
              f"longest_unlisted_s={verdict.get('longest_unlisted_s')} "
              f"engine_first_used_s={verdict.get('engine_first_used_s')} "
              f"engine_before_window={verdict.get('engine_before_window')} scenario={verdict['checks']} "
              f"scenario_ok={verdict['ok']}")
        ok = ok and verdict["ok"]
    return 0 if ok else 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
