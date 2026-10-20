"""QA14 launcher and static scan, QA15 residue parser. No GUI process starts here."""
import io, os, plistlib, re, signal, stat, sys, tempfile, unittest
from pathlib import Path
import launch, ls_residue

HERE = Path(__file__).resolve().parent

# Recorded lsappinfo shapes (2026-10-20): residue, related tmux, coalition tmux, clean.
SAMPLE = """\
81) "whoate" ASN:0x0-0x1412411:
    bundleID="io.github.zhenninglang.whoate"
    bundle path="/Users/x/dist/whoate.app"
    executable path="/Users/x/dist/whoate.app/Contents/MacOS/Whoate"
    pid = 64634 token=[sess=100017 pid=64634 uid:501,501,501 g:20,20 pV:13715574] type="UIElement" flavor=3 Version="0.1.0" fileType="APPL" creator="????" Arch=ARM64 (exited-with-subordinates)
    coalition: 124656    relatedCoalitionIDs: ( 3801)  relatedCoalitionPIDs: ( 48961, 48968)
82) "Whoate" ASN:0x0-0x1500001:
    bundleID=[ NULL ]
    bundle path="/Users/x/app/.build/release/Whoate"
    executable path="/Users/x/app/.build/release/Whoate"
    pid = 7001 token=[sess=100017 pid=7001 uid:501,501,501 g:20,20 pV:1] type="Foreground" flavor=3 Version=[ NULL ]  fileType="????" creator="????" Arch=ARM64
    coalition: 3946    relatedCoalitionIDs: ( 3801)  relatedCoalitionPIDs: ( 13921, 48968)
83) "WhoateChecks" ASN:0x0-0x1600001:
    bundleID=[ NULL ]
    bundle path="/Users/x/app/.build/debug/WhoateChecks"
    executable path="/Users/x/app/.build/debug/WhoateChecks"
    pid = 7002 token=[sess=100017 pid=7002 uid:501,501,501 g:20,20 pV:2] !signalled type="BackgroundOnly" flavor=3 Version=[ NULL ]  fileType="????" creator="????" Arch=ARM64
    coalition: 3946  { 1476 13921 7002 }
84) "whoate" ASN:0x0-0x1700001:
    bundleID="io.github.zhenninglang.whoate"
    bundle path="/Users/x/dist/whoate.app"
    executable path="/Users/x/dist/whoate.app/Contents/MacOS/Whoate"
    pid = 7003 token=[sess=100017 pid=7003 uid:501,501,501 g:20,20 pV:3] type="UIElement" flavor=3 Version="0.1.0" fileType="APPL" creator="????" Arch=ARM64
    coalition: 169201  { 7003 7010 }    relatedCoalitionIDs: ( 3801)  relatedCoalitionPIDs: ( 48961, 48968)
    childASNs: ASN:0x0-0x38038:
85) "Finder" ASN:0x0-0x1800001:
    bundleID="com.apple.finder"
    bundle path="/System/Library/CoreServices/Finder.app"
    executable path="/System/Library/CoreServices/Finder.app/Contents/MacOS/Finder"
    pid = 500 token=[sess=100017 pid=500 uid:501,501,501 g:20,20 pV:4] type="Foreground" flavor=3 Version="1" fileType="FNDR" creator="MACS" Arch=ARM64 (exited-with-subordinates)
    coalition: 3946  { 13921 }
"""
TMUX = [13921, 32509]


