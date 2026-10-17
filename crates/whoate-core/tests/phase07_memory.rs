use whoate_core::sample::system_memory::parse_zones;
mod common;

#[test]
fn cache_worker_memory_is_independent_nonblocking_and_cancelled_on_drop() {
    use std::{
        collections::BTreeMap,
        sync::{
            Arc,
            atomic::{AtomicBool, Ordering},
            mpsc,
        },
        time::Duration,
    };
    use whoate_core::{
        sample::{
            SampleError, SampleResult,
            cache::{CachedProc, SlowSource, SystemCache, Worker},
            system_memory::MemoryCache,
        },
        snapshot::Freshness,
    };
    struct Source {
        top_ok: bool,
        entered: mpsc::Sender<()>,
        release: mpsc::Receiver<()>,
        cancelled: Arc<AtomicBool>,
    }
    impl SlowSource for Source {
        fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
            if !self.top_ok {
                return Err(SampleError::new("top", "denied"));
            }
            Ok(SystemCache {
                as_of: 100,
                cpu_issue: None,
                rows: BTreeMap::from([(
                    1,
                    CachedProc {
                        start_time: 1,
                        uid: 0,
                        footprint_bytes: 100,
                        cpu_pct_sampled: None,
                    },
                )]),
            })
        }
        fn capture_memory(&mut self, stop: &AtomicBool) -> Option<MemoryCache> {
            self.entered.send(()).unwrap();
            while !stop.load(Ordering::Acquire) {
                if self.release.recv_timeout(Duration::from_millis(1)).is_ok() {
                    let mut value = MemoryCache::missing("zprint:denied");
                    if !self.top_ok {
                        value.kernel_zones.status = Freshness::Fresh;
                        value.kernel_zones.as_of = Some("1970-01-01T00:01:40Z".into());
                        value.kernel_zones.reason = None;
                        value.kernel_zones.top = parse_zones(include_str!(
                            "../../../tests/fixtures/system-insights/zprint.txt"
                        ))
                        .unwrap();
                    }
                    return Some(value);
                }
            }
            self.cancelled.store(true, Ordering::Release);
            None
        }
    }
    for top_ok in [false, true] {
        let dir = common::Temp::new();
        let (entered, rx) = mpsc::channel();
        let (release, blocked) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker = Arc::new(Worker::start(
            dir.0.join("system.json"),
            Source {
                top_ok,
                entered,
                release: blocked,
                cancelled: cancelled.clone(),
            },
        ));
        rx.recv_timeout(Duration::from_secs(2)).unwrap();
        // A separate reader completes while capture_memory is still blocked.
        let (tx, read) = mpsc::channel();
        let reader = worker.clone();
        let thread = std::thread::spawn(move || {
            tx.send((reader.state(), reader.memory())).unwrap();
        });
        let state = read.recv_timeout(Duration::from_secs(2)).unwrap();
        thread.join().unwrap();
        assert_eq!(state.0.cache.is_some(), top_ok);
        assert_eq!(state.1.kernel_zones.status, Freshness::Missing);
        release.send(()).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !dir.0.join("system-memory.json").is_file() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::yield_now();
        }
        drop(worker);
        let memory = MemoryCache::load(&dir.0.join("system-memory.json"));
        assert_eq!(
            memory.kernel_zones.status,
            if top_ok {
                Freshness::Missing
            } else {
                Freshness::Fresh
            }
        );
        assert_eq!(
            whoate_core::sample::cache::CacheState::load(&dir.0.join("system.json"))
                .cache
                .is_some(),
            top_ok
        );
        assert!(!cancelled.load(Ordering::Acquire));
    }
    let dir = common::Temp::new();
    let (entered, rx) = mpsc::channel();
    let (_release, blocked) = mpsc::channel();
    let cancelled = Arc::new(AtomicBool::new(false));
    let worker = Worker::start(
        dir.0.join("system.json"),
        Source {
            top_ok: true,
            entered,
            release: blocked,
            cancelled: cancelled.clone(),
        },
    );
    rx.recv_timeout(Duration::from_secs(2)).unwrap();
    drop(worker);
    assert!(cancelled.load(Ordering::Acquire));
    assert!(!dir.0.join("system-memory.json").exists());
}

