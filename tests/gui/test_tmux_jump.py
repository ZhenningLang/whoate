"""tmux_jump.py: a failed cleanup fails QA14 even when the jump worked (review A:B2)."""
import os, sys, unittest
from types import SimpleNamespace

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import tmux_jump


class Client:
    def __init__(self, stops=True):
        self.stops, self.done = stops, False
    def terminate(self):
        self.done = self.stops
    def kill(self):
        self.done = self.stops
    def wait(self, timeout):
        if not self.done:
            raise tmux_jump.subprocess.TimeoutExpired("script", timeout)
    def poll(self):
        return 0 if self.done else None


def fake_tmux(server_alive):
    return lambda *args: SimpleNamespace(returncode=0 if args[0] == "has-session" and server_alive else 1)


class CleanupTests(unittest.TestCase):
    def test_everything_cleaned_passes(self):
        cleaned = tmux_jump.cleanup(fake_tmux(False), Client(), "/x", rmtree=lambda *a, **k: None, exists=lambda p: False)
        self.assertEqual(cleaned, {"client_stopped": True, "private_server_killed": True, "socket_dir_removed": True})
        self.assertTrue(tmux_jump.passed(True, cleaned))

    def test_server_left_or_socket_left_or_client_alive_fails(self):
        for tmux, client, left in [(fake_tmux(True), Client(), False), (fake_tmux(False), Client(), True),
                                   (fake_tmux(False), Client(stops=False), False)]:
            cleaned = tmux_jump.cleanup(tmux, client, "/x", rmtree=lambda *a, **k: None, exists=lambda p: left)
            self.assertFalse(tmux_jump.passed(True, cleaned), cleaned)

    def test_failed_jump_fails(self):
        self.assertFalse(tmux_jump.passed(False, {"client_stopped": True}))


if __name__ == "__main__":
    unittest.main()
