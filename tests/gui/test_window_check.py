"""QA13 timing oracle: one 5 s budget from the launch request. No GUI process starts here."""
import contextlib, io, json, os, tempfile, unittest
import window_check


class FakeClock:
    def __init__(self):
        self.now = 100.0

    def __call__(self):
        return self.now


class FakeRunner:
    """Resolves after `resolve_s`; the app exits `exit_s` after the launch request."""

    def __init__(self, clock, resolve_s, exit_s):
        self.clock, self.resolve_s, self.exit_at, self.stopped = clock, resolve_s, clock.now + exit_s, []

    def start(self, binary, env=None):
        self.clock.now += self.resolve_s
        with open(env["WHOATE_UI_STATE_FILE"], "w") as f:
            f.write("1.0\tpolicy=regular\tmain_windows=1\tsince_launch=0.5\n")
        return {"pid": 4242, "start": 1, "path": "/w/Whoate.app/Contents/MacOS/Whoate", "workdir": tempfile.mkdtemp()}

    def wait(self, handle, timeout_s):
        if self.exit_at <= self.clock.now + timeout_s:
            self.clock.now = max(self.clock.now, self.exit_at)
            return True
        self.clock.now += timeout_s
        return False

    def alive(self, handle):
        return self.clock.now < self.exit_at

    def stop(self, handle):
        self.stopped.append(handle["pid"])
        self.exit_at = self.clock.now
        return "terminated"


class QuitTimingTests(unittest.TestCase):
    def run_check(self, resolve_s, exit_s, others=()):
        clock = FakeClock()
        runner = FakeRunner(clock, resolve_s, exit_s)
        with tempfile.TemporaryDirectory(prefix="whoate-quit-test-") as out:
            with contextlib.redirect_stdout(io.StringIO()):
                ok = window_check.quit_check("/w/Whoate", out, runner=runner, clock=clock,
                                             lister=lambda _: [{"pid": p} for p in others],
                                             entry_for=lambda pid: None, sleep=lambda _: None)
            with open(os.path.join(out, "qa13-quit.json")) as f:
                receipt = json.load(f)
        return ok, receipt, runner

    def test_exit_within_five_seconds_of_the_launch_request_passes(self):
        ok, receipt, _ = self.run_check(resolve_s=1, exit_s=4)
        self.assertTrue(ok)
        self.assertEqual(receipt["seconds_from_launch_request"], 4)

    def test_seven_seconds_fails_even_though_the_quit_timer_is_three(self):
        ok, receipt, runner = self.run_check(resolve_s=1, exit_s=7)
        self.assertFalse(ok, "the 3 s quit timer is not added to the 5 s budget")
        self.assertFalse(receipt["checks"]["exited_within_5s"])
        self.assertEqual(runner.stopped, [4242], "a late app is stopped, by its own pid only")

    def test_slow_pid_resolution_counts_against_the_budget(self):
        ok, receipt, _ = self.run_check(resolve_s=4, exit_s=5.5)
        self.assertFalse(ok)

    def test_an_owner_instance_already_running_is_not_a_leftover(self):
        ok, receipt, _ = self.run_check(resolve_s=1, exit_s=4, others=[555])
        self.assertTrue(ok)
        self.assertEqual(receipt["other_instances"], [555])
        self.assertEqual(receipt["left"], [])


if __name__ == "__main__":
    unittest.main()
