//! Tests call the exported C functions. Engines use isolated homes; kill
//! tests only signal helper processes they start themselves.
use super::*;
use serde_json::Value;
use std::{
    collections::VecDeque,
    ffi::{CStr, CString},
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::atomic::{AtomicU64, Ordering},
};
use whoate_core::{
    engine::{Context, SystemClock},
    recognizers::ConfigReader,
    sample::{MacSampler, SampleResult},
    snapshot::Proc,
    verdict::Language,
};

// Safe shadows of the C entry points: every pointer passed in these tests is
// NULL, freshly returned by the library, or a live slice.
fn whoate_engine_tick(e: *mut WhoateEngine) -> *mut c_char {
    unsafe { super::whoate_engine_tick(e) }
}
fn whoate_why(e: *mut WhoateEngine, pid: i32) -> *mut c_char {
    unsafe { super::whoate_why(e, pid) }
}
fn whoate_kill(pids: *const i32, n: i32, force: i32, err: *mut *mut c_char) -> i32 {
    unsafe { super::whoate_kill(pids, n, force, err) }
}
fn whoate_string_free(s: *mut c_char) {
    unsafe { super::whoate_string_free(s) }
}
fn whoate_engine_free(e: *mut WhoateEngine) {
    unsafe { super::whoate_engine_free(e) }
}
#[cfg(debug_assertions)]
fn fixture(path: *const c_char) -> *mut WhoateEngine {
    unsafe { replay::whoate_engine_new_from_fixture(path) }
}

/// Kill tests share the process-wide STATE; run them one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

struct Temp(PathBuf);
impl Temp {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let dir = std::env::temp_dir().join(format!(
            "whoate-ffi-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn take(s: *mut c_char) -> Value {
    assert!(!s.is_null(), "FFI returned NULL instead of JSON");
    let text = unsafe { CStr::from_ptr(s) }.to_str().unwrap().to_owned();
    whoate_string_free(s);
    serde_json::from_str(&text).unwrap()
}
/// Optional evidence for the Schema validator run by verify.sh.
fn evidence(name: &str, value: &Value) {
    if let Some(dir) = std::env::var_os("WHOATE_FFI_EVIDENCE_DIR") {
        std::fs::write(Path::new(&dir).join(name), value.to_string()).unwrap();
    }
}
fn context(home: &Path) -> Context {
    Context {
        cfg: ConfigReader::isolated(home.to_path_buf()),
        uid: unsafe { libc::getuid() },
        language: Language::En,
        platform_probe: kill::platform_binary,
    }
}
fn boxed(engine: WhoateEngine) -> *mut WhoateEngine {
    Box::into_raw(Box::new(engine))
}
fn capture_snapshot() -> Snapshot {
    serde_json::from_slice(
        &std::fs::read(root().join("tests/fixtures/capture/snapshot-20261014-1940.json")).unwrap(),
    )
    .unwrap()
}
struct Frames(VecDeque<Snapshot>, Snapshot);
impl Sampler for Frames {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().unwrap_or_else(|| self.1.clone()))
    }
}
fn replay(snapshot: Snapshot, home: &Path) -> *mut WhoateEngine {
    let mut ctx = context(home);
    ctx.platform_probe = |_| Ok(false);
    let engine = Engine::try_with_sampler(
        Frames(VecDeque::new(), snapshot),
        SystemClock::default(),
        ctx,
    )
    .unwrap();
    boxed(WhoateEngine::live(engine, false))
}