#[test]
fn resident_sampler_enrichment_uses_the_same_identity_window_as_footprint() {
    use std::collections::VecDeque;
    use whoate_core::{
        sample::{
            SampleError, SampleResult,
            libproc::{self, ProcessReader},
            sysctl::ProcArgs,
        },
        snapshot::Proc,
    };
    struct Reader {
        identities: VecDeque<Option<Proc>>,
        reads: usize,
        denied: bool,
    }
    impl ProcessReader for Reader {
        fn identity(&mut self, _: i32) -> Option<Proc> {
            self.identities.pop_front().flatten()
        }
        fn footprint(&mut self, _: i32) -> SampleResult<(u64, u64)> {
            panic!("must use single resource read")
        }
        fn resources(&mut self, _: i32) -> SampleResult<(u64, u64, Option<u64>)> {
            self.reads += 1;
            if self.denied {
                Err(SampleError::new("rusage", "denied"))
            } else {
                Ok((1, 2, Some(4096)))
            }
        }
        fn exe_path(&mut self, _: i32) -> SampleResult<String> {
            Ok("/demo".into())
        }
        fn cwd(&mut self, _: i32) -> SampleResult<String> {
            Ok("/work".into())
        }
        fn args(&mut self, _: i32) -> SampleResult<ProcArgs> {
            Ok(ProcArgs {
                exe_path: "/demo".into(),
                argv: vec![],
                tmux: None,
                tmux_pane: None,
            })
        }
        fn responsible_pid(&mut self, _: i32) -> Option<i32> {
            None
        }
    }
    let base = Proc::identity(999001, 1, 501, Some(1), "demo".into());
    for mode in ["good", "before", "after", "exited", "denied", "other_uid"] {
        let mut p = base.clone();
        if mode == "other_uid" {
            p.uid = 502;
        }
        let mut before = p.clone();
        let mut after = p.clone();
        if mode == "before" {
            before.start_time = Some(2);
        }
        if mode == "after" {
            after.start_time = Some(2);
        }
        let mut reader = Reader {
            identities: [Some(before), (mode != "exited").then_some(after)].into(),
            reads: 0,
            denied: mode == "denied",
        };
        let issues = libproc::enrich_proc(&mut p, 501, &mut reader);
        assert_eq!(p.resident_bytes, (mode == "good").then_some(4096), "{mode}");
        assert_eq!(p.footprint_bytes, (mode == "good").then_some(1), "{mode}");
        assert_eq!(
            reader.reads,
            usize::from(!matches!(mode, "before" | "other_uid")),
            "{mode}"
        );
        assert_eq!(
            issues.is_empty(),
            matches!(mode, "good" | "other_uid"),
            "{mode}"
        );
    }
    let (_, _, resident) = libproc::resources(std::process::id() as i32).unwrap();
    assert!(resident.unwrap() > 0);
}
#[test]
fn resident_estimate_is_independent_and_requires_every_member() {
    use whoate_core::{
        recognizers::ConfigReader,
        snapshot::{Proc, Snapshot},
    };
    let home = common::Temp::new();
    let proc: Proc = serde_json::from_value(serde_json::json!({"pid":999001,"ppid":1,"uid":501,"start_time":1,"comm":"demo","exe_path":null,"argv":[],"cwd":null,"tmux_pane":null,"tmux":null,"footprint_bytes":1,"resident_bytes":4096,"cpu_time_ns":null,"cpu_pct_sampled":null,"responsible_pid":null,"listen_ports":null,"established_count":null})).unwrap();
    let snap = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![proc]);
    let owners = whoate_core::attribute::attribute(
        &snap,
        501,
        &ConfigReader::isolated(home.0.clone()),
        &Default::default(),
    );
    let json = serde_json::to_value(&owners[0]).unwrap();
    assert_eq!(json["mem_bytes"], 1);
    assert_eq!(json["reclaimable_physical"]["bytes"], 4096);
    assert_eq!(json["reclaimable_physical"]["unknown_count"], 0);
}
#[test]
fn zones_use_current_inuse_not_zero_size_or_total_allocations() {
    let text = concat!(
        "                            elem         cur         max        cur         max         cur  alloc  alloc                total \n",
        "zone name                   size        size        size      #elts       #elts       inuse   size  count               allocs \n",
        "-------------------------------------------------------------------------------------------------------------------------------\n",
        "data.kalloc.1024            1024          0K          0K          0           0        1689     0K      0                   0B\n"
    );
    let zones = parse_zones(text).unwrap();
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].bytes, 1_729_536);
    assert_eq!(zones[0].name, "data.kalloc.1024");
    assert_eq!(zones[0].collectible, Some(false));
}

