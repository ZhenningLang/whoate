#!/usr/bin/env python3
"""Process-level, owned-resource acceptance. No transcript contents in diagnostics."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shutil
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import time
from agent_oracle import first_pair, fingerprint

ROOT = Path(__file__).resolve().parents[2]


def require(condition, code):
    if not condition:
        raise RuntimeError(code)


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def attest(binary):
    location = os.environ.get("WHOATE_BUILD_ATTESTATION")
    require(location, "BUILD_ATTESTATION_REQUIRED")
    value = json.loads(Path(location).read_text())
    require(value["binary"] == str(binary) and digest(binary) == value["sha256"], "BINARY_BINDING_MISMATCH")
    require(value["inputs"], "BUILD_INPUTS_EMPTY")
    for relative, expected in value["inputs"].items():
        path = (ROOT / relative).resolve()
        require(path.is_relative_to(ROOT), "BUILD_INPUT_OUTSIDE_ROOT")
        require(digest(path) == expected, "BUILD_INPUT_CHANGED")


def wait_for(predicate, code, seconds=8):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.025)
    raise RuntimeError(code)


def cleanup_child(child):
    if child.poll() is None:
        child.terminate()
        try:
            child.wait(timeout=2)
        except subprocess.TimeoutExpired:
            child.kill()
            child.wait(timeout=2)


def cleanup_owned_socket(path, receipt):
    if receipt is None:
        return  # No creation receipt grants no cleanup authority.
    try:
        info = path.lstat()
    except FileNotFoundError:
        return  # tmux may already have removed its own socket.
    current = (info.st_dev, info.st_ino, info.st_uid, info.st_ctime_ns)
    require(current == receipt and stat.S_ISSOCK(info.st_mode), "QA21_SOCKET_IDENTITY_CHANGED")
    path.unlink()


class Acceptance:
    def __init__(self, binary, root):
        self.binary, self.root = binary, root
        self.env = dict(os.environ, WHOATE_HOME_OVERRIDE=str(root / "home"),
                        WHOATE_CACHE_DIR=str(root / "cache"), WHOATE_RULES=str(root / "rules.toml"),
                        LC_ALL="en_US.UTF-8", NO_COLOR="1")
        (root / "home").mkdir()
        self.helper = root / "helper"
        result = subprocess.run(["/usr/bin/cc", "-O2", str(ROOT / "tests/cli/helper.c"), "-o", str(self.helper)],
                                capture_output=True, stdin=subprocess.DEVNULL, timeout=30)
        require(result.returncode == 0, "HELPER_BUILD_FAILED")

    def query(self, args, name, env=None, capture=True):
        if name in ("cached-start", "cache-missing"):
            state = subprocess.run(["/usr/sbin/sysctl", "vm.swapusage", "kern.memorystatus_vm_pressure_level"],
                                   capture_output=True, text=True, timeout=30)
            load = subprocess.run(["/usr/bin/uptime"], capture_output=True, text=True, timeout=30)
            require(state.returncode == 0 and load.returncode == 0, "STARTUP_ENV_CAPTURE_FAILED")
            print(f"STARTUP_ENV case={name} {load.stdout.strip()} {state.stdout.strip()}", flush=True)
        start = time.perf_counter()
        result = subprocess.run([str(self.binary), *args], env=env or self.env, capture_output=True,
                                stdin=subprocess.DEVNULL, timeout=40)
        elapsed = (time.perf_counter() - start) * 1000
        value = json.loads(result.stdout)
        require(value.get("schema") == "whoate/v1", name + "_SCHEMA_TAG")
        require(not value.get("error"), name + "_FATAL")
        if not capture:
            # Raw prompts stay in this process and the validator's stdin pipe.
            # Neither cargo output nor a validation error may echo input bytes.
            validation_env = dict(self.env, WHOATE_SCHEMA_STDIN="1")
            checked = subprocess.run(
                ["cargo", "test", "-p", "whoate-core", "--locked", "--test", "phase04_schema",
                 "phase04_schema_memory_only_input", "--", "--exact", "--nocapture"],
                cwd=ROOT, env=validation_env, input=result.stdout,
                stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=180)
            require(checked.returncode == 0 and b"1 passed; 0 failed" in checked.stdout,
                    "QA7_MEMORY_SCHEMA_FAILED")
            print("QA7_MEMORY_SCHEMA_OK=1", flush=True)
        if capture:
            directory = os.environ.get("WHOATE_CLI_CAPTURE_DIR")
            require(directory, "CAPTURE_DIRECTORY_REQUIRED")
            path = Path(directory) / (name + ".json")
            with open(path, "x", opener=lambda p, flags: os.open(p, flags, 0o600)) as stream:
                stream.write(result.stdout.decode())
        return value, result.returncode, elapsed

    def seed(self, age=0):
        result = subprocess.run([str(self.helper), "identity", "1"], capture_output=True, check=True)
        identity = json.loads(result.stdout)
        directory = Path(self.env["WHOATE_CACHE_DIR"])
        directory.mkdir(exist_ok=True)
        value = {"as_of": int(time.time()) - age, "rows": {"1": {
            "start_time": identity["start_time"], "uid": identity["uid"],
            "footprint_bytes": 8192, "cpu_pct_sampled": 12.5}}, "cpu_issue": None}
        (directory / "system.json").write_text(json.dumps(value))
        return value

    def cached_start(self):
        self.seed()
        value, code, elapsed = self.query(["top", "--json"], "cached-start")
        require(code in (0, 1), "QA12_EXIT")
        require(value["system_procs"]["status"] == "fresh", "QA12_NOT_FRESH")
        print(f"QA12 elapsed_ms={elapsed:.3f} status=fresh code={code}", flush=True)
        require(elapsed < 300, "QA12_START_GE_300MS")

    def warm_up(self):
        # One deliberately unmeasured execution, never a timed best-of trial.
        result = subprocess.run([str(self.binary), "top", "--json"], env=self.env,
                                capture_output=True, stdin=subprocess.DEVNULL, timeout=40)
        value = json.loads(result.stdout)
        require(result.returncode in (0, 1) and value.get("schema") == "whoate/v1"
                and not value.get("error"), "QA12_WARM_UP_FAILED")
        print("QA12_WARM_UP executions=1 timed=false", flush=True)

    def cache_states(self):
        value, code, elapsed = self.query(["top", "--json"], "cache-missing")
        print(f"QA13 missing_elapsed_ms={elapsed:.3f} code={code}", flush=True)
        require(elapsed < 500, "QA13_START_GE_500MS")
        require(value["system_procs"]["status"] in ("missing", "stale"), "QA13_MISSING_STATE")
        old = self.seed(age=120)
        value, _, _ = self.query(["top", "--json"], "cache-stale")
        require(value["system_procs"]["status"] == "stale", "QA13_OLD_CACHE_NOT_STALE")
        require(value["system_procs"]["as_of"] is not None, "QA13_AS_OF_LOST")
        from datetime import datetime
        require(int(datetime.fromisoformat(value["system_procs"]["as_of"]).timestamp()) == old["as_of"], "QA13_AS_OF_CHANGED")
        self.seed()
        value, _, _ = self.query(["why", "1", "--json", "--fresh"], "cache-fresh-bypass")
        require(value["system_procs"] == {"status": "missing", "as_of": None}, "QA13_FRESH_DID_NOT_BYPASS")
        require(any(i["code"] == "fresh_requested" for i in value["issues"]), "QA13_FRESH_ISSUE_MISSING")
        owner = next(o for o in value["owners"] if 1 in o["pids"])
        require(owner["cpu_pct"] is None, "QA13_FRESH_ROOT_CPU")
        require(owner["mem_bytes"] == 0 and owner["unknown_mem_count"] > 0, "QA13_FRESH_USED_CACHED_FOOTPRINT")
        require((Path(self.env["WHOATE_CACHE_DIR"]) / "system.json").is_file(), "QA13_FRESH_REMOVED_NORMAL_CACHE")
        self.watch_lifecycle()

    def watch_lifecycle(self):
        child = subprocess.Popen([str(self.binary), "watch", "--interval", "1s", "--json-lines"],
                                 env=self.env, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        frames = []
        try:
            for _ in range(2):
                require(select.select([child.stdout], [], [], 8)[0], "QA17_WATCH_NO_FRAME")
                frame = json.loads(child.stdout.readline())
                require(frame["schema"] == "whoate/v1", "QA17_WATCH_SCHEMA_TAG")
                require(all(o["agent"] is None or (o["agent"]["first_prompt"] is None and o["agent"]["first_reply"] is None)
                            for o in frame["owners"]), "QA17_WATCH_PROMPT_LEAK")
                frames.append(frame)
            child.terminate()
            require(child.wait(timeout=5) == 0, "QA17_SIGTERM_EXIT")
        finally:
            cleanup_child(child)
        path = Path(os.environ["WHOATE_CLI_CAPTURE_DIR"]) / "watch.jsonl"
        with open(path, "x", opener=lambda p, flags: os.open(p, flags, 0o600)) as stream:
            for frame in frames:
                stream.write(json.dumps(frame) + "\n")
        require(any(len(o["history"]) >= 2 for o in frames[-1]["owners"]), "QA17_PERSISTENT_ENGINE_HISTORY_MISSING")
        pipe = subprocess.Popen([str(self.binary), "watch", "--json-lines"], env=self.env,
                                stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        try:
            pipe.stdout.close()
            require(pipe.wait(timeout=5) == 0, "QA17_BROKEN_PIPE_EXIT")
        finally:
            cleanup_child(pipe)
        print("QA17 json_lines=2 persistent_history=1 sigterm=0 broken_pipe=0 cleanup=ok", flush=True)

    def kill(self):
        child = subprocess.Popen([str(self.helper)], stdout=subprocess.PIPE, stdin=subprocess.DEVNULL)
        try:
            require(select.select([child.stdout], [], [], 3)[0], "QA16_HELPER_NOT_READY")
            require(int(child.stdout.readline()) == child.pid, "QA16_HELPER_ID")
            # no --yes and --dry-run both preserve this owned child.
            for flags in ([], ["--yes", "--dry-run"]):
                preview = subprocess.run([str(self.binary), "kill", str(child.pid), *flags], env=self.env,
                                         capture_output=True, stdin=subprocess.DEVNULL, timeout=10)
                require(preview.returncode == 0 and child.poll() is None, "QA16_PREVIEW_SIGNALED")
            result = subprocess.run([str(self.binary), "kill", str(child.pid), "--yes"], env=self.env,
                                    capture_output=True, stdin=subprocess.DEVNULL, timeout=12)
            child.wait(timeout=2)
            require(result.returncode == 0 and b"terminated" in result.stdout, "QA16_OWNED_HELPER_NOT_TERMINATED")
        finally:
            cleanup_child(child)
        refused = subprocess.run([str(self.binary), "kill", "1", "--yes"], env=self.env,
                                  capture_output=True, stdin=subprocess.DEVNULL, timeout=10)
        require(refused.returncode == 4 and b"refused" in refused.stdout, "QA16_PID1_NOT_REFUSED")
        print("QA16 owned_helper_terminated=1 pid1_refused=1 cleanup=ok", flush=True)

    def burst(self):
        children = []
        try:
            for _ in range(30):
                children.append(subprocess.Popen(["/bin/sleep", "600"], stdin=subprocess.DEVNULL,
                                                 stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL))
            expected = {p.pid for p in children}
            value, code, _ = self.query(["swarms", "--json"], "burst")
            groups = [s for s in value["swarms"] if set(s["pids"]) == expected]
            require(len(groups) == 1, "QA22_EXACT_30_PID_GROUP_MISSING")
            group = groups[0]
            require(code == 0 and group["count"] == 30 and group["parents"] == [os.getpid()], "QA22_COUNT_PARENT")
            require(group["shape"] == "burst" and group["verdict"]["rule"] == "swarm-burst", "QA22_SHAPE_RULE")
        finally:
            for child in children:
                cleanup_child(child)
        print("QA22 exact_pid_count=30 parent=matched shape=burst cleanup=ok", flush=True)

    def orphan(self):
        # sockaddr_un on macOS cannot hold the normal (long) test scratch path.
        # This short, private directory is created and removed by this run only.
        with tempfile.TemporaryDirectory(prefix="wt-", dir="/private/tmp") as directory:
            self.orphan_at(Path(directory))

    def orphan_at(self, tmux_root):
        tmux = shutil.which("tmux")
        require(tmux, "QA21_TMUX_MISSING")
        tmux_env = dict(self.env, TMUX_TMPDIR=str(tmux_root))
        tmux_env.pop("TMUX", None)
        # Refuse an existing test server, even if no matching session is visible.
        probe = subprocess.run([tmux, "-L", "whoate-test", "list-sessions"], env=tmux_env, capture_output=True)
        require(probe.returncode != 0, "QA21_SOCKET_IN_USE")
        socket_dir = tmux_root / f"tmux-{os.getuid()}" / "whoate-test"
        require(not os.path.lexists(socket_dir), "QA21_SOCKET_ALREADY_EXISTS")
        check = socket.socket()
        try:
            check.bind(("127.0.0.1", 8797))
        except OSError:
            raise RuntimeError("QA21_PORT_8797_IN_USE") from None
        finally:
            check.close()
        pidfile = self.root / "http.pid"
        script = self.root / "http.py"
        script.write_text("import os,signal,sys\nsignal.signal(signal.SIGHUP,signal.SIG_IGN)\n"
                          f"open({str(pidfile)!r},'w').write(str(os.getpid()))\n"
                          "os.execl(sys.executable,sys.executable,'-m','http.server','8797','--bind','127.0.0.1')\n")
        pane = None
        pid = None
        expected_identity = None
        socket_receipt = None
        try:
            result = subprocess.run([tmux, "-L", "whoate-test", "new-session", "-d", "-s", "whoate", "-P", "-F", "#{pane_id}",
                                     sys.executable, str(script)], env=tmux_env, capture_output=True, timeout=10)
            require(result.returncode == 0, "QA21_CREATE_FAILED")
            pane = result.stdout.decode().strip()
            info = socket_dir.lstat()
            require(info.st_uid == os.getuid() and stat.S_ISSOCK(info.st_mode), "QA21_SOCKET_CREATION_IDENTITY")
            socket_receipt = (info.st_dev, info.st_ino, info.st_uid, info.st_ctime_ns)
            wait_for(pidfile.is_file, "QA21_PID_NOT_READY")
            pid = int(pidfile.read_text())
            expected_identity = json.loads(subprocess.check_output([str(self.helper), "identity", str(pid)]))
            def listening():
                with socket.socket() as connection:
                    return connection.connect_ex(("127.0.0.1", 8797)) == 0
            wait_for(listening, "QA21_LISTEN_NOT_READY")
            subprocess.run([tmux, "-L", "whoate-test", "kill-pane", "-t", pane], env=tmux_env, check=True, capture_output=True)
            def reparented():
                result = subprocess.run(["/bin/ps", "-p", str(pid), "-o", "ppid="], capture_output=True)
                return result.stdout.strip() == b"1"
            wait_for(reparented, "QA21_NOT_ADOPTED_BY_LAUNCHD")
            wrapper_dir = self.root / "bin"
            wrapper_dir.mkdir()
            wrapper = wrapper_dir / "tmux"
            import shlex
            wrapper.write_text("#!/bin/sh\nexec " + shlex.quote(tmux) + " -L whoate-test \"$@\"\n")
            wrapper.chmod(0o700)
            env = dict(tmux_env, PATH=str(wrapper_dir) + os.pathsep + self.env.get("PATH", ""))
            env.pop("TMUX", None)
            value, _, _ = self.query(["orphans", "--json"], "orphan", env=env)
            matches = [o for o in value["owners"] if pid in o["pids"]]
            require(len(matches) == 1, "QA21_PID_NOT_ORPHAN")
            origin = matches[0]["origin"]
            require(origin["tmux_pane"] == pane and origin["pane_alive"] is False and 8797 in origin["ports"], "QA21_ORIGIN_PORT")
        finally:
            # The server/pane was created by this run only; never touch owner servers.
            try:
                if pane:
                    subprocess.run([tmux, "-L", "whoate-test", "kill-session", "-t", "whoate"], env=tmux_env, capture_output=True, timeout=10)
                if pid is not None:
                    current = subprocess.run([str(self.helper), "identity", str(pid)], capture_output=True)
                    if current.returncode == 0:
                        require(expected_identity is not None and json.loads(current.stdout) == expected_identity,
                                "QA21_CLEANUP_IDENTITY_CHANGED")
                        os.kill(pid, signal.SIGTERM)
                        wait_for(lambda: subprocess.run([str(self.helper), "identity", str(pid)], capture_output=True).returncode != 0,
                                 "QA21_CLEANUP_STILL_ALIVE")
            finally:
                if socket_receipt is not None:
                    probe = subprocess.run([tmux, "-L", "whoate-test", "list-sessions"], env=tmux_env, capture_output=True, timeout=10)
                    require(probe.returncode != 0, "QA21_OWNED_SERVER_STILL_ACTIVE")
                    cleanup_owned_socket(socket_dir, socket_receipt)
        print("QA21 pid_origin_port=matched cleanup=ok", flush=True)

    def agent(self):
        require(os.environ.get("WHOATE_TEST_CLAUDE_BODY_AUTHORIZED") == "1",
                "QA23_TRANSCRIPT_BODY_AUTHORIZATION_REQUIRED")
        raw_pid = os.environ.get("WHOATE_TEST_CLAUDE_PID")
        require(raw_pid == "61861", "QA23_ASSOCIATED_CLAUDE_PID_REQUIRED")
        pid = int(raw_pid)
        identity = subprocess.run(["/bin/ps", "-o", "command=", "-p", str(pid)], capture_output=True)
        require(identity.returncode == 0 and
                b"--resume cbf6ddcc-8b8c-40d0-b162-79a84efa3daa" in identity.stdout,
                "QA23_RESUME_SESSION_MISMATCH")
        home = Path(os.environ["HOME"])
        source = json.loads((home / ".claude/sessions" / f"{pid}.json").read_text())
        session_id = source["sessionId"]
        require(session_id and "/" not in session_id and "\\" not in session_id, "QA23_BAD_SESSION_ID")
        require(session_id == "cbf6ddcc-8b8c-40d0-b162-79a84efa3daa", "QA23_RESUME_SESSION_MISMATCH")
        slug = re.sub(r"[^A-Za-z0-9-]", "-", source["cwd"])
        path = home / ".claude/projects" / slug / (session_id + ".jsonl")
        # Unbuffered: readline must not prefetch unauthorized later records.
        with path.open("rb", buffering=0) as stream:
            expected = first_pair(stream)
        # The limited reader uses only session metadata for title. A transcript
        # title occurring after the first pair is outside this capability.
        expected_title = source.get("title")
        env = dict(self.env)
        env.pop("WHOATE_HOME_OVERRIDE", None)
        env["WHOATE_SESSION_PID"] = raw_pid
        default, _, _ = self.query(["agents", "--json"], "agent-default", env=env, capture=False)
        agent = next(o["agent"] for o in default["owners"] if pid in o["pids"])
        require(agent["first_prompt"] is None and agent["first_reply"] is None, "QA23_DEFAULT_PROMPT_LEAK")
        value, _, _ = self.query(["agents", "--prompts", "--json"], "agent-authorized", env=env, capture=False)
        agent = next(o["agent"] for o in value["owners"] if pid in o["pids"])
        require(agent["name"] == source.get("name") and agent["title"] == expected_title,
                "QA23_METADATA_MISMATCH")
        actual = {key: fingerprint(agent[key]) for key in expected}
        print("QA23 fingerprints=" + json.dumps(actual, sort_keys=True), flush=True)
        require(actual == expected, "QA23_FIRST_PAIR_HASH_MISMATCH")
        print("QA23 name_title=matched first_pair_sha256_chars=matched default_null=1", flush=True)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--bin", type=Path, required=True)
    parser.add_argument("--case", required=True, choices=["warm-up", "cached-start", "cache-states", "kill", "burst", "orphan", "agent"])
    args = parser.parse_args()
    binary = args.bin.resolve()
    try:
        attest(binary)
        with tempfile.TemporaryDirectory(prefix="whoate-acceptance-", dir=os.environ.get("WHOATE_VERIFY_TMPDIR")) as directory:
            runner = Acceptance(binary, Path(directory))
            getattr(runner, args.case.replace("-", "_"))()
        print(f"ACCEPTANCE_PASS case={args.case}", flush=True)
    except Exception as error:
        # Never print exception arguments: parser errors may include private data.
        code = str(error) if isinstance(error, RuntimeError) and re.fullmatch(r"[A-Z0-9_]+", str(error)) else type(error).__name__
        print(f"ACCEPTANCE_FAIL case={args.case} code={code}", file=sys.stderr, flush=True)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