class ResidueTests(unittest.TestCase):
    def entries(self):
        return {e["pid"]: e for e in ls_residue.parse(SAMPLE)}

    def test_parser_reads_flags_coalition_members_and_related_pids(self):
        e = self.entries()
        self.assertEqual(len(e), 5, "childASNs/parentASN lines are not entries")
        self.assertEqual(e[64634]["flags"], ["exited-with-subordinates"])
        self.assertEqual(e[64634]["related_pids"], [48961, 48968])
        self.assertEqual(e[7002]["members"], [1476, 13921, 7002])
        self.assertEqual(e[7003]["type"], "UIElement")
        self.assertEqual(e[7002]["flags"], [], "!signalled is not a residue flag")

    def test_each_failure_mode_is_flagged_and_a_clean_entry_is_not(self):
        e = self.entries()
        self.assertEqual(ls_residue.problems(e[64634], TMUX), ["exited-with-subordinates"])
        self.assertEqual(ls_residue.problems(e[7001], TMUX), ["related_contains_tmux=[13921]"])
        self.assertEqual(ls_residue.problems(e[7002], TMUX), ["coalition_contains_tmux=[13921]"])
        self.assertEqual(ls_residue.problems(e[7003], TMUX), [])

    def test_only_new_whoate_entries_fail(self):
        entries = ls_residue.parse(SAMPLE)
        failures, pre, exempt = ls_residue.classify(entries, {"ASN:0x0-0x1412411"}, TMUX, {7002})
        self.assertEqual([x["pid"] for x, _ in failures], [7001])
        self.assertEqual([x["pid"] for x, _ in pre], [64634], "baseline residue is reported, not failed")
        self.assertEqual([x["pid"] for x, _ in exempt], [7002])
        self.assertNotIn(500, [x["pid"] for x, _ in failures + pre + exempt], "non-whoate entries are ignored")
        failures, _, _ = ls_residue.classify(entries, set(), TMUX, set())
        self.assertEqual(sorted(x["pid"] for x, _ in failures), [7001, 7002, 64634],
                         "a new residue is never passed as pre-existing")

    def test_check_waits_for_new_residue_then_prints_raw_entries(self):
        reads, lines = iter([SAMPLE, SAMPLE.split("84)")[0]]), []
        failed = ls_residue.check({"asns": ["ASN:0x0-0x1412411"], "tmux_pids": TMUX}, [7002], wait_s=1,
                                  read=lambda: next(reads), sleep=lambda _: None, out=lines.append)
        text = "\n".join(lines)
        self.assertEqual(failed, 1)
        self.assertIn("PREEXISTING_LS_RESIDUE pid=64634", text)
        self.assertIn("EXEMPT_LS_RESIDUE pid=7002", text)
        self.assertIn('LS_RESIDUE_FAIL pid=7001', text)
        self.assertIn('82) "Whoate" ASN:0x0-0x1500001:', text, "the raw entry is printed")

    def test_tmux_servers_come_from_env_and_ppid_one(self):
        table = "  13921     1 tmux\n  35372 35361 tmux\n  32509     1 /opt/homebrew/bin/tmux\n  1 0 launchd\n"
        self.assertEqual(ls_residue.tmux_servers({"TMUX": "/private/tmp/tmux-501/default,999,0"}, table),
                         [999, 13921, 32509])
        self.assertEqual(ls_residue.tmux_servers({}, table), [13921, 32509])


class RealSampleTests(unittest.TestCase):
    """Owner ruling 2026-10-20 16:14: the post-exit residue is proven on real
    lsappinfo text from this Mac (lsappinfo-real-20261020.txt), not by creating
    a dangerous residue under tmux."""
    SAMPLE = HERE / "lsappinfo-real-20261020.txt"
    TMUX_AT_CAPTURE = [13921, 32509]

    def test_sample_is_anonymised(self):
        text = self.SAMPLE.read_text()
        self.assertNotRegex(text, r"/Users/[^<]", "user paths are ~")
        self.assertIn('bundle path="~/Projects/whoate-docs-design', text)

    def test_real_residue_entries_fail_and_are_printed(self):
        entries = {e["pid"]: e for e in ls_residue.parse(self.SAMPLE.read_text())}
        self.assertEqual(sorted(entries), [19641, 35264, 64634])
        self.assertEqual(ls_residue.problems(entries[64634], self.TMUX_AT_CAPTURE), ["exited-with-subordinates"])
        self.assertEqual(entries[64634]["related_ids"], [3801], "the pre-incident tmux coalition")
        self.assertEqual(ls_residue.problems(entries[19641], self.TMUX_AT_CAPTURE), ["exited-with-subordinates"])
        self.assertEqual(ls_residue.problems(entries[35264], self.TMUX_AT_CAPTURE), ["related_contains_tmux=[13921]"])
        lines = []
        failed = ls_residue.check({"asns": [], "tmux_pids": self.TMUX_AT_CAPTURE}, wait_s=0,
                                  read=self.SAMPLE.read_text, sleep=lambda _: None, out=lines.append)
        text = "\n".join(lines)
        self.assertEqual(failed, 2, "both whoate residues fail; Ghostty is not a whoate entry")
        self.assertIn("LS_RESIDUE_FAIL pid=64634 issues=['exited-with-subordinates']", text)
        self.assertIn("LS_RESIDUE_FAIL pid=19641 issues=['exited-with-subordinates']", text)
        self.assertIn('81) "whoate" ASN:0x0-0x1412411:', text, "the raw entry is printed")
        self.assertNotIn("pid=35264", text)


