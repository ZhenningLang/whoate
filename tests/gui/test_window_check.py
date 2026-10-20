"""QA13 timing oracle, QA31/QA34 layout parsing and the QA33 hidden-scenario
oracle. No GUI process starts here."""
import contextlib, io, json, os, tempfile, unittest
import hidden_scenario, window_check


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


class LayoutTests(unittest.TestCase):
    def test_frames_and_curve_kinds_are_read_apart(self):
        with tempfile.NamedTemporaryFile("w", suffix=".layout", delete=False) as f:
            f.write("orphan:1\tbuttons\t300.0\t40.0\t90.0\t20.0\norphan:1\tcurve\tline\t1791978001.5\n"
                    "orphan:1\ttrend\t200.0\t10.0\t64.0\t18.0\napp:2\tcurve\tinsufficient\t1791978002.0\n")
        try:
            rows, curves = window_check.read_layout(f.name)
        finally:
            os.remove(f.name)
        self.assertEqual(rows["orphan:1"]["buttons"], (300.0, 40.0, 90.0, 20.0))
        self.assertFalse(window_check.intersects(rows["orphan:1"]["buttons"], rows["orphan:1"]["trend"]))
        self.assertEqual(curves, {"orphan:1": ("line", 1791978001.5), "app:2": ("insufficient", 1791978002.0)})


class HiddenScenarioTests(unittest.TestCase):
    """QA33: a hidden run counts only if it really was the 30 s hidden state."""
    START, END = 1000.0, 1062.0

    def receipt(self, comms=("/usr/bin/tmux",), every=1, seconds=range(0, 62)):
        return {"measure_started_epoch": self.START, "measure_ended_epoch": self.END, "launched_epoch": 990.0,
                "descendant_comms": [{"epoch": self.START + i, "comms": list(comms)} for i in seconds[::every]]}

    def cold(self, hidden=(990.5, 1020.6, 1050.9)):
        rows = [(t, "hidden-full") for t in hidden]
        rows += [(t, "host") for t in range(995, 1060, 5) if all(abs(t - h) > 1 for h in hidden)]
        return sorted(rows)

    def test_a_real_cold_hidden_run_is_valid(self):
        verdict = hidden_scenario.judge("cold-hidden", self.receipt(), self.cold())
        self.assertTrue(verdict["ok"], verdict)
        self.assertEqual(verdict["hidden_full_intervals"], [30.3])
        self.assertEqual(verdict["engine_first_used_s"], 0.5)
        self.assertTrue(verdict["engine_before_window"])

    def test_each_invalid_run_fails(self):
        cases = {
            "one hidden-full": ("cold-hidden", self.receipt(), self.cold(hidden=(990.5, 1020.6))),
            "too far apart": ("cold-hidden", self.receipt(), self.cold(hidden=(990.5, 1005.0, 1050.0))),
            "full inside": ("cold-hidden", self.receipt(), self.cold() + [(1030.0, "full")]),
            "resume inside": ("cold-hidden", self.receipt(), self.cold() + [(1031.0, "resume")]),
            "top child": ("cold-hidden", self.receipt(comms=("/usr/bin/top",)), self.cold()),
            "not listed": ("cold-hidden", self.receipt(every=5), self.cold()),
            # Review round 3 A:B1: 31 listings, all in the first half; the last 32 s unseen.
            "first half only": ("cold-hidden", self.receipt(seconds=range(0, 31)), self.cold()),
            "gap in the middle": ("cold-hidden", self.receipt(seconds=[*range(0, 20), *range(23, 62)]), self.cold()),
            "late first listing": ("cold-hidden", self.receipt(seconds=range(3, 62)), self.cold()),
            "listing failed": ("cold-hidden", dict(self.receipt(), descendant_comms=self.receipt()["descendant_comms"]
                                                   + [{"epoch": 1030.5, "error": "ps failed"}]), self.cold()),
            "no window": ("cold-hidden", {}, self.cold()),
            "main never opened": ("hidden-after-main", self.receipt(), self.cold()),
            "pause inside": ("hidden-after-main", self.receipt(),
                             self.cold() + [(985.0, "full"), (1001.0, "pause")]),
        }
        for name, (kind, receipt, rows) in cases.items():
            self.assertFalse(hidden_scenario.judge(kind, receipt, sorted(rows))["ok"], name)

    def test_listing_coverage_is_the_longest_unlisted_stretch(self):
        self.assertEqual(hidden_scenario.coverage([{"epoch": 1000.0 + i} for i in range(0, 31)], 1000.0, 1062.0), 32)
        self.assertEqual(hidden_scenario.coverage([], 1000.0, 1062.0), 62)
        seconds = [0, 1.2, 2.4, 3.6]
        ok = hidden_scenario.judge("cold-hidden", dict(self.receipt(), descendant_comms=[
            {"epoch": self.START + t, "comms": []} for t in [*seconds, *range(5, 63, 2)]]), self.cold())
        self.assertTrue(ok["checks"]["descendants_listed"], ok)

    def test_hidden_after_main_needs_full_then_pause_before_the_window(self):
        rows = self.cold() + [(986.0, "resume"), (986.1, "full"), (988.1, "full"), (990.2, "pause")]
        self.assertTrue(hidden_scenario.judge("hidden-after-main", self.receipt(), sorted(rows))["ok"])


if __name__ == "__main__":
    unittest.main()
