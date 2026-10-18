"""Full-tier native calibration. No skipped tests and no product CPU trials."""
import json
from pathlib import Path
import subprocess
import sys
import unittest
import cpu_sample as meter


class NativeMeterTests(unittest.TestCase):
    def test_live_reaped_and_grandchild_match_independent_process_time(self):
        for kind in ["live", "reaped", "grandchild"]:
            with self.subTest(kind=kind):
                child = subprocess.Popen([sys.executable, str(Path(__file__).with_name("cpu_load.py")), kind],
                                         stdin=subprocess.PIPE, stdout=subprocess.PIPE, text=True)
                try:
                    self.assertEqual(child.stdout.readline().strip(), "ready")
                    before = meter.tree_sample(child.pid)
                    child.stdin.write("start\n")
                    child.stdin.flush()
                    reference = json.loads(child.stdout.readline())["reference_ns"]
                    after = meter.tree_sample(child.pid)
                    own, descendants = meter.tree_delta(before, after, child.pid)
                    ratio = descendants / reference
                    print(f"CPU_METER_NATIVE kind={kind} own_ns={own} child_ns={descendants} reference_ns={reference} ratio={ratio}")
                    self.assertGreater(descendants, 0)
                    self.assertTrue(0.5 <= ratio <= 2)
                    self.assertFalse(meter.evaluate([0] + [0.01] * 60, 61, descendants / 1e9 * 100, 1)["pass"])
                finally:
                    if child.poll() is None:
                        child.communicate("quit\n", timeout=10)


if __name__ == "__main__":
    unittest.main(verbosity=2)
