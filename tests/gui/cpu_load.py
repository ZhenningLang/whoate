"""Owned native meter calibration load. No unrelated process is signalled."""
import json
import subprocess
import sys
import time


def spin():
    before = time.process_time_ns()
    end = time.monotonic() + 0.3
    n = 0
    while time.monotonic() < end:
        n += 1
    return time.process_time_ns() - before


def worker(grandchild=False):
    child = None
    try:
        if grandchild:
            child = subprocess.Popen([sys.executable, __file__, "worker"], stdin=subprocess.PIPE,
                                     stdout=subprocess.PIPE, text=True)
        total = spin()
        if child:
            total += int(child.stdout.readline())
        print(total, flush=True)
        sys.stdin.readline()
    finally:
        if child:
            child.communicate("quit\n", timeout=5)


def root(kind):
    print("ready", flush=True)
    sys.stdin.readline()
    child = subprocess.Popen([sys.executable, __file__, "worker-grandchild" if kind == "grandchild" else "worker"],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
    try:
        reference = int(child.stdout.readline())
        if kind == "reaped":
            child.communicate("quit\n", timeout=5)
        print(json.dumps({"reference_ns": reference}), flush=True)
        sys.stdin.readline()
    finally:
        if child.poll() is None:
            child.communicate("quit\n", timeout=5)


if __name__ == "__main__":
    kind = sys.argv[1]
    if kind in ["worker", "worker-grandchild"]:
        worker(kind == "worker-grandchild")
    else:
        root(kind)
