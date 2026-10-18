import unittest
import json
from pathlib import Path
import subprocess
import sys
import tempfile
from unittest.mock import patch
import cpu_sample as meter


class MeterTests(unittest.TestCase):
    def test_privileged_gap_requires_real_parent_credit_not_zero(self):
        gaps = [dict(pid=2, parent_identity=1, credit_floor_ns=10)]
        root = {"1": dict(self_ns=100, child_ns=10, identity=1)}
        self.assertFalse(meter.reconcile_gaps(gaps, root, 1, 1))
        credited = {"1": dict(self_ns=110, child_ns=30, identity=1)}
        self.assertTrue(meter.reconcile_gaps(gaps, credited, 1, 2))
        self.assertEqual(meter.tree_delta(root, credited, 1)[1], 20)
        self.assertEqual(gaps[0]["parent_credit_after_ns"], 30)

    def test_unchanged_child_credit_does_not_acquire_phantom_negative_cpu(self):
        before = {"1": dict(self_ns=765583041.6666666, child_ns=1387053125.0, identity=1)}
        after = {"1": dict(self_ns=768000083.3333333, child_ns=1387053125.0, identity=1)}
        self.assertEqual(meter.tree_delta(before, after, 1)[1], 0)

    def test_child_transfer_during_snapshot_is_not_double_counted(self):
        def children(_):
            children.calls += 1
            return [{"pid": 2, "state": "S"}] if children.calls == 1 else []
        children.calls = 0
        def read(pid):
            return dict(self_ns=100 if pid == 1 else 20, child_ns=20 if pid == 1 else 0, identity=pid)
        with patch.object(meter, "descendants", children), patch.object(meter, "rusage", read):
            snapshot = meter.tree_sample(1)
        self.assertEqual(set(snapshot), {"1"}, "a reaped child cannot remain in the same cut as its parent credit")
        self.assertEqual(snapshot["1"]["child_ns"], 20)

    def test_persistent_missing_child_counter_fails_after_bounded_cuts(self):
        attempts = []
        def read(pid):
            if pid == 2:
                attempts.append(pid)
                raise ValueError("rusage_unavailable pid=2")
            return dict(self_ns=100, child_ns=0, identity=1)
        with patch.object(meter, "descendants", return_value=[{"pid": 2, "state": "S"}]), patch.object(meter, "rusage", read):
            with self.assertRaisesRegex(ValueError, "cpu_snapshot_unstable"):
                meter.tree_sample(1)
        self.assertEqual(len(attempts), 8)

    def test_high_child_low_self_is_not_a_pass(self):
        result = meter.evaluate([0] + [0.1] * 60, 61, 30.0, 1)
        self.assertFalse(result["pass"])
        self.assertAlmostEqual(result["total"], 30.1)

    def test_threshold_is_strict_and_unrounded(self):
        self.assertFalse(meter.evaluate([0] + [0.5] * 60, 61, 0.5, 1)["pass"])
        self.assertTrue(meter.evaluate([0] + [0.4999] * 60, 61, 0.5, 1)["pass"])

    def test_missing_measurement_is_never_zero(self):
        for children in [None, float("nan"), -0.1]:
            with self.assertRaises(ValueError):
                meter.evaluate([0] + [0] * 60, 61, children, 1)
        with self.assertRaises(ValueError):
            meter.evaluate([0] * 60, 61, 0, 1)

    def test_live_reaped_and_grandchild_credits_are_disjoint(self):
        row = lambda own, children=0, identity=1: dict(self_ns=own, child_ns=children, identity=identity)
        # Before: root, live child and grandchild. After: child waited for its
        # grandchild, root waited for child. Exactly the same subtree moves up.
        before = {"1": row(10), "2": row(20), "3": row(30)}
        after = {"1": row(15, 70)}
        self.assertEqual(meter.tree_delta(before, after, 1), (5, 20))
        live_after = {"1": row(15), "2": row(30, 40)}
        self.assertEqual(meter.tree_delta(before, live_after, 1), (5, 20))
        grandchild_after = {"1": row(15), "2": row(30), "3": row(40)}
        self.assertEqual(meter.tree_delta(before, grandchild_after, 1), (5, 20))

    def test_identity_reuse_or_lost_cpu_fails_closed(self):
        row = lambda own, identity: dict(self_ns=own, child_ns=0, identity=identity)
        for after in [{"1": row(2, 2)}, {}, {"1": row(0, 1)}]:
            with self.assertRaises(ValueError):
                meter.tree_delta({"1": row(1, 1)}, after, 1)
        with self.assertRaisesRegex(ValueError, "without_wait_credit"):
            meter.tree_delta({"1": row(1, 1), "2": row(10, 2)},
                             {"1": row(2, 1), "3": row(20, 3)}, 1)

    def test_launch_failure_retains_a_failed_receipt(self):
        with tempfile.TemporaryDirectory(prefix="whoate-meter-") as directory:
            output = Path(directory) / "failure.json"
            result = subprocess.run([sys.executable, "-B", str(Path(meter.__file__)), "--binary", "/nonexistent/whoate",
                                     "--label", "bad-binary", "--out", str(output)], capture_output=True, timeout=5)
            self.assertNotEqual(result.returncode, 0)
            receipt = json.loads(output.read_text())
            self.assertFalse(receipt["pass"])
            self.assertIn("error", receipt)

if __name__ == "__main__":
    unittest.main()
