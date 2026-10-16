mod common;
use common::Temp;
use std::{collections::VecDeque, time::Duration};
use whoate_core::{
    engine::{Clock, Context, Engine},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};

struct Replay(VecDeque<Snapshot>);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().expect("fixture exhausted"))
    }
}
#[derive(Default)]
struct Time;
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        1
    }
    fn wait(&mut self, _: Duration) {}
}
fn frame(second: i64, memory: Option<u64>) -> Snapshot {
    let mut proc = Proc::identity(930001, 1, 501, Some(1), "demo".into());
    proc.footprint_bytes = memory;
    let at = time::OffsetDateTime::from_unix_timestamp(1_760_000_000 + second).unwrap();
    Snapshot::historical(
        at.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        vec![proc],
    )
}

fn missing_trend(window: i64) -> [whoate_core::model::Report; 6] {
    let home = Temp::new();
    let mut frames = VecDeque::from([frame(-1, Some(0))]);
    for second in 0..=window {
        frames.push_back(frame(second, Some(second as u64 * 1_048_576)));
    }
    frames.push_back(frame(window + 1, None));
    frames.push_back(frame(window + 2, None));
    frames.push_back(frame(window + 4, Some((window + 4) as u64 * 1_048_576)));
    let mut invalid_time = frame(window + 5, Some((window + 5) as u64 * 1_048_576));
    invalid_time.taken_at = "not-a-time".into();
    frames.push_back(invalid_time);
    for second in (window + 6)..=(2 * window + 6) {
        frames.push_back(frame(second, Some(second as u64 * 1_048_576)));
    }
    let mut engine = Engine::with_sampler(
        Replay(frames),
        Time,
        Context {
            cfg: ConfigReader::isolated(home.0.clone()),
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let mut report = engine.tick().unwrap();
    for _ in 1..=window {
        report = engine.tick().unwrap();
    }
    let initial = report;
    let first_missing = engine.tick().unwrap();
    let consecutive_missing = engine.tick().unwrap();
    let recovery = engine.tick().unwrap();
    let invalid_time = engine.tick().unwrap();
    report = invalid_time.clone();
    for _ in 0..=window {
        report = engine.tick().unwrap();
    }
    [
        initial,
        first_missing,
        consecutive_missing,
        recovery,
        invalid_time,
        report,
    ]
}
#[test]
fn phase03_trend_fast_growth_pauses_missing_frames_and_recovers() {
    let [initial, first, consecutive, recovery, invalid, regrown] = missing_trend(600);
    assert_eq!(initial.owners[0].verdict.rule, "fast-growth");
    assert_eq!(first.owners[0].verdict.rule, "default");
    assert_eq!(first.owners[0].history, initial.owners[0].history);
    assert_eq!(consecutive.owners[0].verdict.rule, "default");
    assert_eq!(consecutive.owners[0].history, initial.owners[0].history);
    assert_eq!(recovery.owners[0].verdict.rule, "default");
    assert!(
        !recovery.owners[0]
            .history
            .iter()
            .any(|p| p.timestamp_s == 1760000603)
    );
    assert_eq!(invalid.owners[0].verdict.rule, "default");
    assert_eq!(invalid.owners[0].history, recovery.owners[0].history);
    assert_eq!(regrown.owners[0].verdict.rule, "fast-growth");
}
#[test]
fn phase03_trend_leak_pauses_missing_frames_and_recovers() {
    let [initial, first, consecutive, recovery, invalid, regrown] = missing_trend(1800);
    assert_eq!(initial.owners[0].verdict.rule, "leak-suspect");
    assert_eq!(first.owners[0].verdict.rule, "default");
    assert_eq!(first.owners[0].history, initial.owners[0].history);
    assert_eq!(consecutive.owners[0].verdict.rule, "default");
    assert_eq!(consecutive.owners[0].history, initial.owners[0].history);
    assert_eq!(recovery.owners[0].verdict.rule, "default");
    assert!(
        !recovery.owners[0]
            .history
            .iter()
            .any(|p| p.timestamp_s == 1760001803)
    );
    assert_eq!(invalid.owners[0].verdict.rule, "default");
    assert_eq!(invalid.owners[0].history, recovery.owners[0].history);
    assert_eq!(regrown.owners[0].verdict.rule, "leak-suspect");
}

#[test]
fn phase03_trend_missing_memory_does_not_disable_independent_or_system_rules() {
    for (uid, platform, expected) in [
        (501, false, "orphan-dev-server"),
        (0, false, "system"),
        (501, true, "system"),
    ] {
        let home = Temp::new();
        let mut snap = frame(0, None);
        snap.procs[0].uid = uid;
        if uid == 501 && !platform {
            snap.procs[0].tmux_pane = Some("%closed".into());
        }
        snap.procs[0].listen_ports = Some(vec![8797]);
        snap.procs[0].established_count = Some(0);
        let mut engine = Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap])),
            Time,
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: Language::En,
                platform_probe: if platform {
                    |_| Ok(true)
                } else {
                    |_| Ok(false)
                },
            },
        );
        let r = engine.tick().unwrap();
        assert_eq!(r.owners[0].unknown_mem_count, 1);
        assert_eq!(r.owners[0].verdict.rule, expected);
        assert!(r.owners[0].history.is_empty());
    }
}
