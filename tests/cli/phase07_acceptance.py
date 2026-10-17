"""Independent P07 native oracles. Never reads a real transcript or changes services."""
import argparse
import contextlib
import hashlib
import json
import os
import pathlib
import re
import selectors
import shutil
import signal
import subprocess
import tempfile
import time


def require(condition, code):
    if not condition:
        raise RuntimeError(code)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def bind(root, binary, path):
    files = set(root.glob("crates/**/*.rs")) | set(root.glob("crates/**/*.c"))
    files |= set(root.glob("crates/**/*.toml")) | set(root.glob("tests/cli/phase07*"))
    files |= set(root.glob("tests/cli/test_phase07*"))
    files |= {root / p for p in ["Cargo.toml", "Cargo.lock", "schema/whoate-v1.schema.json", "skills/whoate/SKILL.md"]}
    value = {"binary": str(binary.resolve()), "binary_sha256": sha(binary), "inputs": {str(p.relative_to(root)): sha(p) for p in sorted(files) if p.is_file()}}
    path.write_text(json.dumps(value, sort_keys=True))
    print("P07_BUILD_BINDING_WRITTEN=1")


def check_binding(root, binary, path):
    value = json.loads(path.read_text())
    require(value["binary"] == str(binary.resolve()) and value["binary_sha256"] == sha(binary), "BINARY_BINDING_MISMATCH")
    for name, digest in value["inputs"].items():
        require(sha(root / name) == digest, "SOURCE_BINDING_MISMATCH:" + name)


def native_zones(raw):
    """All independent candidates, never the production Rust parser/top five."""
    lines = raw.splitlines()
    require(len(lines) >= 4 and "elem" in lines[0] and "inuse" in lines[1], "ORACLE_ZPRINT_HEADER")
    records = []
    for line in lines[3:]:
        if line.startswith("----") or line == "ZONE TOTALS":
            break
        if not line.strip():
            continue
        columns = line.split()
        require(len(columns) >= 10, "ORACLE_ZPRINT_COLUMNS")
        require(columns[1].isdigit() and columns[6].isdigit(), "ORACLE_ZPRINT_COUNTS")
        records.append({"name": columns[0], "bytes": int(columns[1]) * int(columns[6])})
    require(records, "ORACLE_ZPRINT_EMPTY")
    return sorted(records, key=lambda z: (-z["bytes"], z["name"]))


def wired_bytes(raw):
    page = re.search(r"page size of (\d+) bytes", raw)
    count = re.search(r"^Pages wired down:\s+(\d+)\.", raw, re.MULTILINE)
    require(page and count, "ORACLE_VM_STAT_FORMAT")
    return int(page[1]) * int(count[1])


def check_envelope(kind, name, product, before, after):
    # P07 ruling six: dynamic tolerance, never a product memory guarantee.
    maximum = max(before, after)
    tol = max(256 * 1024 * 1024, maximum / 20) if kind == "wired" else max(64 * 1024, maximum / 1000)
    outside = max(min(before, after) - product, product - maximum, 0)
    print(json.dumps({"kind": kind, "name": name, "before": before, "product": product, "after": after, "tol": tol, "outside_bytes": outside, "excess_after_tol": max(0, outside - tol)}, sort_keys=True), flush=True)
    require(outside <= tol, kind.upper() + "_OUTSIDE_EXTERNAL_ENVELOPE:" + name)


def check_zone_envelope(product, before, after):
    require(0 < len(product) == min(5, len(before), len(after)), "ZONE_TOP5_COUNT")
    require(product == sorted(product, key=lambda z: (-z["bytes"], z["name"])), "ZONE_PRODUCT_ORDER")
    names = {z["name"] for z in product}
    require(len(names) == len(product), "ZONE_DUPLICATE_NAME")
    for sample in (before, after):
        # zprint truncates some unrelated small names to the same '$' suffix.
        # Only candidates participating in the top-five comparison need identity.
        cutoff = sample[min(5, len(sample)) - 1]["bytes"]
        expected = {z["name"] for z in sample[:5]}
        # Only names within the approved tolerance of fifth place may exchange.
        for name in names ^ expected:
            row = next((z for z in sample if z["name"] == name), None)
            require(row is not None, "ZONE_NAME_AMBIGUOUS_OR_MISSING")
            gap = abs(row["bytes"] - cutoff)
            tol = max(64 * 1024, max(row["bytes"], cutoff) / 1000)
            print(json.dumps({"kind": "zone_rank", "name": name, "tol": tol, "outside_bytes": gap, "excess_after_tol": max(0, gap - tol)}, sort_keys=True), flush=True)
            require(gap <= tol, "ZONE_DYNAMIC_RANK_MISMATCH:" + name)
    for row in product:
        left = [z["bytes"] for z in before if z["name"] == row["name"]]
        right = [z["bytes"] for z in after if z["name"] == row["name"]]
        require(len(left) == len(right) == 1, "ZONE_NAME_AMBIGUOUS_OR_MISSING")
        check_envelope("zone", row["name"], row["bytes"], left[0], right[0])


