"""Regression locks for acceptance harness failures (no real processes/signals)."""
import contextlib
import importlib.util
import io
import json
import os
import socket
from pathlib import Path
import tempfile
import types
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location("whoate_acceptance", Path(__file__).with_name("acceptance.py"))
acceptance = importlib.util.module_from_spec(spec)
spec.loader.exec_module(acceptance)


class DriverTests(unittest.TestCase):
    def test_agent_installs_source_limit_before_both_cli_queries_without_tail_reads(self):
        runner = acceptance.Acceptance.__new__(acceptance.Acceptance)
        runner.env = {"WHOATE_HOME_OVERRIDE": "/isolated"}
        session = "cbf6ddcc-8b8c-40d0-b162-79a84efa3daa"
        metadata = json.dumps({"sessionId": session, "cwd": "/demo", "name": "Test"})
        transcript = (b'{"type":"user","message":{"content":"Hello"}}\n'
                      b'{"type":"assistant","message":{"content":"World"}}\n')
        class NoTail(io.BytesIO):
            def seek(self, *args):
                raise AssertionError("TAIL_READ_FORBIDDEN")
        identity = types.SimpleNamespace(stdout=("claude --resume " + session).encode(), returncode=0)
        calls = []
        def query(args, name, env, capture):
            calls.append(env)
            return {"owners": [{"pids": [61861], "agent": {
                "name": "Test", "title": None,
                "first_prompt": "Hello" if "--prompts" in args else None,
                "first_reply": "World" if "--prompts" in args else None,
            }}]}, 0, 0
        runner.query = query
        with patch.dict(os.environ, {"WHOATE_TEST_CLAUDE_BODY_AUTHORIZED": "1", "WHOATE_TEST_CLAUDE_PID": "61861"}), \
                patch.object(acceptance.subprocess, "run", return_value=identity), \
                patch.object(Path, "read_text", return_value=metadata), \
                patch.object(Path, "open", side_effect=lambda *a, **kw: NoTail(transcript)), \
                contextlib.redirect_stdout(io.StringIO()):
            runner.agent()
        self.assertEqual([env.get("WHOATE_SESSION_PID") for env in calls], ["61861", "61861"])
        self.assertTrue(all("WHOATE_HOME_OVERRIDE" not in env for env in calls))

    def test_agent_rejects_a_different_resumed_session_before_any_file_open(self):
        runner = acceptance.Acceptance.__new__(acceptance.Acceptance)
        result = types.SimpleNamespace(stdout=b"claude --resume another-session", returncode=0)
        with patch.dict(os.environ, {"WHOATE_TEST_CLAUDE_BODY_AUTHORIZED": "1",
                                    "WHOATE_TEST_CLAUDE_PID": "61861"}), \
                patch.object(acceptance.subprocess, "run", return_value=result), \
                patch.object(Path, "read_text", side_effect=AssertionError("UNAUTHORIZED_OPEN")):
            with self.assertRaisesRegex(RuntimeError, "QA23_RESUME_SESSION_MISMATCH"):
                runner.agent()

    def test_cleanup_only_removes_the_recorded_closed_socket(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "owned.sock"
            with socket.socket(socket.AF_UNIX) as child:
                # sockaddr_un is short on macOS; use the same owned directory,
                # not a different temp root or a weakened identity assertion.
                with contextlib.chdir(directory):
                    child.bind(path.name)
                info = path.lstat()
                receipt = (info.st_dev, info.st_ino, info.st_uid, info.st_ctime_ns)
            acceptance.cleanup_owned_socket(path, receipt)
            self.assertFalse(path.exists())

    def test_cleanup_refuses_replaced_socket_and_preserves_unknown_socket(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "owned.sock"
            with socket.socket(socket.AF_UNIX) as child:
                with contextlib.chdir(directory):
                    child.bind(path.name)
                info = path.lstat()
                receipt = (info.st_dev, info.st_ino, info.st_uid, info.st_ctime_ns)
            acceptance.cleanup_owned_socket(path, None)
            self.assertTrue(path.exists())
            path.unlink()  # This test created it and has already closed its descriptor.
            path.write_text("KEEP_REPLACEMENT")
            with self.assertRaisesRegex(RuntimeError, "QA21_SOCKET_IDENTITY_CHANGED"):
                acceptance.cleanup_owned_socket(path, receipt)
            self.assertEqual(path.read_text(), "KEEP_REPLACEMENT")

    def test_query_captures_same_json_with_private_permissions(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = acceptance.Acceptance.__new__(acceptance.Acceptance)
            runner.binary = Path(directory) / "unused-binary"
            runner.env = {}
            raw = b'{"schema":"whoate/v1","owners":[]}\n'
            result = types.SimpleNamespace(stdout=raw, stderr=b"", returncode=0)
            with patch.dict(os.environ, {"WHOATE_CLI_CAPTURE_DIR": directory}), patch.object(acceptance.subprocess, "run", return_value=result):
                value, code, _ = runner.query(["top", "--json"], "example")
            path = Path(directory) / "example.json"
            self.assertEqual(path.read_bytes(), raw)
            self.assertEqual(path.stat().st_mode & 0o777, 0o600)
            self.assertEqual(value, json.loads(raw))
            self.assertEqual(code, 0)

    def test_uncaptured_query_validates_the_exact_bytes_in_memory_only(self):
        with tempfile.TemporaryDirectory() as directory:
            runner = acceptance.Acceptance.__new__(acceptance.Acceptance)
            runner.binary = Path(directory) / "unused-binary"
            runner.env = {}
            raw = b'{"schema":"whoate/v1","owners":[],"private":"MEMORY_ONLY"}\n'
            result = types.SimpleNamespace(stdout=raw, stderr=b"", returncode=0)
            valid = types.SimpleNamespace(stdout=b"1 passed; 0 failed", stderr=b"", returncode=0)
            with patch.object(acceptance.subprocess, "run", side_effect=[result, valid]) as run:
                runner.query(["agents", "--prompts", "--json"], "authorized", capture=False)
            self.assertEqual(run.call_count, 2)
            self.assertEqual(run.call_args.kwargs["input"], raw)
            self.assertEqual(list(Path(directory).iterdir()), [])

    def test_missing_attestation_reports_safe_failure_not_input_contents(self):
        output = io.StringIO()
        with patch.dict(os.environ, {}, clear=True), patch("sys.argv", ["acceptance.py", "--case", "agent", "--bin", "/PRIVATE_SENTINEL/binary"]), contextlib.redirect_stderr(output):
            self.assertEqual(acceptance.main(), 1)
        self.assertIn("BUILD_ATTESTATION_REQUIRED", output.getvalue())
        self.assertNotIn("PRIVATE_SENTINEL", output.getvalue())


if __name__ == "__main__":
    unittest.main()
