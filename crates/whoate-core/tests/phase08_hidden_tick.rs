//! Phase 08 round 3 (QA31): the GUI's hidden full tick runs with the engine
//! paused. Ticking a paused engine must not restart the slow worker (`top`).
mod common;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};
use whoate_core::{
    engine::{Context, Engine, SystemClock},
    recognizers::ConfigReader,
    sample::{
        MacSampler, SampleError, SampleResult,
        cache::{SlowSource, SystemCache},
    },
    verdict::Language,
};

struct Counting(Arc<AtomicUsize>);
impl SlowSource for Counting {
    fn capture(&mut self, _: &AtomicBool) -> SampleResult<SystemCache> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(SampleError::new("test", "counted"))
    }
}

fn wait_for(count: &AtomicUsize, above: usize) -> bool {
    let began = Instant::now();
    while began.elapsed() < Duration::from_secs(3) {
        if count.load(Ordering::SeqCst) > above {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

#[test]
fn paused_engine_ticks_without_restarting_the_slow_worker() {
    let root = common::Temp::new();
    let captures = Arc::new(AtomicUsize::new(0));
    let sampler =
        MacSampler::with_source_at(root.0.join("system.json"), Counting(captures.clone())).unwrap();
    assert!(wait_for(&captures, 0), "the worker captures once on start");
    let mut engine = Engine::with_sampler(
        sampler,
        SystemClock::default(),
        Context {
            cfg: ConfigReader::isolated(root.0.clone()),
            uid: unsafe { libc::getuid() },
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    engine.set_active(false);
    let paused = captures.load(Ordering::SeqCst);
    for _ in 0..3 {
        engine.tick().unwrap();
    }
    // The worker captures at once when it starts; a second is ample to see a restart.
    std::thread::sleep(Duration::from_secs(1));
    assert_eq!(
        captures.load(Ordering::SeqCst),
        paused,
        "three paused ticks captured again"
    );
    // Control: the counter does see a restarted worker.
    engine.set_active(true);
    assert!(wait_for(&captures, paused), "resume restarts the worker");
}