def check_pane_origins(owners, title, window):
    require(owners, "TMUX_PANE_OWNER_MISSING")
    for owner in owners:
        origin = owner["origin"]
        require(origin["pane_title"] == title and origin["window_name"] == window, "TMUX_ORIGIN_MISMATCH")


def environment_evidence():
    commands = [["/usr/bin/uptime"], ["/usr/sbin/sysctl", "-n", "vm.swapusage"], ["/usr/sbin/sysctl", "-n", "kern.memorystatus_vm_pressure_level"]]
    return [subprocess.run(cmd, capture_output=True, timeout=5, check=True).stdout.decode().strip() for cmd in commands]


class Native:
    def __init__(self, root, binary, binding, directory, captures=None):
        self.root, self.binary, self.directory = root, binary, directory
        check_binding(root, binary, binding)
        self.captures = captures
        self.env = dict(os.environ)
        for key in ["TMUX", "TMUX_PANE", "WHOATE_SESSION_PID", "WHOATE_TEST_CLAUDE_PID", "WHOATE_TEST_CLAUDE_BODY_AUTHORIZED", "WHOATE_CAPTURE_RAW", "WHOATE_CAPTURE_LIVE"]:
            self.env.pop(key, None)
        for name in ["home", "cache", "tmux"]:
            (directory / name).mkdir()
        self.env.update(HOME=str(directory / "home"), WHOATE_HOME_OVERRIDE=str(directory / "home"), WHOATE_CACHE_DIR=str(directory / "cache"), WHOATE_RULES=str(directory / "home/rules.toml"), TMUX_TMPDIR=str(directory / "tmux"), NO_COLOR="1")
        self.serial = 0

    def record(self, raw):
        value = json.loads(raw)
        if self.captures:
            self.captures.mkdir(parents=True, exist_ok=True, mode=0o700)
            self.serial += 1
            path=self.captures / f"{os.getpid()}-{self.serial}.json"
            path.write_bytes(raw.strip() + b"\n")
            path.chmod(0o600)
        return value

    def query(self, args, env=None):
        start = time.monotonic()
        result = subprocess.run([str(self.binary), *args], env=env or self.env, capture_output=True, timeout=15, stdin=subprocess.DEVNULL)
        elapsed = (time.monotonic() - start) * 1000
        require(result.returncode in [0, 1], "CLI_RUNTIME_EXIT:" + str(result.returncode))
        value = self.record(result.stdout)
        require("error" not in value, "CLI_RUNTIME_ERROR")
        return value, elapsed

    def watch_until(self, predicate):
        process = subprocess.Popen([str(self.binary), "watch", "--interval", "1s", "--json-lines"], env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        pending, frames = b"", 0
        deadline = time.monotonic() + 15
        try:
            while time.monotonic() < deadline:
                require(process.poll() is None, "WATCH_EARLY_EXIT")
                if not selector.select(max(0, deadline - time.monotonic())):
                    break
                chunk = os.read(process.stdout.fileno(), 65536)
                require(chunk, "WATCH_EOF")
                pending += chunk
                # ceiling: 16 MiB per frame and 32 frames bound the test driver;
                # increase only with a demonstrated supported larger snapshot.
                require(len(pending) <= 16 * 1024 * 1024, "WATCH_FRAME_LIMIT")
                while b"\n" in pending:
                    raw, pending = pending.split(b"\n", 1)
                    if not raw:
                        continue
                    frames += 1
                    require(frames <= 32, "WATCH_FRAME_COUNT_LIMIT")
                    value = self.record(raw)
                    if predicate(value):
                        return value
            raise RuntimeError("WATCH_SIGNAL_DEADLINE")
        finally:
            selector.close()
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
            try:
                code = process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                process.kill()  # Only this Popen child, never a preexisting PID.
                process.wait(timeout=5)
                raise RuntimeError("WATCH_CLEANUP_TIMEOUT")
            require(code == 0, "WATCH_CLEANUP_EXIT")

    def kernel(self):
        started = time.time()
        before_wired = wired_bytes(subprocess.check_output(["/usr/bin/vm_stat"], text=True, timeout=5))
        before = native_zones(subprocess.check_output(["/usr/bin/zprint", "-t"], text=True, timeout=5))
        value = self.watch_until(lambda v: v.get("host", {}).get("kernel_zones", {}).get("status") == "fresh")
        after = native_zones(subprocess.check_output(["/usr/bin/zprint", "-t"], text=True, timeout=5))
        after_wired = wired_bytes(subprocess.check_output(["/usr/bin/vm_stat"], text=True, timeout=5))
        finished = time.time()
        host = value["host"]
        zones = host["kernel_zones"]
        # Persist observations before any oracle assertion. A failing dynamic
        # comparison must not discard its two external reference samples.
        observation={"case": "kernel", "started": started, "finished": finished, "as_of": zones["as_of"], "wired_before": before_wired, "wired_product": host["wired_bytes"], "wired_after": after_wired, "all_candidates_before": before, "all_candidates_after": after, "product": zones["top"]}
        if self.captures:
            path=self.captures.with_name(self.captures.name+f".kernel-{os.getpid()}.json")
            path.write_text(json.dumps(observation,sort_keys=True));path.chmod(0o600)
        print(json.dumps(observation, sort_keys=True), flush=True)
        check_envelope("wired", "host", host["wired_bytes"], before_wired, after_wired)
        from datetime import datetime
        at = datetime.fromisoformat(zones["as_of"]).timestamp()
        require(started <= at <= finished, "ZONE_AS_OF_OUTSIDE_OBSERVATION")
        check_zone_envelope(zones["top"], before, after)
        gpu = host["gpu_memory"]
        require(gpu["status"] == "missing" and gpu["bytes"] is None and gpu["reason"], "GPU_UNPROVEN_MEASUREMENT")

    def startup(self):
        self.watch_until(lambda v: v["system_procs"]["status"] == "fresh")
        self.query(["top", "--json"])  # Exactly one untimed warm-up.
        environment = environment_evidence()
        cached, cached_ms = self.query(["top", "--json"])
        missing_env = dict(self.env, WHOATE_CACHE_DIR=str(self.directory / "missing-cache"))
        missing, missing_ms = self.query(["top", "--json"], missing_env)
        print(json.dumps({"case": "startup", "cached_ms": cached_ms, "missing_ms": missing_ms, "cached_status": cached["system_procs"]["status"], "missing_status": missing["system_procs"]["status"], "environment_before": environment, "environment_after": environment_evidence()}, sort_keys=True))
        require(cached["system_procs"]["status"] == "fresh" and cached_ms < 300, "CACHED_STARTUP_BUDGET")
        require(missing["system_procs"]["status"] == "missing" and missing_ms < 500, "MISSING_STARTUP_BUDGET")

    def helper_binary(self):
        output = self.directory / "whoate-p07-memory"
        sdk = subprocess.check_output(["/usr/bin/xcrun", "--sdk", "macosx", "--show-sdk-path"], text=True, timeout=10).strip()
        subprocess.run(["/usr/bin/clang", "-O2", "-Wall", "-Wextra", "-Werror", "-isysroot", sdk, str(self.root / "tests/cli/phase07_memory_helper.c"), "-o", str(output)], check=True, timeout=30, capture_output=True)
        return output

    def reclaim(self):
        raw_amount = os.environ.get("WHOATE_P07_HELPER_BYTES", "")
        require(raw_amount.isdigit() and int(raw_amount) > 0, "QA11_HELPER_BYTES_REQUIRED: register explicit allocation, no implicit default")
        amount = int(raw_amount)
        total = int(subprocess.check_output(["/usr/sbin/sysctl", "-n", "hw.memsize"], text=True, timeout=5))
        # ceiling: the fixture may allocate at most 1/16 physical RAM. An explicit
        # larger test needs an owner safety ruling, not a silent driver override.
        require(amount <= total // 16, "QA11_HELPER_SAFETY_CAP")
        helper = self.helper_binary()
        process = subprocess.Popen([str(helper), "--allocate", str(amount)], env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, bufsize=0)
        selector = selectors.DefaultSelector()
        selector.register(process.stdout, selectors.EVENT_READ)
        receipt = None

        def read_record():
            require(selector.select(15), "HELPER_READY_TIMEOUT")
            # The helper emits only bounded native JSON records, never text.
            line = process.stdout.readline(4096)
            require(line.endswith(b"\n"), "HELPER_RECORD_TRUNCATED")
            return json.loads(line)

        def observe():
            result = subprocess.run([str(helper), "--resident", str(receipt["pid"])], env=self.env, capture_output=True, timeout=5)
            if result.returncode == 3:
                return None
            require(result.returncode == 0, "HELPER_NATIVE_IDENTITY_UNAVAILABLE")
            value = json.loads(result.stdout)
            require((value["pid"], value["uid"], value["start_time"]) == (receipt["pid"], receipt["uid"], receipt["start_time"]), "HELPER_IDENTITY_REUSED")
            return value

        try:
            receipt = read_record()
            require(receipt["uid"] == os.getuid() and receipt["pid"] > 1 and receipt["start_time"] > 0, "HELPER_CREATION_RECEIPT")
            ready = read_record()
            require(ready["pid"] == receipt["pid"] and ready["start_time"] == receipt["start_time"], "HELPER_READY_IDENTITY")
            require(process.wait(timeout=5) == 0, "HELPER_PARENT_EXIT")
            before = observe()
            report, _ = self.query(["why", str(receipt["pid"]), "--json"])
            after = observe()
            require(before and after, "HELPER_DIED_BEFORE_KILL")
            owner = next(o for o in report["owners"] if receipt["pid"] in o["pids"])
            require(owner["pids"] == [receipt["pid"]], "HELPER_OWNER_NOT_ISOLATED")
            estimate = owner["reclaimable_physical"]["bytes"]
            require(estimate and min(before["resident_bytes"], after["resident_bytes"]) <= estimate <= max(before["resident_bytes"], after["resident_bytes"]), "RESIDENT_OUTSIDE_NATIVE_ENVELOPE")
            environment = environment_evidence()
            killed, _ = self.query(["kill", str(receipt["pid"]), "--json", "--yes"])
            require(killed["executed"] is True and len(killed["results"]) == 1, "KILL_RESULT_SHAPE")
            require(killed["results"][0]["pid"] == receipt["pid"] and killed["results"][0]["status"] == "terminated", "HELPER_NOT_TERMINATED")
            memory = killed["memory"]
            physical = memory["physical_memory_change_bytes"]
            kill_estimate = memory["before"]["target_resident_bytes"]
            ratio = physical / kill_estimate if physical is not None and kill_estimate else None
            print(json.dumps({"case": "reclaim", "allocated_bytes": amount, "native_before": before, "native_after_query": after, "owner_estimate": estimate, "memory": memory, "ratio": ratio, "environment_before": environment, "environment_after": environment_evidence()}, sort_keys=True))
            require(physical is not None and physical > 0 and ratio is not None and 0.1 <= ratio <= 10, "HOST_RECLAIM_DIRECTION_OR_ORDER_OF_MAGNITUDE")
        finally:
            selector.close()
            if receipt is not None:
                current = observe()
                if current is not None:
                    os.kill(receipt["pid"], signal.SIGTERM)  # Exact owned identity was just rechecked.
                    deadline = time.monotonic() + 5
                    while observe() is not None and time.monotonic() < deadline:
                        time.sleep(0.01)
                    require(observe() is None, "HELPER_CLEANUP_FAILED")
            if process.poll() is None:
                process.terminate()
                process.wait(timeout=5)
            print("P07_OWNED_HELPER_CLEANUP_OK=1")

    def tmux(self):
        tmux = shutil.which("tmux")
        require(tmux, "TMUX_REQUIRED")
        # macOS sockaddr_un is too short for the normal per-user TMPDIR.
        with tempfile.TemporaryDirectory(prefix="wt7-", dir="/private/tmp") as short:
            env = dict(self.env, TMUX_TMPDIR=short)
            base = [tmux, "-f", "/dev/null", "-L", "whoate-p07"]
            socket = pathlib.Path(short) / f"tmux-{os.getuid()}" / "whoate-p07"
            require(not socket.exists(), "TMUX_RESOURCE_ALREADY_EXISTS")

            def run(args):
                return subprocess.run(base + args, env=env, capture_output=True, timeout=10, check=True).stdout

            pane = run(["new-session", "-d", "-s", "p07", "-P", "-F", "#{pane_id}", "/bin/sleep", "120"]).decode().strip()
            created = socket.stat()
            identity = lambda st: (st.st_dev, st.st_ino, st.st_uid, st.st_ctime_ns)
            try:
                run(["set-window-option", "-t", "p07", "automatic-rename", "off"])
                run(["rename-window", "-t", "p07", "window 雪"])
                title = "~/Projects/whoate"
                run(["select-pane", "-t", pane, "-T", title])
                # tmux3.7c rejects control characters and leaves the prior title.
                # Compare against actual metadata, not an unaccepted setter input.
                run(["select-pane", "-t", pane, "-T", "rejected\tline\n\x1b"])
                actual_title = run(["display-message", "-p", "-t", pane, "#{pane_title}"])[:-1].decode()
                actual_window = run(["display-message", "-p", "-t", pane, "#{window_name}"])[:-1].decode()
                require(actual_title == title and actual_window == "window 雪", "TMUX_ORACLE_TITLE")
                print(json.dumps({"case":"tmux","requested_title":title,"native_title":actual_title,"native_window":actual_window},ensure_ascii=True),flush=True)
                wrapper_dir = self.directory / "bin"
                wrapper_dir.mkdir()
                wrapper = wrapper_dir / "tmux"
                import shlex
                wrapper.write_text("#!/bin/sh\nexec " + shlex.quote(tmux) + " -L whoate-p07 \"$@\"\n")
                wrapper.chmod(0o700)
                env["PATH"] = str(wrapper_dir) + os.pathsep + env.get("PATH", "")
                value, _ = self.query(["pane", pane, "--json"], env)
                check_pane_origins(value["owners"],actual_title,actual_window)
                redacted, _ = self.query(["pane", pane, "--json", "--redact"], env)
                require(all(o["origin"]["pane_title"] == "<redacted>" and o["origin"]["window_name"] == "<redacted>" for o in redacted["owners"]), "TMUX_REDACT_LEAK")
                text = subprocess.run([str(self.binary), "pane", pane], env=env, capture_output=True, timeout=15, stdin=subprocess.DEVNULL)
                require(text.returncode in [0, 1] and b"\x1b" not in text.stdout and b"\t" not in text.stdout and b"line\nend" not in text.stdout, "TMUX_TERMINAL_INJECTION")
                print("P07_TMUX_TITLES_MATCH=1")
            finally:
                if socket.exists():
                    require(identity(socket.stat()) == identity(created), "TMUX_SOCKET_IDENTITY_CHANGED")
                    run(["kill-server"])
                print("P07_OWNED_TMUX_CLEANUP_OK=1")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=pathlib.Path, default=pathlib.Path.cwd())
    parser.add_argument("--bin", type=pathlib.Path, required=True)
    parser.add_argument("--binding", type=pathlib.Path, required=True)
    parser.add_argument("--write-binding", action="store_true")
    parser.add_argument("--captures", type=pathlib.Path)
    parser.add_argument("--case", choices=["kernel", "startup", "reclaim", "tmux"])
    args = parser.parse_args()
    if args.write_binding:
        bind(args.root.resolve(), args.bin.resolve(), args.binding)
        return
    require(args.case, "CASE_REQUIRED")
    with tempfile.TemporaryDirectory(prefix="whoate-p07-") as directory:
        native = Native(args.root.resolve(), args.bin.resolve(), args.binding, pathlib.Path(directory), args.captures)
        require(hasattr(native, args.case), "P07_CASE_NOT_IMPLEMENTED:" + args.case)
        getattr(native, args.case)()
        print("P07_ACCEPTANCE_PASS case=" + args.case)


if __name__ == "__main__":
    main()
