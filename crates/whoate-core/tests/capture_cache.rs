mod common;
use common::Temp;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use whoate_core::{
    sample::{
        SampleError, SampleResult,
        cache::{self, CacheState, CachedProc, SlowSource, SystemCache, Worker},
    },
    snapshot::{Freshness, Proc},
};

fn data(as_of: i64) -> SystemCache {
    SystemCache {
        as_of,
        cpu_issue: None,
        rows: BTreeMap::from([(
            12,
            CachedProc {
                start_time: 100,
                uid: 501,
                footprint_bytes: 1024,
                cpu_pct_sampled: None,
            },
        )]),
    }
}

#[test]
fn cache_freshness_boundaries_and_pid_reuse_are_explicit() {
    let state = CacheState {
        cache: Some(data(100)),
        error: None,
    };
    assert_eq!(state.status(130).status, Freshness::Fresh);
    assert_eq!(state.status(131).status, Freshness::Stale);
    assert_eq!(state.status(99).status, Freshness::Stale);
    assert!(state.status(130).as_of.is_some());
    assert_eq!(CacheState::default().status(100).status, Freshness::Missing);
    let mut procs = vec![Proc::identity(12, 1, 501, Some(100), "node".into())];
    state.merge(&mut procs, 130);
    assert_eq!(procs[0].footprint_bytes, Some(1024));
    procs[0].footprint_bytes = Some(2048);
    state.merge(&mut procs, 130);
    assert_eq!(procs[0].footprint_bytes, Some(2048));
    procs[0].start_time = Some(200);
    procs[0].footprint_bytes = None;
    state.merge(&mut procs, 130);
    assert_eq!(procs[0].footprint_bytes, None);
    procs[0].start_time = Some(100);
    procs[0].uid = 502;
    state.merge(&mut procs, 130);
    assert_eq!(procs[0].footprint_bytes, None);
    assert!(!cache::should_refresh(
        Duration::from_secs(105),
        Some(Duration::from_secs(100))
    ));
    assert!(cache::should_refresh(
        Duration::from_secs(106),
        Some(Duration::from_secs(100))
    ));
    assert!(cache::should_refresh(Duration::from_secs(100), None));
}

#[test]
fn second_frame_cpu_survives_cache_io_and_identity_checked_snapshot_merge() {
    let dir = Temp::new();
    let path = dir.0.join("system.json");
    let mut sample = data(100);
    sample.rows.get_mut(&12).unwrap().cpu_pct_sampled = Some(3.5);
    let mut state = CacheState::default();
    state.update(Ok(sample), &path);
    let loaded = CacheState::load(&path);
    let mut procs = vec![Proc::identity(12, 1, 501, Some(100), "node".into())];
    loaded.merge(&mut procs, 101);
    assert_eq!(procs[0].cpu_pct_sampled, Some(3.5));
    procs[0].cpu_pct_sampled = None;
    procs[0].start_time = Some(101);
    loaded.merge(&mut procs, 101);
    assert_eq!(procs[0].cpu_pct_sampled, None);

    let mut missing = data(100);
    missing.cpu_issue = Some("second_frame_missing".into());
    state.update(Ok(missing), &path);
    let loaded = CacheState::load(&path);
    assert_eq!(
        loaded.cache.as_ref().unwrap().cpu_issue.as_deref(),
        Some("second_frame_missing")
    );
    procs[0].start_time = Some(100);
    loaded.merge(&mut procs, 101);
    assert_eq!(procs[0].cpu_pct_sampled, None);
}

#[test]
fn atomic_cache_replacement_and_failure_keep_original_timestamp_and_data() {
    let dir = Temp::new();
    let path = dir.0.join("system.json");
    assert_eq!(
        CacheState::load(&path).status(100).status,
        Freshness::Missing
    );
    let mut state = CacheState::default();
    state.update(Ok(data(100)), &path);
    assert_eq!(CacheState::load(&path).cache, Some(data(100)));
    assert_eq!(std::fs::read_dir(&dir.0).unwrap().count(), 1);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    state.update(Err(SampleError::new("top", "timeout")), &path);
    assert_eq!(state.status(110).status, Freshness::Stale);
    assert_eq!(state.cache, Some(data(100)));
    assert_eq!(CacheState::load(&path).cache, Some(data(100)));
    let blocked = dir.0.join("directory");
    std::fs::create_dir(&blocked).unwrap();
    state.update(Ok(data(110)), &blocked);
    assert_eq!(state.cache, Some(data(100)));
    assert!(state.error.is_some());
    std::fs::write(&path, b"corrupt PRIVATE_SENTINEL").unwrap();
    let corrupt = CacheState::load(&path);
    assert_eq!(corrupt.status(110).status, Freshness::Missing);
    assert!(!format!("{:?}", corrupt.error).contains("PRIVATE_SENTINEL"));
    std::fs::write(&path, b"{\"as_of\":100,\"rows\":{}}").unwrap();
    assert_eq!(
        CacheState::load(&path).status(110).status,
        Freshness::Missing
    );
}

#[test]
fn blocked_slow_worker_does_not_block_readers_or_multiply_and_drop_cancels() {
    struct Blocked(Arc<AtomicUsize>);
    impl SlowSource for Blocked {
        fn capture(&mut self, stop: &AtomicBool) -> SampleResult<SystemCache> {
            self.0.fetch_add(1, Ordering::Release);
            while !stop.load(Ordering::Acquire) {
                std::thread::park_timeout(Duration::from_millis(1));
            }
            Err(SampleError::new("top", "cancelled"))
        }
    }
    let dir = Temp::new();
    let calls = Arc::new(AtomicUsize::new(0));
    let worker = Worker::start(dir.0.join("system.json"), Blocked(calls.clone()));
    let start = Instant::now();
    while calls.load(Ordering::Acquire) == 0 {
        assert!(start.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    let read_start = Instant::now();
    for _ in 0..100 {
        assert_eq!(worker.state().status(100).status, Freshness::Missing);
    }
    assert!(read_start.elapsed() < Duration::from_millis(50));
    assert_eq!(calls.load(Ordering::Acquire), 1);
    let drop_start = Instant::now();
    drop(worker);
    assert!(drop_start.elapsed() < Duration::from_secs(1));
    assert!(std::fs::read_dir(&dir.0).unwrap().next().is_none());
}

#[test]
fn actual_worker_uses_injected_clock_and_refreshes_exactly_at_six_seconds() {
    use std::sync::{atomic::AtomicU64, mpsc};
    struct Manual(AtomicU64);
    impl cache::Clock for Manual {
        fn now(&self) -> Duration {
            Duration::from_secs(self.0.load(Ordering::Acquire))
        }
    }
    struct Source(mpsc::Sender<()>);
    impl SlowSource for Source {
        fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
            self.0.send(()).unwrap();
            Ok(data(100))
        }
    }
    let dir = Temp::new();
    let clock = Arc::new(Manual(AtomicU64::new(0)));
    let (tx, rx) = mpsc::channel();
    let worker = Worker::start_with_clock(dir.0.join("system.json"), Source(tx), clock.clone());
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    clock.0.store(5, Ordering::Release);
    worker.clock_changed();
    assert!(matches!(
        rx.recv_timeout(Duration::from_millis(20)),
        Err(mpsc::RecvTimeoutError::Timeout)
    ));
    clock.0.store(6, Ordering::Release);
    worker.clock_changed();
    rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert!(matches!(rx.try_recv(), Err(mpsc::TryRecvError::Empty)));
    drop(worker);
}
