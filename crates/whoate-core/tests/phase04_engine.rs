mod common;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use whoate_core::{
    engine::{Clock, Context, Engine},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};
struct Replay {
    samples: VecDeque<Snapshot>,
    count: Arc<AtomicUsize>,
}
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        self.count.fetch_add(1, Ordering::Relaxed);
        Ok(self.samples.pop_front().unwrap())
    }
}
#[derive(Default)]
struct Time(u128);
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        self.0
    }
    fn wait(&mut self, d: Duration) {
        self.0 += d.as_nanos();
    }
}
fn snapshot(second: u8, memory: Option<u64>) -> Snapshot {
    let mut p = Proc::identity(940001, 1, 501, Some(1), "demo".into());
    p.footprint_bytes = memory;
    p.cpu_time_ns = Some(0);
    Snapshot::historical(format!("2026-10-14T19:40:{second:02}+08:00"), vec![p])
}
#[test]
fn phase04_engine_session_limit_prevents_unlisted_source_opens() {
    use std::{
        ffi::CString,
        os::unix::ffi::OsStrExt,
        process::{Command, Stdio},
        time::Instant,
    };
    for (tool, forbidden) in [
        ("claude", ".claude/sessions/940002.json"),
        ("opencode", ".local/share/opencode/opencode.db"),
        ("kilo", ".local/share/kilo/kilo.db"),
    ] {
        let temp = common::Temp::new();
        let sessions = temp.0.join(".claude/sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        std::fs::write(
            sessions.join("940001.json"),
            r#"{"sessionId":"allowed","cwd":"/demo"}"#,
        )
        .unwrap();
        let transcript = temp.0.join(".claude/projects/-demo/allowed.jsonl");
        std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
        std::fs::write(transcript, "{\"type\":\"user\",\"message\":{\"content\":\"Hello\"}}\n{\"type\":\"assistant\",\"message\":{\"content\":\"World\"}}\n").unwrap();
        let path = temp.0.join(forbidden);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        // Opening this FIFO for reading cannot complete: there is no writer.
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "phase04_engine_session_probe_child",
                "--nocapture",
            ])
            .env("WHOATE_P04_PROBE_HOME", &temp.0)
            .env("WHOATE_P04_PROBE_TOOL", tool)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() {
                break Some(status);
            }
            if start.elapsed() > Duration::from_secs(3) {
                child.kill().unwrap();
                child.wait().unwrap();
                break None;
            }
            std::thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.is_some_and(|s| s.success()),
            "unlisted {tool} source must never open a session file"
        );
    }
}
#[test]
fn phase04_engine_session_probe_child() {
    let Some(home) = std::env::var_os("WHOATE_P04_PROBE_HOME") else {
        return;
    };
    let cfg = ConfigReader::from_lookup(|key| match key {
        "WHOATE_HOME_OVERRIDE" => Some(home.clone()),
        "WHOATE_SESSION_PID" => Some("940001".into()),
        _ => None,
    })
    .unwrap();
    let allowed = Proc::identity(940001, 1, 501, Some(1), "claude".into());
    let other = Proc::identity(
        940002,
        1,
        501,
        Some(2),
        std::env::var("WHOATE_P04_PROBE_TOOL").unwrap(),
    );
    let (agents, issues) =
        whoate_core::sources::Sessions::default().collect(&[allowed, other], &cfg);
    assert_eq!(agents.keys().copied().collect::<Vec<_>>(), vec![940001]);
    assert!(issues.is_empty());
}
#[test]
fn phase04_engine_session_limit_stops_at_first_assistant_record_even_without_text() {
    let temp = common::Temp::new();
    let sessions = temp.0.join(".claude/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(
        sessions.join("940001.json"),
        r#"{"sessionId":"allowed","cwd":"/demo","title":"Metadata title"}"#,
    )
    .unwrap();
    let transcript = temp.0.join(".claude/projects/-demo/allowed.jsonl");
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(transcript, concat!(
        "{\"type\":\"user\",\"message\":{\"content\":\"Hello\"}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_use\",\"name\":\"test\"}]}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":\"UNAUTHORIZED_LATER_TEXT\"}}\n",
        "{\"type\":\"ai-title\",\"aiTitle\":\"UNAUTHORIZED_TAIL_TITLE\"}\n"
    )).unwrap();
    let cfg = ConfigReader::from_lookup(|key| match key {
        "WHOATE_HOME_OVERRIDE" => Some(temp.0.clone().into_os_string()),
        "WHOATE_SESSION_PID" => Some("940001".into()),
        _ => None,
    })
    .unwrap();
    let proc = Proc::identity(940001, 1, 501, Some(1), "claude".into());
    let read = whoate_core::sources::claude_code::read(&proc, &cfg).unwrap();
    assert_eq!(read.agent.first_prompt.as_deref(), Some("Hello"));
    assert_eq!(read.agent.first_reply, None);
    assert_eq!(read.agent.title.as_deref(), Some("Metadata title"));
}
#[test]
fn phase04_engine_session_limit_invalid_pid_is_fail_closed() {
    for value in ["", "0", "-1", "940001,940002", "940001 ", "*", "2147483648"] {
        let result = ConfigReader::from_lookup(|key| match key {
            "HOME" => Some("/unused".into()),
            "WHOATE_SESSION_PID" => Some(value.into()),
            _ => None,
        });
        assert!(
            result.is_err(),
            "invalid session capability must never become unrestricted"
        );
    }
}
#[test]
fn phase04_engine_sample_once_and_continuous_unknown_cpu_keep_real_gaps() {
    let temp = common::Temp::new();
    let count = Arc::new(AtomicUsize::new(0));
    let mut recovered = snapshot(3, Some(10));
    let mut child = Proc::identity(940002, 940001, 501, Some(2), "worker".into());
    child.footprint_bytes = Some(20);
    recovered.procs.push(child);
    let mut engine = Engine::with_sampler(
        Replay {
            samples: VecDeque::from([
                snapshot(0, Some(10)),
                snapshot(1, Some(10)),
                snapshot(2, None),
                recovered,
            ]),
            count: count.clone(),
        },
        Time::default(),
        Context {
            cfg: ConfigReader::isolated(temp.0.clone()),
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let first = engine.tick().unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 2);
    assert_eq!(first.owners[0].cpu_pct, Some(0.0));
    assert_eq!(first.owners[0].history.len(), 1);
    let gap = engine.tick().unwrap();
    assert_eq!(gap.owners[0].history.len(), 1);
    assert_eq!(gap.owners[0].unknown_mem_count, 1);
    assert_ne!(gap.owners[0].verdict.rule, "fast-growth");
    let restored = engine.tick().unwrap();
    assert_eq!(count.load(Ordering::Relaxed), 4);
    assert_eq!(restored.owners[0].cpu_pct, None);
    assert_eq!(
        restored.owners[0]
            .history
            .iter()
            .map(|p| (p.timestamp_s, p.mem_bytes))
            .collect::<Vec<_>>(),
        vec![(1791978001, 10), (1791978003, 30)]
    );
}
#[test]
fn phase04_engine_blocked_native_slow_worker_does_not_delay_or_outlive_engine() {
    use std::sync::{atomic::AtomicBool, mpsc};
    use whoate_core::sample::{
        MacSampler,
        cache::{SlowSource, SystemCache},
    };
    struct Blocked {
        started: mpsc::Sender<()>,
        stopped: mpsc::Sender<()>,
    }
    impl SlowSource for Blocked {
        fn capture(&mut self, stop: &AtomicBool) -> SampleResult<SystemCache> {
            self.started.send(()).unwrap();
            while !stop.load(Ordering::Acquire) {
                std::thread::yield_now();
            }
            self.stopped.send(()).unwrap();
            Err(whoate_core::sample::SampleError::new("test", "cancelled"))
        }
    }
    let root = common::Temp::new();
    let (tx, started) = mpsc::channel();
    let (done, stopped) = mpsc::channel();
    let sampler = MacSampler::with_source_at(
        root.0.join("system.json"),
        Blocked {
            started: tx,
            stopped: done,
        },
    )
    .unwrap();
    started.recv_timeout(Duration::from_secs(3)).unwrap();
    let mut engine = Engine::with_sampler(
        sampler,
        whoate_core::engine::SystemClock::default(),
        Context {
            cfg: ConfigReader::isolated(root.0.clone()),
            uid: unsafe { libc::getuid() },
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let start = std::time::Instant::now();
    let report = engine.sample_once().unwrap();
    println!(
        "QA11_NATIVE_SAMPLE_ONCE_MS={:.3} slow_source_returned={}",
        start.elapsed().as_secs_f64() * 1000.0,
        stopped.try_recv().is_ok()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    assert_eq!(
        report.system_procs.status,
        whoate_core::snapshot::Freshness::Missing
    );
    assert!(report.owners.iter().all(|o| o.history.is_empty()));
    let start = std::time::Instant::now();
    drop(engine);
    assert!(start.elapsed() < Duration::from_secs(1));
    stopped.recv_timeout(Duration::from_secs(1)).unwrap();
}

#[test]
fn phase04_engine_large_report_matches_the_pre_index_pipeline_field_for_field() {
    use whoate_core::{
        attribute, history::History, insights, model::Report, recognizers, sources::Sessions,
        swarm, verdict,
    };
    let root = common::Temp::new();
    let cfg = ConfigReader::isolated(root.0.clone());
    let mut procs = Vec::new();
    for index in 0..512 {
        let mut p = Proc::identity(960000 + index, 1, 501, Some(1), format!("service-{index}"));
        p.footprint_bytes = (index % 17 != 0).then_some(1000 + index as u64);
        p.cpu_time_ns = (index % 19 != 0).then_some(1);
        procs.push(p);
    }
    for (index, comm, exe, args) in [
        (0, "node", "/usr/local/bin/node", vec!["node", "vite"]),
        (
            1,
            "Google Chrome",
            "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
            vec!["Google Chrome"],
        ),
        (
            2,
            "Code",
            "/Applications/Visual Studio Code.app/Contents/MacOS/Electron",
            vec!["Code", "vite"],
        ),
        (
            3,
            "com.apple.WebKit.WebContent",
            "/System/Library/Frameworks/WebKit.framework/com.apple.WebKit.WebContent",
            vec![],
        ),
        (4, "claude", "/usr/local/bin/claude", vec!["claude"]),
        (
            5,
            "python3",
            "/usr/local/bin/python3",
            vec!["python3", "-m", "http.server", "8797"],
        ),
    ] {
        procs[index].comm = comm.into();
        procs[index].exe_path = Some(exe.into());
        procs[index].argv = args.into_iter().map(str::to_owned).collect();
    }
    // A multi-member owner must not mask a missing CPU, even at large scale.
    procs[6].ppid = procs[0].pid;
    procs[6].cpu_time_ns = None;
    let input = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), procs);
    let mut expected_snapshot = input.clone();
    for p in &mut expected_snapshot.procs {
        p.cpu_pct_sampled = p.cpu_time_ns.map(|_| 0.0);
    }
    // Frozen 8eef7e0 loop as the differential oracle, not the optimized helper.
    // It intentionally scans the full snapshot and builds every owner view.
    let (agents, issues) = Sessions::default().collect(&expected_snapshot.procs, &cfg);
    expected_snapshot.issues.extend(issues);
    let mut expected = Report::from_snapshot(&expected_snapshot);
    expected.owners = attribute::attribute(&expected_snapshot, 501, &cfg, &agents);
    let catalog = insights::Catalog::new(insights::registry::load().unwrap());
    let builtins = recognizers::builtins();
    let rules = recognizers::user_rules::Rules::load(&cfg).unwrap();
    for owner in &mut expected.owners {
        let view = recognizers::registry::prepare(owner, &expected_snapshot, &cfg);
        for recognizer in &builtins {
            if recognizers::apply(recognizer.as_ref(), owner, &expected_snapshot, &view).unwrap() {
                catalog.attach(recognizer.id(), owner, &view).unwrap();
            }
        }
        recognizers::registry::finish(owner);
        // Phase 10 round 4 R12: cpu_pct still null; the readable sum and count beside it.
        let unknown_cpu = expected_snapshot
            .procs
            .iter()
            .filter(|p| owner.pids.contains(&p.pid) && p.cpu_pct_sampled.is_none())
            .count();
        if unknown_cpu > 0 {
            owner.cpu_pct_readable = owner.cpu_pct.take();
            owner.unknown_cpu_count = Some(unknown_cpu);
        }
        owner.verdict = verdict::rules::owner(
            owner,
            &expected_snapshot,
            &History::default(),
            false,
            false,
            Language::En,
        );
        rules.apply(owner, &expected_snapshot, &cfg);
    }
    expected.swarms = swarm::detect(&expected_snapshot, &expected.owners);
    for group in &mut expected.swarms {
        group.verdict = verdict::rules::swarm(group, Language::En);
    }
    let mut engine = Engine::with_sampler(
        Replay {
            samples: VecDeque::from([input.clone(), input]),
            count: Arc::new(AtomicUsize::new(0)),
        },
        Time::default(),
        Context {
            cfg,
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let actual = engine.sample_once().unwrap();
    assert!(actual.owners.len() > 500);
    assert_eq!(actual, expected);
}

#[test]
fn phase04_engine_matched_adapter_keeps_full_snapshot_and_rolls_back_ownership() {
    use whoate_core::{
        model::*,
        recognizers::{self, Recognizer},
    };
    struct Adapter(bool);
    impl Recognizer for Adapter {
        fn id(&self) -> &'static str {
            "test-adapter"
        }
        fn matches(&self, p: &Proc) -> bool {
            p.pid == 970001
        }
        fn explain(&self, owner: &mut Owner, snap: &Snapshot, _: &ConfigReader) {
            owner.facts.push(Fact {
                key: "other-process".into(),
                label: "Other process".into(),
                value: FactValue::Count(
                    snap.procs.iter().find(|p| p.pid == 970002).unwrap().pid as u64,
                ),
            });
            if self.0 {
                owner.mem_bytes = 0;
            }
        }
    }
    let root = common::Temp::new();
    let cfg = ConfigReader::isolated(root.0.clone());
    let snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![
            Proc::identity(970001, 1, 501, Some(1), "demo".into()),
            Proc::identity(970002, 1, 501, Some(1), "outside-owner".into()),
        ],
    );
    let mut owner = Owner::new("service:970001".into(), OwnerKind::Service, "demo".into());
    owner.pids = vec![970001];
    owner.mem_bytes = 123;
    assert_eq!(
        recognizers::apply(&Adapter(false), &mut owner, &snap, &cfg),
        Ok(true)
    );
    assert_eq!(owner.facts[0].value, FactValue::Count(970002));
    let before = owner.clone();
    assert_eq!(
        recognizers::apply(&Adapter(true), &mut owner, &snap, &cfg),
        Err("recognizer_changed_ownership")
    );
    assert_eq!(owner, before);
    owner.pids = vec![970003];
    let before = owner.clone();
    assert_eq!(
        recognizers::apply(&Adapter(true), &mut owner, &snap, &cfg),
        Ok(false)
    );
    assert_eq!(owner, before);
}
