#!/usr/bin/env python3
# window_check.without_input reruns a focus check only when someone used the
# keyboard or pointer during it (full run 2026-10-25 00:48: every focus loss came
# right after a trackpad click). Without input the first result is final,
# whoever is frontmost; the assertions themselves are unchanged.
import os, sys, tempfile, unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import phase10_checks, window_check


class Clock:
    def __init__(self):
        self.t = 0.0
    def __call__(self):
        return self.t


class WithoutInput(unittest.TestCase):
    def run_with(self, results, idles):
        # results: (ok, run seconds) per attempt; idles: idle readings in call order.
        out, clock, idle_values, calls = tempfile.mkdtemp(), Clock(), list(idles), []

        def scenario(attempt):
            calls.append(attempt)
            ok, seconds = results[attempt - 1]
            clock.t += seconds
            path = os.path.join(out, "receipt.json")
            open(path, "w").write("{}")
            return ok, path

        with mock.patch("builtins.print"):
            ok = window_check.without_input(scenario, "T", idle=lambda: idle_values.pop(0),
                                            sleep=lambda s: setattr(clock, "t", clock.t + s), clock=clock)
        return ok, calls, sorted(os.listdir(out))

    def test_input_during_the_run_runs_again_and_keeps_the_receipt(self):
        # quiet before (5), touched at end (1 < 10 s run), quiet before (5), untouched at end (30).
        ok, calls, files = self.run_with([(False, 10), (True, 10)], [5, 1, 5, 30])
        self.assertEqual((ok, calls), (True, [1, 2]))
        self.assertEqual(files, ["receipt.human-input-1.json", "receipt.json"])

    def test_a_failure_without_input_is_final(self):
        ok, calls, files = self.run_with([(False, 10), (True, 10)], [5, 30])
        self.assertEqual((ok, calls, files), (False, [1], ["receipt.json"]))

    def test_input_in_every_run_is_not_a_pass(self):
        runs = window_check.FOCUS_RUNS
        ok, calls, _ = self.run_with([(True, 10)] * runs, [5, 1] * runs)
        self.assertEqual((ok, calls), (False, list(range(1, runs + 1))))

    def test_it_waits_for_a_quiet_moment_before_running(self):
        ok, calls, _ = self.run_with([(True, 10)], [0, 1, 2, 3.5, 30])
        self.assertEqual((ok, calls), (True, [1]))

    def test_panel_goes_through_it(self):
        self.assertIn("without_input(", open(phase10_checks.__file__).read())


if __name__ == "__main__":
    unittest.main()
