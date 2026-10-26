"""Phase 11 checker gates (QA22 window ownership and home paths, QA23 login items).
Nothing here starts an app, captures the screen or reads the owner's databases."""
import os, plistlib, tempfile, unittest
from pathlib import Path
import phase11_checks as checks


class CaptureOwnershipTests(unittest.TestCase):
    def capture(self, owner):
        taken = []
        found = None if owner is None else (owner, 100, 50)
        result = checks.capture("4242", "100x50x2.0", 777, "/nonexistent/shot.png", owner=lambda n: found,
                                take=lambda n, png: taken.append(n) or (True, ""),
                                measure=lambda png: {"width": 200, "height": 100, "distinct_colours": 99, "mean_luma": 1})
        return result, taken

    def test_a_window_of_another_process_fails_and_is_never_captured(self):
        result, taken = self.capture(owner=12345)
        self.assertFalse(result["ok"])
        self.assertFalse(result["owner_ok"])
        self.assertEqual(taken, [])

    def test_a_missing_window_fails(self):
        result, taken = self.capture(owner=None)
        self.assertFalse(result["ok"])
        self.assertEqual(result["error"], "window missing")
        self.assertEqual(taken, [])

    def test_the_launched_process_window_passes(self):
        result, taken = self.capture(owner=777)
        self.assertTrue(result["ok"])
        self.assertTrue(result["owner_ok"])
        self.assertEqual(taken, ["4242"])

    def test_the_window_server_knows_no_such_window(self):
        self.assertIsNone(checks.window_owner(2_000_000_000))


class HeldSizeTests(unittest.TestCase):
    """QA22: the window server's size must hold before a capture and after it."""

    def reads(self, *heights):
        found = iter([(777, 100, h) for h in heights])
        return lambda n: next(found, None)

    def test_the_size_once_five_reads_agree(self):
        self.assertEqual(checks.held("1", owner=self.reads(301, 301, 290, 290, 290, 290, 290), sleep=lambda s: None),
                         (100, 290))

    def test_a_size_that_never_holds_is_none(self):
        self.assertIsNone(checks.held("1", owner=self.reads(*[301, 290] * 40), sleep=lambda s: None))

    def test_a_window_that_grows_during_the_capture_fails(self):
        heights = iter([50] * 6 + [61])
        result = checks.capture("4242", "100x50x2.0", 777, "/nonexistent/shot.png", owner=lambda n: (777, 100, next(heights)),
                                take=lambda n, png: (True, ""), sleep=lambda s: None,
                                measure=lambda png: {"width": 200, "height": 100, "distinct_colours": 99, "mean_luma": 1})
        self.assertEqual(result["expected_size"], (200, 100))
        self.assertFalse(result["held_after"])
        self.assertFalse(result["ok"])


def timeline(changes, end):
    """(time, tab, height) every 30 ms from the change points a probe printed."""
    out = []
    for (t, tab, h), nxt in zip(changes, [c[0] for c in changes[1:]] + [end]):
        while t < nxt:
            out.append((t, tab, h))
            t += 0.03
    return out


class SettleTests(unittest.TestCase):
    """QA22 settle, on the window heights measured with the debug app (en, light, 2026-10-27)."""
    # Before settingsPageResized: notifications at 301 pt, then 290 pt 0.62 s later.
    BEFORE = [(4.72, "general", 286), (4.76, "general", 288), (4.79, "general", 290), (9.75, "general", 253),
              (9.91, "menubar", 253), (12.16, "menubar", 301), (12.20, "notifications", 301),
              (12.82, "notifications", 290), (14.51, "cli", 290), (14.60, "cli", 262), (17.04, "about", 350)]
    # Before, another tab order: notifications stayed at 301 pt for the whole dwell.
    BEFORE_STUCK = [(2.92, "general", 290), (7.85, "notifications", 301), (10.20, "notifications", 253),
                    (10.37, "menubar", 253), (12.73, "menubar", 301), (12.89, "notifications", 301),
                    (15.17, "cli", 262), (17.5, "about", 350)]
    AFTER = [(4.78, "general", 288), (4.81, "general", 290), (9.43, "menubar", 290), (9.50, "menubar", 253),
             (11.98, "menubar", 290), (12.17, "notifications", 290), (14.45, "cli", 290), (14.49, "cli", 262),
             (16.95, "about", 262), (16.98, "about", 350)]

    def test_before_the_fix_notifications_drops_during_its_dwell(self):
        dwell = checks.dwell_heights(timeline(self.BEFORE, 19.5))
        self.assertEqual(dwell["notifications"], [290, 301])
        self.assertFalse(checks.settled_verdict(dwell, 290))

    def test_before_the_fix_a_page_left_too_tall_differs_from_opening_on_it(self):
        dwell = checks.dwell_heights(timeline(self.BEFORE_STUCK, 19.5))
        self.assertEqual(dwell["notifications"], [301])
        self.assertFalse(checks.settled_verdict(dwell, 290))

    def test_after_the_fix_every_tab_holds_one_height(self):
        dwell = checks.dwell_heights(timeline(self.AFTER, 19.5))
        self.assertEqual(dwell, {"general": [290], "menubar": [253], "notifications": [290], "cli": [262], "about": [350]})
        self.assertTrue(checks.settled_verdict(dwell, 290))

    def test_a_tab_never_seen_fails(self):
        self.assertFalse(checks.settled_verdict({"notifications": [290]}, 290))


class HomeTests(unittest.TestCase):
    def test_any_value_under_the_home_folder_fails(self):
        self.assertTrue(checks.home_free({"HOME": "/private/tmp/x/home", "A": "1"}, home="/Users/someone"))
        self.assertFalse(checks.home_free({"WHOATE_UI_CLI_BUNDLED": "/Users/someone/p/target/debug/whoate"},
                                          home="/Users/someone"))


class LoginItemRecordTests(unittest.TestCase):
    def archive(self, directory, name, records):
        objects = ["$null"]
        for identifier, disposition in records:
            objects.append(identifier)
            objects.append({"identifier": plistlib.UID(len(objects) - 1), "disposition": disposition,
                            "url": plistlib.UID(0)})
        Path(directory, name).write_bytes(plistlib.dumps({"$objects": objects}, fmt=plistlib.FMT_BINARY))

    def test_only_whoate_records_with_their_disposition(self):
        with tempfile.TemporaryDirectory() as d:
            self.archive(d, "BackgroundItems-v18-A.btm", [("2.io.github.zhenninglang.whoate", 2), ("2.com.other.app", 3)])
            Path(d, "BackgroundItems-v18-B.btm").write_bytes(b"not a plist")
            read, entries = checks.btm_entries(d)
            self.assertEqual(read, 1)
            self.assertEqual(entries, [["BackgroundItems-v18-A.btm", "2.io.github.zhenninglang.whoate", 2, "$null"]])

    def test_a_changed_disposition_is_a_different_snapshot(self):
        with tempfile.TemporaryDirectory() as d:
            self.archive(d, "BackgroundItems-v18-A.btm", [("2.io.github.zhenninglang.whoate", 2)])
            before = checks.btm_entries(d)
            self.archive(d, "BackgroundItems-v18-A.btm", [("2.io.github.zhenninglang.whoate", 10)])
            self.assertNotEqual(before, checks.btm_entries(d))


if __name__ == "__main__":
    unittest.main()