class FakeOpener:
    def __init__(self, rc=0, stderr=b""):
        self.returncode, self.stderr = rc, io.BytesIO(stderr)

    def poll(self):
        return self.returncode


class LauncherTests(unittest.TestCase):
    def setUp(self):
        self.dir = tempfile.TemporaryDirectory(prefix="whoate-launch-test-")
        self.bare = Path(self.dir.name) / "WhoateChecks"
        self.bare.write_bytes(b"\xcf\xfa\xed\xfe fake")
        self.bare.chmod(self.bare.stat().st_mode | stat.S_IXUSR)

    def tearDown(self):
        self.dir.cleanup()

    def test_bare_executable_is_wrapped_as_an_lsuielement_bundle(self):
        bundle, executable = launch.bundle_for(self.bare, self.dir.name + "/w")
        info = plistlib.loads((bundle / "Contents/Info.plist").read_bytes())
        self.assertTrue(info["LSUIElement"])
        self.assertEqual(info["CFBundleExecutable"], "WhoateChecks")
        self.assertEqual(executable.read_bytes(), self.bare.read_bytes())
        self.assertEqual(launch.bundle_for(bundle, "/unused"), (bundle, executable), "a bundle is used as is")
        self.assertEqual(launch.bundle_for(executable, "/unused")[0], bundle, "an executable inside a bundle")
        with self.assertRaises(launch.LaunchError):
            launch.bundle_for(Path(self.dir.name) / "missing", self.dir.name)

    def test_variables_go_through_env_flags_and_open_never_forwards_tmux(self):
        command = launch.open_command("/b.app", {"WHOATE_X": "1", "LC_ALL": "en_US.UTF-8"}, ["a"], "/o", "/e", True)
        self.assertEqual(command[:4], ["/usr/bin/open", "-n", "-g", "-W"])
        self.assertIn("--env", command)
        self.assertIn("WHOATE_X=1", command)
        self.assertEqual(command[-2:], ["--args", "a"])
        base = launch.launchd_env({"TMUX": "/t,1,0", "TMUX_PANE": "%1", "HOME": "/h", "SECRET": "x"})
        self.assertEqual(base, {"HOME": "/h", "PATH": "/usr/bin:/bin:/usr/sbin:/sbin"})

    def launcher(self, rows, lookup=None, signals=None):
        calls, listed = [], []
        clock = iter(range(0, 10_000))
        def lister(_):
            # Nothing before the launch, then `rows`.
            listed.append(1)
            return rows if len(listed) > 1 else []
        return launch.Launcher(
            runner=lambda command, **kw: calls.append((command, kw)) or FakeOpener(),
            lister=lister, lookup=lookup or (lambda pid: None),
            signaller=lambda pid, sig: signals.append((pid, sig)) if signals is not None else None,
            clock=lambda: next(clock) * 0.01 + 1000, sleep=lambda _: None), calls

    def test_ambiguous_pid_fails(self):
        rows = [dict(pid=p, ppid=1, start=2_000_000_000, path="/x") for p in (10, 11)]
        runner, calls = self.launcher(rows)
        with self.assertRaisesRegex(launch.LaunchError, "pid_ambiguous"):
            runner.start(self.bare)
        self.assertNotIn("TMUX", calls[0][1]["env"])

    def test_an_app_that_exits_before_its_pid_is_seen_still_returns_its_output(self):
        out = Path(self.dir.name) / "stdout"
        out.write_text("{}\n")
        runner = launch.Launcher(runner=lambda command, **kw: FakeOpener(0), lister=lambda _: [],
                                 lookup=lambda pid: self.fail("nothing to look up"),
                                 signaller=lambda pid, sig: self.fail("nothing to signal"), sleep=lambda _: None)
        handle = runner.start(self.bare, stdout=str(out), wait=True)
        self.assertIsNone(handle["pid"])
        self.assertTrue(runner.wait(handle, 1))
        self.assertEqual(runner.stop(handle), "gone_or_reused")
        with self.assertRaisesRegex(launch.LaunchError, "open_failed"):
            launch.Launcher(runner=lambda command, **kw: FakeOpener(1, b"LSOpenURLsWithRole() failed"),
                            lister=lambda _: [], sleep=lambda _: None).start(self.bare, wait=True)
        # open -W's race: the app quit before open could watch it; the output is still there.
        raced = launch.Launcher(runner=lambda command, **kw: FakeOpener(
            1, b"Unable to block on applications (initial call to kevent() failed: No such process)"),
            lister=lambda _: [], sleep=lambda _: None).start(self.bare, stdout=str(out), wait=True)
        self.assertIsNone(raced["pid"])
        self.assertIn("kevent", raced["open_note"])

    def test_stop_signals_only_the_same_identity(self):
        handle = {"pid": 42, "start": 5, "path": "/w/Whoate"}
        signals = []
        reused, _ = self.launcher([], lookup=lambda pid: {"pid": 42, "start": 6, "path": "/w/Whoate"}, signals=signals)
        self.assertEqual(reused.stop(handle), "gone_or_reused")
        self.assertEqual(signals, [], "a reused pid is never signalled")
        state = {"alive": True}
        def lookup(pid):
            return dict(handle) if state["alive"] else None
        def signal_it(pid, sig):
            signals.append((pid, sig))
            state["alive"] = False
        owned = launch.Launcher(lookup=lookup, signaller=signal_it, sleep=lambda _: None)
        self.assertEqual(owned.stop(handle), "terminated")
        self.assertEqual(signals, [(42, signal.SIGTERM)])