#[test]
fn zones_stable_top_five_flags_and_malformed_table() {
    let fixture = include_str!("../../../tests/fixtures/system-insights/zprint.txt");
    let zones = parse_zones(fixture).unwrap();
    assert_eq!(
        zones
            .iter()
            .map(|z| (z.name.as_str(), z.bytes))
            .collect::<Vec<_>>(),
        vec![
            ("data.kalloc.1024", 1_729_536),
            ("small.total", 1024),
            ("five", 500),
            ("four", 400),
            ("three", 300)
        ]
    );
    assert_eq!(zones[3].collectible, Some(true));
    assert_eq!(zones[4].pageable, Some(true));
    let text = fixture.replace("1689", "0").replace(
        "1024          0K          0K          0           0           1",
        "1          0K          0K          0           0           1",
    );
    let rows = parse_zones(&text).unwrap();
    assert_eq!(
        rows.iter().map(|z| z.name.as_str()).collect::<Vec<_>>(),
        vec!["five", "four", "three", "alpha", "beta"]
    );
    assert_eq!(rows[4].collectible, None);
    for bad in [
        fixture.replace("1689", "not_number"),
        fixture.replace("1024          0K", "18446744073709551615          0K"),
        fixture.replace("zone name", "unknown heading"),
        fixture.replace("inuse", "total"),
        "".into(),
        "{ malformed 雪".into(),
    ] {
        assert!(parse_zones(&bad).is_err());
    }
    assert!(parse_zones(&"x".repeat(whoate_core::sample::system_memory::MAX_BYTES + 1)).is_err());
    let header = fixture.lines().take(3).collect::<Vec<_>>().join("\n") + "\n";
    assert!(
        parse_zones(
            &(header
                + &"row 1 0K 0K 0 0 1 0K 0 0B\n"
                    .repeat(whoate_core::sample::system_memory::MAX_ROWS + 1))
        )
        .is_err()
    );
}

#[test]
fn resident_unique_zero_partial_and_overflow_are_not_footprints() {
    use whoate_core::{model::PhysicalEstimate, snapshot::Proc};
    let mut a = Proc::identity(1, 0, 501, Some(1), "a".into());
    a.footprint_bytes = Some(99);
    a.resident_bytes = Some(0);
    let mut b = Proc::identity(2, 1, 501, Some(2), "b".into());
    b.footprint_bytes = Some(1);
    b.resident_bytes = Some(4096);
    let exact = PhysicalEstimate::from_procs([&a, &b, &b]);
    assert_eq!(exact.bytes, Some(4096));
    assert_eq!(exact.unknown_count, 0);
    b.resident_bytes = None;
    let partial = PhysicalEstimate::from_procs([&a, &b]);
    assert_eq!(partial.bytes, None);
    assert_eq!(partial.unknown_count, 1);
    assert_eq!(
        partial.reason.as_deref(),
        Some("resident_measurement_missing")
    );
    a.resident_bytes = Some(u64::MAX);
    b.resident_bytes = Some(1);
    assert_eq!(
        PhysicalEstimate::from_procs([&a, &b]).reason.as_deref(),
        Some("resident_sum_overflow")
    );
}

#[test]
fn gpu_unverified_cache_cannot_fabricate_available_measurement() {
    use whoate_core::{
        sample::system_memory::{GpuMemory, MemoryCache},
        snapshot::Freshness,
    };
    let missing = GpuMemory::missing();
    assert_eq!(missing.status, Freshness::Missing);
    assert_eq!(missing.bytes, None);
    assert!(missing.reason.as_ref().unwrap().contains("unverified"));
    let home = common::Temp::new();
    let path = home.0.join("memory.json");
    let mut cache = MemoryCache::missing("probe");
    cache.gpu_memory.status = Freshness::Fresh;
    cache.gpu_memory.bytes = Some(0);
    std::fs::write(&path, serde_json::to_vec(&cache).unwrap()).unwrap();
    let loaded = MemoryCache::load(&path);
    assert_eq!(
        loaded.kernel_zones.reason.as_deref(),
        Some("invalid_memory_cache")
    );
    assert_eq!(loaded.gpu_memory.bytes, None);
}

