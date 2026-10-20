#!/usr/bin/env python3
"""QA14 integration: WhoateKit's tmux jump against a private tmux server.

A server in a fresh TMUX_TMPDIR (never the caller's default server) gets two
windows; `script` attaches one client to window 0. WhoateChecks, started
through launch.py with WHOATE_TMUX_JUMP=<pane of window 1>, runs the real
executor (argument arrays, no shell). Pass: the client's current pane is the
target. Only this script's client, server and socket directory are removed,
and the cleanup result is printed.

  tmux_jump.py <WhoateChecks executable>
"""
import os, shutil, subprocess, sys, tempfile, time
import launch

TMUX = shutil.which("tmux", path=os.environ.get("PATH", "") + ":/opt/homebrew/bin:/usr/local/bin")


def main():
    checks = sys.argv[1]
    root = tempfile.mkdtemp(prefix="wtj-", dir="/private/tmp")
    env = {k: v for k, v in os.environ.items() if k not in ("TMUX", "TMUX_PANE")}
    env.update(TMUX_TMPDIR=root, TERM="xterm-256color")
    tmux = lambda *args: subprocess.run([TMUX, *args], env=env, capture_output=True, text=True, timeout=20)
    client = None
    ok = False
    try:
        assert tmux("new-session", "-d", "-s", "jump", "-x", "80", "-y", "24").returncode == 0, "new-session"
        assert tmux("new-window", "-t", "jump").returncode == 0, "new-window"
        panes = tmux("list-panes", "-s", "-t", "jump", "-F", "#{window_index}\t#{pane_id}").stdout.split()
        target = dict(zip(panes[0::2], panes[1::2]))["1"]
        tmux("select-window", "-t", "jump:0")
        client = subprocess.Popen(["/usr/bin/script", "-q", "/dev/null", TMUX, "attach", "-t", "jump"], env=env,
                                  stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        tty = ""
        for _ in range(100):
            tty = tmux("list-clients", "-F", "#{client_tty}").stdout.strip()
            if tty:
                break
            time.sleep(0.1)
        assert tty, "no attached client"
        before = tmux("display", "-p", "-c", tty, "#{pane_id}").stdout.strip()
        assert before != target, f"client already on {target}"
        exited, out, handle = launch.Launcher().run(checks, env={"WHOATE_TMUX_JUMP": target, "TMUX_TMPDIR": root},
                                                    timeout_s=60)
        launch.cleanup(handle)
        after = tmux("display", "-p", "-c", tty, "#{pane_id}").stdout.strip()
        ok = exited and "TMUX_JUMP jumped" in out and after == target
        print(f"QA14 jump target={target} before={before} after={after} app={out.strip()!r} ok={ok}")
    finally:
        cleaned = cleanup(tmux, client, root)
        print("QA14 cleanup " + " ".join(f"{k}={v}" for k, v in cleaned.items()))
    sys.exit(0 if passed(ok, cleaned) else 1)


def cleanup(tmux, client, root, rmtree=shutil.rmtree, exists=os.path.exists):
    """Stops this script's client, server and socket directory; reports each result."""
    if client:
        client.terminate()
        try:
            client.wait(timeout=5)
        except subprocess.TimeoutExpired:
            client.kill()
            try:
                client.wait(timeout=5)
            except subprocess.TimeoutExpired:
                pass
    tmux("kill-server")
    # A server that already exited counts as gone; only a reachable one is a leftover.
    server_gone = tmux("has-session").returncode != 0
    rmtree(root, ignore_errors=True)
    return {"client_stopped": client is None or client.poll() is not None,
            "private_server_killed": server_gone, "socket_dir_removed": not exists(root)}


def passed(jump_ok, cleaned):
    """QA14 passes only when the jump worked and every cleanup step did (review A:B2)."""
    return bool(jump_ok) and all(cleaned.values())


if __name__ == "__main__":
    main()