#[test]
fn phase09_host_uses_only_injected_host_and_preserves_full_snapshot() {
    let _serial = SERIAL.lock().unwrap();
    let mut snapshot = capture_snapshot();
    snapshot.host = Some(
        serde_json::from_value(json!({"mem_total_bytes":16384,"mem_used_bytes":8192,
        "compressed_bytes":1024,"wired_bytes":2048,"swap_used_bytes":4096,"pressure":"warn"}))
        .unwrap(),
    );
    let previous = lock().snapshot.replace(snapshot.clone());
    let expected = serde_json::to_value(snapshot.host.clone().unwrap()).unwrap();
    let mut calls = 0;
    let value = super::host_json(|| {
        calls += 1;
        Ok(snapshot.host.clone().unwrap())
    });
    assert_eq!(calls, 1);
    assert_eq!(value["schema"], "whoate/host-v1");
    assert_eq!(value["host"], expected);
    assert!(value["taken_at"].is_string());
    assert!(value.get("owners").is_none());
    assert_eq!(lock().snapshot, Some(snapshot));
    let failure = super::host_json(|| Err(SampleError::new("host", "unavailable")));
    assert_eq!(failure["error"]["code"], 1);
    assert_eq!(failure["error"]["message"], "host: unavailable");
    lock().snapshot = previous;
}

#[test]
fn phase08_host_json_cpu_is_the_difference_since_the_previous_call() {
    let _serial = SERIAL.lock().unwrap();
    let first = take(super::whoate_host_json());
    let cpu = &first["host"]["cpu"];
    assert!(
        cpu["logical_cores"].as_u64().is_some_and(|n| n > 0),
        "{first}"
    );
    assert!(cpu["load_1m"].is_number());
    if cpu["status"] == "missing" {
        assert!(cpu["usage_pct"].is_null(), "missing is never 0%");
        assert!(cpu["reason"].is_string());
    }
    // Tick counters advance in bursts (measured: idle stretches up to ~1 s),
    // so a short gap is honestly missing; the next call that spans ticks is fresh.
    let mut second = Value::Null;
    for _ in 0..15 {
        std::thread::sleep(Duration::from_millis(200));
        second = take(super::whoate_host_json());
        if second["host"]["cpu"]["status"] == "fresh" {
            break;
        }
        assert_eq!(
            second["host"]["cpu"]["reason"], "no_elapsed_ticks",
            "{second}"
        );
        assert!(second["host"]["cpu"]["usage_pct"].is_null());
    }
    let cpu = &second["host"]["cpu"];
    assert_eq!(cpu["status"], "fresh", "{second}");
    let usage = cpu["usage_pct"].as_f64().unwrap();
    assert!((0.0..=100.0).contains(&usage));
    assert!(cpu["reason"].is_null());
}

#[test]
fn phase09_control_reaches_sampler_without_replacing_history_or_kill_identity() {
    let _serial = SERIAL.lock().unwrap();
    use std::sync::Arc;
    struct Observed {
        frame: Snapshot,
        activity: Arc<Mutex<Vec<bool>>>,
    }
    impl Sampler for Observed {
        fn sample(&mut self) -> SampleResult<Snapshot> {
            Ok(self.frame.clone())
        }
        fn set_active(&mut self, active: bool) {
            self.activity.lock().unwrap().push(active);
        }
    }
    let home = Temp::new();
    let activity = Arc::new(Mutex::new(vec![]));
    let engine = Engine::with_sampler(
        Observed {
            frame: capture_snapshot(),
            activity: activity.clone(),
        },
        SystemClock::default(),
        context(&home.0),
    );
    let engine = boxed(WhoateEngine::live(engine, false));
    let before = take(whoate_engine_tick(engine));
    let pid = before["owners"][0]["pids"][0].as_i64().unwrap() as i32;
    let why = take(whoate_why(engine, pid));
    for active in [0, 1] {
        let response = take(unsafe { super::whoate_engine_set_active(engine, active) });
        assert_eq!(response["schema"], "whoate/control-v1");
        assert_eq!(response["active"], active == 1);
        let host = take(super::whoate_host_json());
        assert_eq!(host["schema"], "whoate/host-v1");
        assert_eq!(take(whoate_why(engine, pid)), why);
    }
    assert_eq!(*activity.lock().unwrap(), vec![false, true]);
    assert_eq!(
        take(unsafe { super::whoate_engine_set_active(std::ptr::null_mut(), 0) })["error"]["code"],
        2
    );
    assert_eq!(
        take(unsafe { super::whoate_engine_set_active(engine, 2) })["error"]["code"],
        2
    );
    whoate_engine_free(engine);
}