#[test]
fn cache_memory_preserves_last_good_as_of_and_expires_independently() {
    use whoate_core::{
        sample::system_memory::{BASIS, GpuMemory, KernelZones, MemoryCache},
        snapshot::Freshness,
    };
    let home = common::Temp::new();
    let path = home.0.join("memory.json");
    let zones = parse_zones(include_str!(
        "../../../tests/fixtures/system-insights/zprint.txt"
    ))
    .unwrap();
    let mut cache = MemoryCache {
        kernel_zones: KernelZones {
            status: Freshness::Fresh,
            as_of: Some("1970-01-01T00:01:40Z".into()),
            basis: BASIS.into(),
            top: zones,
            reason: None,
        },
        gpu_memory: GpuMemory::missing(),
    };
    assert_eq!(cache.at(130).kernel_zones.status, Freshness::Fresh);
    assert_eq!(cache.at(131).kernel_zones.status, Freshness::Stale);
    assert_eq!(cache.at(99).kernel_zones.status, Freshness::Stale);
    cache.update(MemoryCache::missing("zprint:nonzero_exit"), &path);
    assert_eq!(
        cache.kernel_zones.as_of.as_deref(),
        Some("1970-01-01T00:01:40Z")
    );
    assert_eq!(cache.kernel_zones.status, Freshness::Stale);
    assert_eq!(cache.kernel_zones.top[0].bytes, 1_729_536);
    assert_eq!(MemoryCache::load(&path), cache);
}

#[test]
fn protection_system_diagnosis_never_grants_signal_permission() {
    use whoate_core::kill::{self, Backend, Identity, KillPlan, KillStatus, PlannedPid};
    struct Protected {
        mode: &'static str,
        signals: usize,
    }
    impl Backend for Protected {
        fn self_pid(&self) -> i32 {
            999099
        }
        fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
            if self.mode == "unknown" {
                return Err("unavailable");
            }
            Ok(Some(Identity {
                pid,
                uid: 501,
                start_time: Some(if self.mode == "reuse" { 2 } else { 1 }),
            }))
        }
        fn platform_binary(&mut self, _: i32) -> Result<bool, &'static str> {
            Ok(self.mode == "apple")
        }
        fn signal(&mut self, _: i32, _: i32) -> Result<(), &'static str> {
            self.signals += 1;
            Ok(())
        }
        fn wait(&mut self, _: std::time::Duration) {
            panic!("protected target cannot wait for a signal")
        }
    }
    for mode in ["root", "apple", "self", "unknown", "reuse"] {
        let pid = if mode == "self" { 999099 } else { 999001 };
        let plan = KillPlan {
            pids: vec![PlannedPid {
                pid,
                expected: Some(Identity {
                    pid,
                    uid: if mode == "root" { 0 } else { 501 },
                    start_time: Some(1),
                }),
            }],
            missing_targets: vec![],
        };
        let mut backend = Protected { mode, signals: 0 };
        let report = kill::execute(&plan, false, &mut backend);
        assert_eq!(report.results[0].status, KillStatus::Refused, "{mode}");
        assert_eq!(backend.signals, 0, "{mode}");
    }
}

#[test]
fn resident_measurement_checks_both_identity_reads_and_deduplicates() {
    use whoate_core::kill::{self, Identity, KillPlan, MemoryReader, PlannedPid};
    struct Reader {
        reads: usize,
        mode: &'static str,
        resources: usize,
    }
    impl MemoryReader for Reader {
        fn memory_identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
            self.reads += 1;
            match self.mode {
                "missing" => Ok(None),
                "denied" => Err("denied"),
                _ => Ok(Some(Identity {
                    pid,
                    uid: 501,
                    start_time: Some(if self.mode == "reuse" && self.reads == 2 {
                        2
                    } else {
                        1
                    }),
                })),
            }
        }
        fn resident_bytes(&mut self, _: i32) -> Result<u64, &'static str> {
            self.resources += 1;
            Ok(4096)
        }
        fn host_used_bytes(&mut self) -> Result<u64, &'static str> {
            Ok(1000)
        }
        fn measured_at(&mut self) -> Option<String> {
            Some("2026-10-17T00:00:00Z".into())
        }
    }
    let item = PlannedPid {
        pid: 999001,
        expected: Some(Identity {
            pid: 999001,
            uid: 501,
            start_time: Some(1),
        }),
    };
    let plan = KillPlan {
        pids: vec![item.clone(), item],
        missing_targets: vec![],
    };
    for (mode, expected, count) in [
        ("good", Some(4096), 0),
        ("reuse", None, 1),
        ("missing", Some(0), 0),
        ("denied", None, 1),
    ] {
        let mut reader = Reader {
            reads: 0,
            mode,
            resources: 0,
        };
        let value = kill::measure_memory(&plan, &mut reader);
        assert_eq!(value.target_resident_bytes, expected, "{mode}");
        assert_eq!(value.unknown_count, count, "{mode}");
        assert!(reader.resources <= 1);
        assert!(reader.reads <= 2);
    }
}
