import importlib.util
import json
import pathlib
import tempfile
import unittest

spec = importlib.util.spec_from_file_location("phase07_acceptance", pathlib.Path(__file__).with_name("phase07_acceptance.py"))
driver = importlib.util.module_from_spec(spec)
spec.loader.exec_module(driver)


class OracleTests(unittest.TestCase):
    def test_zprint_oracle_uses_current_inuse_and_preserves_all_candidates(self):
        root = pathlib.Path(__file__).resolve().parents[2]
        rows = driver.native_zones((root / "tests/fixtures/system-insights/zprint.txt").read_text())
        self.assertEqual(rows[0], {"name": "data.kalloc.1024", "bytes": 1729536})
        self.assertEqual(len(rows), 7)
        self.assertEqual(rows[-1], {"name": "beta", "bytes": 200})
        for bad in ["", "garbage\n", "elem\ninuse\n---\nrow x\n"]:
            with self.assertRaises(RuntimeError):
                driver.native_zones(bad)

    def test_dynamic_envelope_rejects_value_and_ranking_outside_external_samples(self):
        before = [{"name": str(i), "bytes": 10_000_000 - i * 1_000_000} for i in range(7)]
        after = [{"name": str(i), "bytes": 10_000_001 - i * 1_000_000} for i in range(7)]
        product = [dict(row) for row in before[:5]]
        driver.check_zone_envelope(product, before, after)
        bad = [dict(row) for row in product]
        bad[0]["bytes"] = 20_000_000
        with self.assertRaisesRegex(RuntimeError, "OUTSIDE_EXTERNAL_ENVELOPE"):
            driver.check_zone_envelope(bad, before, after)
        bad = [dict(row) for row in product]
        bad[-1] = dict(before[-1])
        with self.assertRaisesRegex(RuntimeError, "RANK_MISMATCH"):
            driver.check_zone_envelope(bad, before, after)

    def test_approved_zone_tolerance_and_fifth_place_exchange_have_exact_limits(self):
        before = [{"name": str(i), "bytes": 10_000_000 - i * 1_000_000} for i in range(5)]
        before.append({"name": "alternate", "bytes": 6_000_000 - 65_536})
        product = [dict(row) for row in before[:4]] + [dict(before[5])]
        driver.check_zone_envelope(product, before, before)
        product = [dict(row) for row in before[:5]]
        product[0]["bytes"] += 65_536
        driver.check_zone_envelope(product, before, before)
        product[0]["bytes"] += 1
        with self.assertRaisesRegex(RuntimeError, "OUTSIDE_EXTERNAL_ENVELOPE"):
            driver.check_zone_envelope(product, before, before)
        before[5]["bytes"] -= 1
        product = [dict(row) for row in before[:4]] + [dict(before[5])]
        with self.assertRaisesRegex(RuntimeError, "RANK_MISMATCH"):
            driver.check_zone_envelope(product, before, before)

    def test_unselected_truncated_zone_names_do_not_invalidate_unique_top_five(self):
        rows = [{"name": str(i), "bytes": 10_000_000 - i * 1_000_000} for i in range(5)]
        rows += [{"name": "truncated$", "bytes": 2}, {"name": "truncated$", "bytes": 1}]
        driver.check_zone_envelope(rows[:5], rows, rows)
        ambiguous = [dict(r) for r in rows] + [dict(rows[0])]
        with self.assertRaisesRegex(RuntimeError, "AMBIGUOUS_OR_MISSING"):
            driver.check_zone_envelope(rows[:5], ambiguous, rows)

    def test_wired_oracle_uses_actual_page_size_not_owner_or_zone_sum(self):
        self.assertEqual(driver.wired_bytes("Mach Virtual Memory Statistics: (page size of 16384 bytes)\nPages wired down:  123.\n"), 2015232)
        with self.assertRaises(RuntimeError):
            driver.wired_bytes("Pages wired down: unknown")

    def test_wired_and_zone_proportional_tolerance_reject_one_byte_beyond_limit(self):
        for kind, reference, tol in [("wired", 1_000_000_000, 268_435_456), ("wired", 10_000_000_000, 500_000_000), ("zone", 1_000_000_000, 1_000_000)]:
            for sign in [-1, 1]:
                driver.check_envelope(kind, "test", reference + sign * tol, reference, reference)
                with self.assertRaisesRegex(RuntimeError, "OUTSIDE_EXTERNAL_ENVELOPE"):
                    driver.check_envelope(kind, "test", reference + sign * (tol + 1), reference, reference)

    def test_pane_comparison_requires_actual_native_metadata_not_requested_input(self):
        actual="pane 雪 \\\\ text"
        owner={"origin":{"pane_title":actual,"window_name":"native"}}
        driver.check_pane_origins([owner],actual,"native")
        with self.assertRaisesRegex(RuntimeError,"TMUX_ORIGIN_MISMATCH"):
            driver.check_pane_origins([owner],"pane 雪 \\ text","native")
        with self.assertRaisesRegex(RuntimeError,"TMUX_PANE_OWNER_MISSING"):
            driver.check_pane_origins([],actual,"native")

    def test_build_binding_rejects_stale_source_or_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            files = ["Cargo.toml", "Cargo.lock", "crates/demo/src/lib.rs", "schema/whoate-v1.schema.json", "skills/whoate/SKILL.md", "tests/cli/test_phase07_acceptance.py"]
            for name in files:
                path = root / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(name)
            binary = root / "whoate"
            binary.write_bytes(b"binary")
            binding = root / "binding.json"
            driver.bind(root, binary, binding)
            driver.check_binding(root, binary, binding)
            for name in [files[2], files[-1]]:
                (root / name).write_text("changed source")
                with self.assertRaisesRegex(RuntimeError, "SOURCE_BINDING_MISMATCH"):
                    driver.check_binding(root, binary, binding)
                (root / name).write_text(name)
            binary.write_bytes(b"old binary")
            with self.assertRaisesRegex(RuntimeError, "BINARY_BINDING_MISMATCH"):
                driver.check_binding(root, binary, binding)
            self.assertEqual(len(json.loads(binding.read_text())["inputs"]), 6)


if __name__ == "__main__":
    unittest.main()
