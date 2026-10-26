//! Phase 11 QA26 (D2 = A, owner approval 2026-10-27): growth windows are
//! measured in time, so the GUI's 2–60 s cadence forms trends; strictly
//! per-second input keeps the old results exactly.
mod common;
use common::Temp;
use std::{collections::VecDeque, time::Duration};
use whoate_core::{
    engine::{Clock, Context, Engine},
    history::{History, MAX_GAP_S, Trend},
    model::{Owner, OwnerKind},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};

const MIB: i128 = 1024 * 1024;
const ID: &str = "service:930001";

fn owner(mem: u64, unknown: usize) -> Owner {
    let mut o = Owner::new(ID.into(), OwnerKind::Service, "demo".into());
    o.pids = vec![930001, 930002];
    o.mem_bytes = mem;
    o.unknown_mem_count = unknown;
    o
}
fn root(start: u64) -> Proc {
    Proc::identity(930001, 1, 501, Some(start), "demo".into())
}

/// The pre-phase-11 History, copied as the per-second oracle.
#[derive(Default)]
struct Old {
    anchor: Option<u64>,
    points: VecDeque<(i64, u64, Option<usize>, Option<i128>)>,
}
impl Old {
    fn record(&mut self, o: &Owner, start: u64, t: i64) {
        if self.anchor != Some(start) {
            self.points.clear();
        }
        self.anchor = Some(start);
        if o.unknown_mem_count > 0 && o.unknown_mem_count >= o.pids.len() {
            return;
        }
        let unknown = (o.unknown_mem_count > 0).then_some(o.unknown_mem_count);
        if self.points.back().is_some_and(|p| p.0 > t) {
            self.points.clear();
        }
        if self.points.back().is_some_and(|p| p.0 == t) {
            self.points.pop_back();
        }
        let delta = self
            .points
            .back()
            .filter(|p| p.0 + 1 == t && p.2 == unknown)
            .map(|p| o.mem_bytes as i128 - p.1 as i128);
        self.points.push_back((t, o.mem_bytes, unknown, delta));
        if self.points.len() > 1800 {
            self.points.pop_front();
        }
    }
    fn window(&self, seconds: usize) -> Option<Trend> {
        if seconds == 0 || self.points.len() < seconds {
            return None;
        }
        let (mut growth, mut monotonic, mut last) = (0, true, None);
        for p in self.points.iter().skip(self.points.len() - seconds) {
            if last.is_some_and(|last: i64| last + 1 != p.0) {
                return None;
            }
            let d = p.3?;
            growth += d;
            monotonic &= d >= 0;
            last = Some(p.0);
        }
        Some(Trend {
            growth_bytes: growth,
            monotonic,
        })
    }
}

#[test]
fn qa26_per_second_results_match_the_old_window_at_every_step() {
    let mut new = History::default();
    let mut old = Old::default();
    let mut mem: u64 = 100 << 20;
    let mut start = 1;
    let mut seed: u64 = 0x5eed;
    let mut some = 0;
    for t in 0..5000_i64 {
        seed = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let roll = (seed >> 33) % 1000;
        // Growth, then a falling stretch, then growth again.
        mem = if (2000..2600).contains(&t) {
            mem.saturating_sub(1 << 20)
        } else {
            mem + (2 << 20)
        };
        let unknown = match roll {
            0..=4 => 2, // nothing readable: a failed measurement
            5..=9 => 1, // the unreadable count changes
            _ if (3000..3050).contains(&t) => 1,
            _ => 0,
        };
        if t == 4200 {
            start += 1; // root identity reused
        }
        let o = owner(mem, unknown);
        new.record_at(&o, &[root(start)], t);
        old.record(&o, start, t);
        for s in [1, 2, 60, 600, 1800] {
            let (a, b) = (new.window(ID, s), old.window(s));
            assert_eq!(a, b, "t={t} window({s})");
            some += a.is_some() as usize;
        }
    }
    assert!(some > 10_000, "the comparison covered real trends ({some})");
}

#[test]
fn qa26_gui_cadences_form_trends_within_one_interval() {
    for cadence in [2_i64, 30, 60] {
        let mut h = History::default();
        let mut t = 0;
        while t <= 1900 {
            h.record_at(&owner((t as u64) * (2 << 20), 0), &[root(1)], t);
            t += cadence;
        }
        for seconds in [60_usize, 600, 1800] {
            let w = h
                .window(ID, seconds)
                .unwrap_or_else(|| panic!("cadence {cadence} window({seconds})"));
            let exact = seconds as i128 * 2 * MIB;
            assert!(w.monotonic);
            assert!(
                w.growth_bytes >= exact && w.growth_bytes <= exact + cadence as i128 * 2 * MIB,
                "cadence {cadence} window({seconds}) growth {}",
                w.growth_bytes
            );
        }
        let last = t - cadence;
        // A gap of exactly MAX_GAP_S is still continuous; one more second breaks it.
        let mut joined = History::default();
        joined.record_at(&owner(0, 0), &[root(1)], last);
        joined.record_at(&owner(1 << 20, 0), &[root(1)], last + MAX_GAP_S);
        assert!(joined.window(ID, 60).is_some());
        h.record_at(&owner(0, 0), &[root(1)], last + MAX_GAP_S + 1);
        assert_eq!(h.window(ID, 60), None, "cadence {cadence}: > 90 s gap");
        assert_eq!(h.window(ID, 600), None);
    }
}

#[test]
fn qa26_a_failed_measurement_still_breaks_a_slow_cadence() {
    let mut h = History::default();
    for t in (0..=600).step_by(30) {
        h.record_at(&owner(t as u64 * (2 << 20), 0), &[root(1)], t);
    }
    assert!(h.window(ID, 600).is_some());
    h.record_at(&owner(0, 2), &[root(1)], 630);
    h.record_at(&owner(660 * (2 << 20), 0), &[root(1)], 660);
    assert_eq!(h.window(ID, 30), None, "no delta across a failed read");
}

struct Replay(VecDeque<Snapshot>);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().expect("fixture exhausted"))
    }
}
struct Time;
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        1
    }
    fn wait(&mut self, _: Duration) {}
}
fn frame(second: i64) -> Snapshot {
    let mut proc = Proc::identity(930001, 1, 501, Some(1), "demo".into());
    proc.footprint_bytes = Some(100 * 1_048_576 + second as u64 * 2 * 1_048_576);
    let at = time::OffsetDateTime::from_unix_timestamp(1_760_000_000 + second).unwrap();
    Snapshot::historical(
        at.format(&time::format_description::well_known::Rfc3339)
            .unwrap(),
        vec![proc],
    )
}

#[test]
fn qa26_engine_reports_fast_growth_at_gui_cadences() {
    for cadence in [2_i64, 30] {
        let home = Temp::new();
        let seconds: Vec<_> = (0..=660).step_by(cadence as usize).collect();
        let mut engine = Engine::with_sampler(
            // The first tick reads two samples (CPU baseline), so repeat t=0.
            Replay(
                std::iter::once(frame(0))
                    .chain(seconds.iter().map(|s| frame(*s)))
                    .collect(),
            ),
            Time,
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        let mut rules = vec![];
        for _ in &seconds {
            rules.push(engine.tick().unwrap().owners[0].verdict.rule.clone());
        }
        assert_eq!(
            rules.last().map(String::as_str),
            Some("fast-growth"),
            "cadence {cadence}"
        );
        assert!(
            rules.iter().take(5).all(|r| r != "fast-growth"),
            "cadence {cadence}: no trend before 10 minutes of data"
        );
    }
}
