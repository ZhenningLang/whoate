#!/usr/bin/env python3
# phase10_checks.panel reruns only when another app held focus at check time
# (full run 2026-10-25 00:16: One Markdown, then a Space switch to Ghostty).
# A failure while whoate is frontmost is final; the assertion itself is unchanged.
import os, sys, unittest
from unittest import mock

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import phase10_checks


class PanelRerun(unittest.TestCase):
    def run_with(self, results):
        calls = []

        def once(binary, out, attempt):
            calls.append(attempt)
            return results[attempt - 1]

        with mock.patch.object(phase10_checks, "panel_once", once), mock.patch("builtins.print"):
            return phase10_checks.panel("bin", "out"), calls

    def test_focus_taken_by_another_app_runs_again(self):
        ok, calls = self.run_with([(False, "other:com.mitchellh.ghostty"), (True, None)])
        self.assertEqual((ok, calls), (True, [1, 2]))

    def test_failure_with_whoate_frontmost_is_final(self):
        ok, calls = self.run_with([(False, None), (True, None)])
        self.assertEqual((ok, calls), (False, [1]))

    def test_focus_lost_in_every_run_fails(self):
        lost = (False, "other:com.example.app")
        ok, calls = self.run_with([lost] * phase10_checks.PANEL_RUNS)
        self.assertEqual((ok, calls), (False, list(range(1, phase10_checks.PANEL_RUNS + 1))))


if __name__ == "__main__":
    unittest.main()