class StaticScanTests(unittest.TestCase):
    """QA14: every GUI launch goes through launch.py; nothing force quits."""
    BARE = re.compile(r"\b(Popen|subprocess\.run|exec|execve|env -i)\b[^\n]*(Whoate|WhoateChecks|\$binary|"
                      r"args\.binary|\[binary\])")
    FORBIDDEN = re.compile(r"killall\s+Dock|forceTerminate|lsappinfo\s+(kill|forcequit)|libquit|"
                           r"kill\s+-9\s+\$\(|pkill|killall")

    def sources(self):
        extra = [Path(p) for p in os.environ.get("WHOATE_SCAN_EXTRA", "").split(":") if p]
        return sorted([*HERE.glob("*.py"), *HERE.glob("*.sh"), *HERE.glob("*.c")] + extra)

    def test_no_bare_gui_exec_outside_the_negative_control(self):
        for path in self.sources():
            if path.name in ("test_launch_residue.py", "ls_negative_control.py", "ls_control_wrapper.c"):
                continue
            hits = [line for line in path.read_text().splitlines() if self.BARE.search(line)]
            self.assertEqual(hits, [], f"{path.name} starts a GUI binary without launch.py")
        control = (HERE / "ls_negative_control.py").read_text()
        self.assertIn("NEGATIVE CONTROL", control, "the one exception says so")

    def test_no_force_quit_or_foreign_signals(self):
        for path in self.sources():
            if path.name == "test_launch_residue.py":
                continue
            self.assertIsNone(self.FORBIDDEN.search(path.read_text()), path.name)
        self.assertTrue(any("screenshots.py" == p.name for p in self.sources()))

    # A call: the binary path, a quoted argv entry, or a shell command with flags.
    CAPTURE = re.compile(r"/usr/sbin/screencapture|[\"']screencapture[\"']|\bscreencapture\s+-")

    def test_every_screencapture_names_one_window(self):
        # QA34: only whoate's own window is captured, never the whole screen.
        found = 0
        for path in self.sources():
            if path.name == "test_launch_residue.py":
                continue
            for line in path.read_text().splitlines():
                if self.CAPTURE.search(line):
                    found += 1
                    self.assertRegex(line, r"-l[\w{$<]", f"{path.name}: {line.strip()}")
        self.assertGreater(found, 0)
        self.assertIsNotNone(self.CAPTURE.search("screencapture -x shot.png"), "a whole-screen call is seen")


if __name__ == "__main__":
    unittest.main()
