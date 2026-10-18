mod common;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};
use whoate_core::sample::{
    SampleError, SampleResult,
    cache::{CachedProc, SlowSource, SystemCache, Worker},
    top::Scheduled,
};

struct Counting(Arc<AtomicUsize>);
impl SlowSource for Counting {
    fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(SystemCache {
            as_of: 100 + n as i64,
            cpu_issue: None,
            rows: BTreeMap::from([(
                12,
                CachedProc {
                    start_time: 1,
                    uid: 0,
                    footprint_bytes: 4096,
                    cpu_pct_sampled: Some(3.5),
                },
            )]),
        })
    }
}
#[test]
fn phase09_top_runs_independently_at_24s_without_restamping_cached_samples() {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut source = Scheduled::new(Counting(calls.clone()));
    let stop = AtomicBool::new(false);
    for second in [0, 6, 12, 18] {
        assert_eq!(
            source
                .capture_at(Duration::from_secs(second), &stop)
                .unwrap()
                .as_of,
            100
        );
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        source
            .capture_at(Duration::from_secs(24), &stop)
            .unwrap()
            .as_of,
        101
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn phase09_top_transient_failure_retries_on_next_six_second_worker_tick() {
    struct Flaky(usize);
    impl SlowSource for Flaky {
        fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
            self.0 += 1;
            if self.0 == 1 {
                return Err(SampleError::new("top", "transient"));
            }
            Ok(SystemCache {
                as_of: 106,
                cpu_issue: None,
                rows: BTreeMap::from([(
                    12,
                    CachedProc {
                        start_time: 1,
                        uid: 0,
                        footprint_bytes: 4096,
                        cpu_pct_sampled: Some(3.5),
                    },
                )]),
            })
        }
    }
    let mut source = Scheduled::new(Flaky(0));
    let stop = AtomicBool::new(false);
    assert!(source.capture_at(Duration::ZERO, &stop).is_err());
    let recovered = source.capture_at(Duration::from_secs(6), &stop).unwrap();
    assert_eq!(recovered.as_of, 106);
    assert_eq!(recovered.rows[&12].cpu_pct_sampled, Some(3.5));
    assert_eq!(
        source.capture_at(Duration::from_secs(12), &stop).unwrap(),
        recovered
    );
}

#[test]
fn phase09_top_expired_success_then_error_recovers_at_next_worker_tick() {
    struct Sequence(std::collections::VecDeque<SampleResult<SystemCache>>);
    impl SlowSource for Sequence {
        fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
            self.0.pop_front().expect("only three captures are needed")
        }
    }
    let stop = AtomicBool::new(false);
    let mut counting = Counting(Arc::new(AtomicUsize::new(0)));
    let first = counting.capture(&stop).unwrap();
    let recovered = counting.capture(&stop).unwrap();
    let mut source = Scheduled::new(Sequence(std::collections::VecDeque::from([
        Ok(first.clone()),
        Err(SampleError::new("top", "transient")),
        Ok(recovered.clone()),
    ])));
    assert_eq!(source.capture_at(Duration::ZERO, &stop).unwrap(), first);
    assert!(source.capture_at(Duration::from_secs(24), &stop).is_err());
    assert_eq!(
        source.capture_at(Duration::from_secs(30), &stop).unwrap(),
        recovered
    );
    assert_eq!(
        source.capture_at(Duration::from_secs(36), &stop).unwrap(),
        recovered
    );
}

struct Blocking {
    started: mpsc::Sender<()>,
    cancelled: mpsc::Sender<()>,
}
impl SlowSource for Blocking {
    fn capture(&mut self, stop: &AtomicBool) -> SampleResult<SystemCache> {
        self.started.send(()).unwrap();
        while !stop.load(Ordering::Acquire) {
            std::thread::park_timeout(Duration::from_millis(1));
        }
        self.cancelled.send(()).unwrap();
        Err(SampleError::new("test", "cancelled"))
    }
}
#[test]
fn phase09_worker_pause_cancels_and_resume_reuses_one_source() {
    let t = common::Temp::new();
    let (started, starts) = mpsc::channel();
    let (cancelled, cancellations) = mpsc::channel();
    let mut worker = Worker::start(t.0.join("system.json"), Blocking { started, cancelled });
    starts.recv_timeout(Duration::from_secs(1)).unwrap();
    worker.set_active(false);
    cancellations
        .recv_timeout(Duration::from_millis(100))
        .unwrap();
    worker.set_active(false);
    assert!(matches!(starts.try_recv(), Err(mpsc::TryRecvError::Empty)));
    worker.set_active(true);
    starts.recv_timeout(Duration::from_secs(1)).unwrap();
    worker.set_active(true);
    assert!(matches!(starts.try_recv(), Err(mpsc::TryRecvError::Empty)));
    drop(worker);
    cancellations.recv_timeout(Duration::from_secs(1)).unwrap();
}
