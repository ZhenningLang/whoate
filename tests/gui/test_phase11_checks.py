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
