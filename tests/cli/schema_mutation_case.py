#!/usr/bin/env python3
"""Deterministic same-capture Schema oracle for the formal mutation runner.

The caller owns the heavy slot. Only public synthetic fixture bytes are written.
Refresh Rust inputs before building the subprocess binary: a previous mutation
may have restored source mtimes while leaving a stale binary in target/.
"""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--test", required=True)
    parser.add_argument("--capture-fixture", action="store_true")
    args = parser.parse_args()
    root = Path(__file__).resolve().parents[2]
    for path in root.glob("crates/**/*.rs"):
        path.touch()
    built = subprocess.run(["cargo", "build", "-p", "whoate-cli", "--locked"], cwd=root)
    if built.returncode:
        return built.returncode
    result = subprocess.run(["cargo", "metadata", "--locked", "--format-version", "1", "--no-deps"],
                            cwd=root, capture_output=True, check=True)
    target = Path(json.loads(result.stdout)["target_directory"])
    env = dict(os.environ, WHOATE_CLI_BIN=str(target / "debug/whoate"))
    env.pop("WHOATE_CLI_CAPTURE_DIR", None)
    with tempfile.TemporaryDirectory(prefix="whoate-schema-mutation-") as directory:
        if args.capture_fixture:
            path = Path(directory) / "synthetic.json"
            # Captures use one complete JSON value per line, like CLI stdout.
            fixture = json.loads((root / "tests/fixtures/attribution/sample-v1.json").read_bytes())
            path.write_text(json.dumps(fixture, separators=(",", ":")) + "\n")
            path.chmod(0o600)
            env["WHOATE_CLI_CAPTURE_DIR"] = directory
        return subprocess.run(["cargo", "test", "-p", "whoate-core", "--locked", "--test", "phase04_schema",
                               args.test, "--", "--exact", "--nocapture"], cwd=root, env=env).returncode


if __name__ == "__main__":
    raise SystemExit(main())