#[test]
fn phase05_ffi_tick_and_why_contract() {
    let home = Temp::new();
    let engine = replay(capture_snapshot(), &home.0);
    let tick = take(whoate_engine_tick(engine));
    assert_eq!(tick["schema"], "whoate/v1");
    let owners = tick["owners"].as_array().unwrap();
    assert!(!owners.is_empty());
    evidence("replay-tick.json", &tick);
    // A pid shared by no other owner, and the full set of owners holding it.
    let pid = owners[0]["pids"][0].as_i64().unwrap();
    let holders: BTreeSet<_> = owners
        .iter()
        .filter(|o| {
            o["pids"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p.as_i64() == Some(pid))
        })
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    let why = take(whoate_why(engine, pid as i32));
    evidence("replay-why.json", &why);
    let got: BTreeSet<_> = why["owners"]
        .as_array()
        .unwrap()
        .iter()
        .map(|o| o["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(
        got, holders,
        "why returns exactly the owners containing pid"
    );
    for swarm in why["swarms"].as_array().unwrap() {
        assert!(
            swarm["owners"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| got.contains(r["id"].as_str().unwrap()))
        );
    }
    let missing = take(whoate_why(engine, i32::MAX));
    assert_eq!(missing["error"]["code"], 3);
    evidence("why-missing.json", &missing);
    whoate_engine_free(engine);

    let null = take(whoate_engine_tick(std::ptr::null_mut()));
    assert_eq!(null["error"]["code"], 2);
    assert_eq!(
        take(whoate_why(std::ptr::null_mut(), 1))["error"]["code"],
        2
    );
    // A panic becomes an error JSON instead of unwinding across the C boundary.
    let panicked = take(guarded(|| panic!("boom")));
    assert_eq!(panicked["error"], json!({"code": 1, "message": "panic"}));
}

#[test]
fn phase05_ffi_why_ticks_once_when_engine_never_ticked() {
    let home = Temp::new();
    let engine = replay(capture_snapshot(), &home.0);
    let pid = capture_snapshot()
        .procs
        .iter()
        .find(|p| p.footprint_bytes.is_some())
        .unwrap()
        .pid;
    let why = take(whoate_why(engine, pid));
    assert!(!why["owners"].as_array().unwrap().is_empty());
    whoate_engine_free(engine);
}

#[test]
fn phase05_ffi_insights_match_cli_registry() {
    let value = take(whoate_insights_json());
    assert_eq!(value["schema"], "whoate/v1");
    let entries = whoate_core::insights::registry::load().unwrap();
    assert_eq!(value["insights"], serde_json::to_value(&entries).unwrap());
    assert!(
        entries.iter().any(|e| e.system.is_some()),
        "system service policies included"
    );
}

#[test]
fn phase05_ffi_downsample_even_spacing_keeps_last() {
    let points: Vec<_> = (0..1800)
        .map(|i| HistoryPoint {
            timestamp_s: i,
            mem_bytes: i as u64,
            cpu_pct: None,
            unknown_mem_count: None,
        })
        .collect();
    let short = downsample(&points, 60);
    assert_eq!(short.len(), 60);
    assert_eq!(short.first(), points.first());
    assert_eq!(short.last(), points.last());
    let gaps: BTreeSet<_> = short
        .windows(2)
        .map(|w| w[1].timestamp_s - w[0].timestamp_s)
        .collect();
    assert!(
        gaps.iter().max().unwrap() - gaps.iter().min().unwrap() <= 1,
        "{gaps:?}"
    );
    assert_eq!(downsample(&points[..60], 60), points[..60].to_vec());
    assert_eq!(downsample(&points[..3], 60), points[..3].to_vec());
    assert!(downsample(&[], 60).is_empty());
}

#[test]
fn phase05_ffi_tick_history_capped_and_why_full_series() {
    let home = Temp::new();
    let uid = unsafe { libc::getuid() };
    let frame = |second: u32| {
        let procs = (0..200_i32)
            .map(|i| {
                let mut p =
                    Proc::identity(960_000 + i, 1, uid, Some(1_000_000), format!("worker{i}"));
                p.footprint_bytes = Some(1_048_576 * (i as u64 + 1) + u64::from(second));
                p
            })
            .collect();
        let (m, s) = (second / 60, second % 60);
        Snapshot::historical(format!("2026-10-14T19:{m:02}:{s:02}+08:00"), procs)
    };
    let frames: VecDeque<_> = (0..1802).map(frame).collect();
    let last = frames.back().unwrap().clone();
    let engine = Engine::try_with_sampler(
        Frames(frames, last),
        SystemClock::default(),
        context(&home.0),
    )
    .unwrap();
    let engine = boxed(WhoateEngine::live(engine, false));
    // The first tick consumes two frames (two-sample CPU); 1800 more fill history.
    for _ in 0..1800 {
        whoate_string_free(whoate_engine_tick(engine));
    }
    let tick = take(whoate_engine_tick(engine));
    let owners = tick["owners"].as_array().unwrap();
    assert_eq!(owners.len(), 200);
    let last = unsafe { &*engine }.last.as_ref().unwrap();
    for owner in owners {
        let history = owner["history"].as_array().unwrap();
        assert!(history.len() <= TICK_HISTORY_POINTS, "{}", history.len());
        let id = owner["id"].as_str().unwrap();
        let full = &last.owners.iter().find(|o| o.id == id).unwrap().history;
        assert_eq!(full.len(), 1800);
        assert_eq!(
            history.last().unwrap(),
            &serde_json::to_value(full.last().unwrap()).unwrap()
        );
    }
    let pid = owners[0]["pids"][0].as_i64().unwrap() as i32;
    let id = owners[0]["id"].as_str().unwrap();
    let why = take(whoate_why(engine, pid));
    let series = unsafe { &*engine }.series(id);
    assert_eq!(series.len(), 1800);
    assert_eq!(
        why["owners"][0]["history"],
        serde_json::to_value(&series).unwrap()
    );
    whoate_engine_free(engine);
}

#[cfg(debug_assertions)]
#[test]
fn phase05_ffi_fixture_replay_report_and_snapshot() {
    let open = |path: &Path| {
        let c = CString::new(path.to_str().unwrap()).unwrap();
        fixture(c.as_ptr())
    };
    let sample = root().join("tests/fixtures/attribution/sample-v1.json");
    let before = std::fs::read(&sample).unwrap();
    let report = open(&sample);
    assert!(!report.is_null());
    let tick = take(whoate_engine_tick(report));
    let original: Value = serde_json::from_slice(&before).unwrap();
    assert_eq!(tick["owners"], original["owners"]);
    assert_eq!(
        take(whoate_engine_tick(report))["owners"],
        original["owners"]
    );
    whoate_engine_free(report);
    assert_eq!(
        std::fs::read(&sample).unwrap(),
        before,
        "fixture bytes untouched"
    );

    let snapshot = open(&root().join("tests/fixtures/capture/snapshot-20261014-1940.json"));
    assert!(!snapshot.is_null());
    let tick = take(whoate_engine_tick(snapshot));
    assert!(!tick["owners"].as_array().unwrap().is_empty());
    whoate_engine_free(snapshot);

    let home = Temp::new();
    let bad = home.0.join("bad.json");
    std::fs::write(&bad, "{not json").unwrap();
    let neither = home.0.join("neither.json");
    std::fs::write(&neither, r#"{"foo":1}"#).unwrap();
    assert!(open(&home.0.join("absent.json")).is_null());
    assert!(open(&bad).is_null());
    assert!(open(&neither).is_null());
    assert!(fixture(std::ptr::null()).is_null());
}

#[cfg(debug_assertions)]
#[test]
fn phase05_ffi_replayed_identities_never_authorize_kill() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    lock().snapshot = None;
    let path = CString::new(
        root()
            .join("tests/fixtures/capture/snapshot-20261014-1940.json")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    let engine = fixture(path.as_ptr());
    whoate_string_free(whoate_engine_tick(engine));
    assert!(lock().snapshot.is_none());
    whoate_engine_free(engine);
}

struct Helper(Child);
impl Helper {
    fn start(binary: &Path, mode: Option<&str>) -> Self {
        use std::io::BufRead;
        let mut child = Command::new(binary)
            .args(mode)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut line = String::new();
        std::io::BufReader::new(child.stdout.as_mut().unwrap())
            .read_line(&mut line)
            .unwrap();
        assert_eq!(
            line.trim().parse::<u32>().unwrap(),
            child.id(),
            "helper ready"
        );
        Self(child)
    }
    fn pid(&self) -> i32 {
        self.0.id() as i32
    }
    /// Reaps the helper if it exited; true while it is still running.
    /// An error means the Reaper already collected it.
    fn alive(&mut self) -> bool {
        matches!(self.0.try_wait(), Ok(None))
    }
}
impl Drop for Helper {
    fn drop(&mut self) {
        // Only signal a child that is provably still ours (never a reused pid).
        if self.alive() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}
/// Reaps exited helpers in the background so a zombie never looks alive.
struct Reaper(MacBackend, Vec<i32>);
impl Backend for Reaper {
    fn self_pid(&self) -> i32 {
        self.0.self_pid()
    }
    fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
        if self.1.contains(&pid) {
            let mut status = 0;
            unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        }
        self.0.identity(pid)
    }
    fn platform_binary(&mut self, pid: i32) -> Result<bool, &'static str> {
        self.0.platform_binary(pid)
    }
    fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str> {
        self.0.signal(pid, signal)
    }
    fn wait(&mut self, duration: Duration) {
        self.0.wait(duration)
    }
    fn measure_memory(&mut self, plan: &KillPlan) -> kill::MemoryMeasurement {
        self.0.measure_memory(plan)
    }
}
fn status(value: &Value, pid: i32) -> (String, Option<String>) {
    let item = value["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["pid"] == pid)
        .unwrap_or_else(|| panic!("pid {pid} missing in {value}"));
    (
        item["status"].as_str().unwrap().to_owned(),
        item["reason"].as_str().map(str::to_owned),
    )
}
fn c_kill(pids: &[i32], force: i32) -> (i32, Value) {
    let mut err = std::ptr::null_mut();
    let code = whoate_kill(pids.as_ptr(), pids.len() as i32, force, &mut err);
    (code, take(err))
}

#[test]
fn phase05_ffi_kill_two_stage_contract() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let temp = Temp::new();
    let helper = temp.0.join("helper");
    let built = Command::new("/usr/bin/cc")
        .args(["-O2", "-o"])
        .arg(&helper)
        .arg(root().join("tests/cli/helper.c"))
        .status()
        .unwrap();
    assert!(built.success());
    let mut plain = Helper::start(&helper, None);
    let mut stubborn = Helper::start(&helper, Some("ignore-term"));
    let engine = Engine::try_with_sampler(
        MacSampler::with_cache_path(temp.0.join("cache/system.json")).unwrap(),
        SystemClock::default(),
        context(&temp.0),
    )
    .unwrap();
    let engine = boxed(WhoateEngine::live(engine, true));
    whoate_string_free(whoate_engine_tick(engine));
    let mut late = Helper::start(&helper, None);
    let reap = vec![plain.pid(), stubborn.pid(), late.pid()];

    // Never-confirmed pid with force=1: refused, no signal sent.
    lock().pending = None;
    let (code, value) = c_kill(&[late.pid()], 1);
    assert_eq!(code, 4);
    assert_eq!(
        status(&value, late.pid()),
        ("refused".into(), Some("no_confirmed_plan".into()))
    );
    assert!(late.alive());

    // Plain helper: SIGTERM, 5 s wait, terminated, memory measured both sides.
    let (code, value) = kill_json(&[plain.pid()], false, &mut Reaper(MacBackend, reap.clone()));
    evidence("kill-terminated.json", &value);
    assert_eq!(code, 0);
    assert_eq!(status(&value, plain.pid()).0, "terminated");
    assert!(value["memory"]["before"].is_object() && value["memory"]["after"].is_object());
    assert!(!plain.alive());

    // TERM-ignoring helper: still alive, then force=1 reuses the confirmed plan.
    let (code, value) = kill_json(
        &[stubborn.pid()],
        false,
        &mut Reaper(MacBackend, reap.clone()),
    );
    assert_eq!(code, 0);
    assert_eq!(status(&value, stubborn.pid()).0, "still_alive");
    assert!(stubborn.alive());
    let planned = lock().pending.as_ref().unwrap().0.pids.clone();
    let (code, value) = kill_json(
        &[stubborn.pid(), late.pid()],
        true,
        &mut Reaper(MacBackend, reap.clone()),
    );
    evidence("kill-forced.json", &value);
    assert_eq!(code, 4, "late pid refused");
    assert_eq!(status(&value, stubborn.pid()).0, "terminated");
    assert_eq!(
        status(&value, late.pid()).1.as_deref(),
        Some("no_confirmed_plan")
    );
    assert!(!stubborn.alive());
    assert!(late.alive());
    assert_eq!(planned[0].expected.as_ref().unwrap().pid, stubborn.pid());

    // Protected and unknown pids.
    let (code, value) = c_kill(&[1], 0);
    assert_eq!(code, 4);
    assert_eq!(status(&value, 1).0, "refused");
    let me = std::process::id() as i32;
    let (code, value) = c_kill(&[me], 0);
    assert_eq!(code, 4);
    assert_eq!(
        status(&value, me),
        ("refused".into(), Some("protected_pid".into()))
    );
    let (code, value) = c_kill(&[late.pid()], 0);
    evidence("kill-missing.json", &value);
    assert_eq!(code, 3, "started after the tick: not in the snapshot");
    assert_eq!(status(&value, late.pid()).0, "missing");
    assert!(late.alive());
    let (code, value) = c_kill(&[1, late.pid()], 0);
    assert_eq!(code, 4, "refusal wins over missing");
    assert_eq!(value["results"].as_array().unwrap().len(), 2);

    // NULL err pointer and invalid arguments.
    assert_eq!(whoate_kill([1].as_ptr(), 1, 0, std::ptr::null_mut()), 4);
    assert_eq!(whoate_kill(std::ptr::null(), 1, 0, std::ptr::null_mut()), 2);
    let (code, value) = c_kill(&[], 0);
    assert_eq!((code, value["error"]["code"].as_i64()), (2, Some(2)));
    whoate_engine_free(engine);
}

#[test]
fn phase05_ffi_kill_without_tick_is_missing() {
    let _serial = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    let saved = lock().snapshot.take();
    let (code, value) = c_kill(&[999_999], 0);
    lock().snapshot = saved;
    assert_eq!(code, 3);
    assert_eq!(
        status(&value, 999_999),
        ("missing".into(), Some("not_in_snapshot".into()))
    );
}

#[test]
fn phase10_ffi_downsample_keeps_the_largest_unreadable_count_per_segment() {
    let points: Vec<_> = (0..1800)
        .map(|i| HistoryPoint {
            timestamp_s: i,
            mem_bytes: 1,
            cpu_pct: None,
            unknown_mem_count: match i {
                5 => Some(3),
                6 => Some(1),
                _ => None,
            },
        })
        .collect();
    let short = downsample(&points, 60);
    // Points 1..=30 fall into the second kept point (index 30).
    assert_eq!(short[1].timestamp_s, 30);
    assert_eq!(short[1].unknown_mem_count, Some(3));
    assert_eq!(short[0].unknown_mem_count, None);
    assert!(short[2..].iter().all(|p| p.unknown_mem_count.is_none()));
    let json = serde_json::to_value(&short[0]).unwrap();
    assert!(
        json.get("unknown_mem_count").is_none(),
        "absent, not null, when all were read"
    );
}

/// R4/R8 (QA40): whoate_why lists the owner's processes with redacted argv and
/// their ancestors up to launchd; a replayed report has none.
#[test]
fn phase10_ffi_why_lists_owner_processes_with_redacted_argv_and_ancestors() {
    let uid = unsafe { libc::getuid() };
    let mut server = Proc::identity(71900, 1, uid, Some(1), "tmux".into());
    server.argv = vec!["tmux".into(), "new-session".into()];
    let mut shell = Proc::identity(72000, 71900, uid, Some(2), "zsh".into());
    shell.argv = vec!["-zsh".into()];
    shell.cwd = Some("/Users/someone/work".into());
    shell.footprint_bytes = Some(4096);
    let mut node = Proc::identity(72001, 72000, uid, Some(3), "node".into());
    node.argv = [
        "node",
        "/Users/someone/private/app.js",
        "--token=https://secret.example/x",
        "--port",
        "5173",
    ]
    .map(String::from)
    .to_vec();
    node.cwd = Some("/Users/someone/work".into());
    let mut snap = Snapshot::historical(
        "2026-10-21T15:00:00+08:00".into(),
        vec![server.clone(), shell.clone(), node.clone()],
    );
    snap.tmux_panes.push(whoate_core::snapshot::TmuxPane {
        pane_id: "%41".into(),
        target: "work:2.1".into(),
        window_name: None,
        pane_title: None,
        pane_pid: 72000,
        cwd: None,
    });
    let home = Temp::new();
    let engine = replay(snap, &home.0);
    let why = take(whoate_why(engine, 72001));
    whoate_engine_free(engine);
    evidence("replay-why-procs.json", &why);
    assert_eq!(why["owners"][0]["id"], "pane:%41");
    let procs = why["procs"].as_array().unwrap();
    let by_pid = |pid: i64| procs.iter().find(|p| p["pid"] == pid).unwrap();
    let mut redactor = Redactor::default();
    for p in [&shell, &node] {
        let brief = by_pid(p.pid as i64);
        assert_eq!(brief["in_owner"], true);
        assert_eq!(brief["ppid"], p.ppid);
        assert_eq!(brief["comm"], p.comm.as_str());
        assert_eq!(brief["cwd"], "/Users/someone/work");
        assert_eq!(brief["argv"], json!(redactor.redact_argv(&p.argv)));
    }
    assert_eq!(by_pid(72000)["mem_bytes"], 4096);
    assert_eq!(by_pid(72001)["mem_bytes"], Value::Null);
    let ancestor = by_pid(71900);
    assert_eq!(
        ancestor,
        &json!({"pid": 71900, "ppid": 1, "comm": "tmux", "in_owner": false})
    );
    assert_eq!(procs.len(), 3);
    let argv = by_pid(72001)["argv"].to_string();
    for leaked in [
        "https://",
        "secret.example",
        "/Users/someone",
        "private",
        "app.js",
    ] {
        assert!(!argv.contains(leaked), "{leaked} in {argv}");
    }
    assert!(argv.contains("--port") && argv.contains("--token="));
    let tick_owners = why["owners"].as_array().unwrap().len();
    assert_eq!(tick_owners, 1);
}

#[cfg(debug_assertions)]
#[test]
fn phase10_ffi_replayed_report_has_no_process_list() {
    let path = CString::new(
        root()
            .join("tests/fixtures/attribution/sample-v1.json")
            .to_str()
            .unwrap(),
    )
    .unwrap();
    let engine = fixture(path.as_ptr());
    let tick = take(whoate_engine_tick(engine));
    assert!(tick.get("procs").is_none(), "ticks never carry procs");
    let pid = tick["owners"][0]["pids"][0].as_i64().unwrap() as i32;
    let why = take(whoate_why(engine, pid));
    whoate_engine_free(engine);
    assert_eq!(why["procs"], json!([]));
}
