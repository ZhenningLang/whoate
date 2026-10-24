//! Phase 10 round 4 R12 (owner 2026-10-24): an owner, a process group and a
//! history point with some unreadable CPU keep `cpu_pct` null ("not known")
//! and report the readable sum and the unreadable count beside it.
mod common;
use serde_json::Value;
use std::{collections::VecDeque, time::Duration};
use whoate_core::{
    engine::{Clock, Context, Engine},
    history::History,
    model::{Owner, OwnerKind},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    swarm,
    verdict::Language,
};

struct Replay(VecDeque<Snapshot>);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().unwrap())
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

/// A parent and two children; `readable[i]` says whether process i has a CPU
/// time. Between the two frames each readable process uses 100 ms of CPU.
fn frames(readable: [bool; 3]) -> VecDeque<Snapshot> {
    let frame = |cpu_ns: u64| {
        let procs = (0..3)
            .map(|i| {
                let pid = 940001 + i as i32;
                let ppid = if i == 0 { 1 } else { 940001 };
                let mut p = Proc::identity(pid, ppid, 501, Some(1), "demo".into());
                p.footprint_bytes = Some(1024);
                p.cpu_time_ns = readable[i].then_some(cpu_ns);
                p
            })
            .collect();
        Snapshot::historical("2026-10-14T19:40:00+08:00".into(), procs)
    };
    VecDeque::from([frame(0), frame(100_000_000)])
}

fn sample(readable: [bool; 3]) -> Owner {
    let temp = common::Temp::new();
    let context = Context {
        cfg: ConfigReader::isolated(temp.0.clone()),
        uid: 501,
        language: Language::En,
        platform_probe: |_| Ok(false),
    };
    let mut engine = Engine::with_sampler(Replay(frames(readable)), Time::default(), context);
    let report = engine.sample_once().unwrap();
    let owner = report
        .owners
        .iter()
        .find(|o| o.pids.contains(&940001))
        .unwrap();
    assert_eq!(
        owner.pids.len(),
        3,
        "one owner holds the parent and both children"
    );
    owner.clone()
}

#[test]
fn r12_partly_unreadable_cpu_keeps_null_and_reports_the_readable_sum() {
    let owner = sample([true, true, false]);
    assert_eq!(owner.cpu_pct, None, "cpu_pct keeps meaning: not known");
    // 200 ms waited between the frames; 100 ms CPU each → 50 % per readable process.
    assert_eq!(owner.cpu_pct_readable, Some(100.0));
    assert_eq!(owner.unknown_cpu_count, Some(1));
    let json = serde_json::to_value(&owner).unwrap();
    assert_eq!(json["cpu_pct"], Value::Null);
    assert_eq!(json["cpu_pct_readable"], 100.0);
    assert_eq!(json["unknown_cpu_count"], 1);
}

#[test]
fn r12_fully_readable_cpu_is_unchanged_and_adds_no_keys() {
    let owner = sample([true, true, true]);
    assert_eq!(owner.cpu_pct, Some(150.0));
    let json = serde_json::to_value(&owner).unwrap();
    assert!(json.get("cpu_pct_readable").is_none());
    assert!(json.get("unknown_cpu_count").is_none());
}

#[test]
fn r12_nothing_readable_has_a_count_but_no_readable_sum() {
    let owner = sample([false, false, false]);
    assert_eq!(
        (
            owner.cpu_pct,
            owner.cpu_pct_readable,
            owner.unknown_cpu_count
        ),
        (None, None, Some(3))
    );
}

#[test]
fn r12_history_points_carry_the_readable_part() {
    let mut o = Owner::new(
        "app:com.brave.Browser".into(),
        OwnerKind::App,
        "Brave".into(),
    );
    o.pids = vec![1, 2, 3];
    o.cpu_pct_readable = Some(12.5);
    o.unknown_cpu_count = Some(2);
    let mut h = History::default();
    h.record_at(&o, &[], 10);
    let point = &h.series(&o.id)[0];
    assert_eq!(point.cpu_pct, None);
    assert_eq!(
        (point.cpu_pct_readable, point.unknown_cpu_count),
        (Some(12.5), Some(2))
    );
    o.cpu_pct = Some(3.0);
    o.cpu_pct_readable = None;
    o.unknown_cpu_count = None;
    h.record_at(&o, &[], 11);
    let json = serde_json::to_value(&h.series(&o.id)[1]).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"timestamp_s": 11, "mem_bytes": 0, "cpu_pct": 3.0})
    );
}

#[test]
fn r12_process_groups_report_the_readable_part_too() {
    let now = 1791978000_u64 * 1_000_000;
    let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![]);
    let mut owner = Owner::new("pane:%1".into(), OwnerKind::TmuxPane, "demo".into());
    for i in 0..20 {
        let mut p = Proc::identity(990000 + i, 970000, 501, Some(now), "sleep".into());
        p.argv = vec!["sleep".into(), "600".into()];
        p.footprint_bytes = Some(1);
        p.cpu_pct_sampled = (i >= 3).then_some(1.0);
        owner.pids.push(p.pid);
        snap.procs.push(p);
    }
    let groups = swarm::detect(&snap, std::slice::from_ref(&owner));
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!(
        (g.cpu_pct, g.cpu_pct_readable, g.unknown_cpu_count),
        (None, Some(17.0), Some(3))
    );
    for p in &mut snap.procs {
        p.cpu_pct_sampled = None;
    }
    let g = &swarm::detect(&snap, std::slice::from_ref(&owner))[0];
    assert_eq!(
        (g.cpu_pct, g.cpu_pct_readable, g.unknown_cpu_count),
        (None, None, Some(20))
    );
}

/// Optional in the Schema: added properties, never required; a report with them validates.
#[test]
fn r12_fields_are_optional_schema_properties() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema: Value =
        serde_json::from_slice(&std::fs::read(root.join("schema/whoate-v1.schema.json")).unwrap())
            .unwrap();
    let defs = &schema["$defs"];
    for (name, def) in [
        ("owner", &defs["owner"]),
        ("swarm", &defs["swarm"]),
        ("history", &defs["history"]["items"]),
    ] {
        for key in ["cpu_pct_readable", "unknown_cpu_count"] {
            assert!(def["properties"].get(key).is_some(), "{name}.{key} missing");
            let required = def["required"].as_array().unwrap();
            assert!(
                !required.iter().any(|r| r == key),
                "{name}.{key} must stay optional"
            );
        }
    }
    let mut owner_schema = defs["owner"].clone();
    owner_schema["$defs"] = defs.clone();
    let validator = jsonschema::validator_for(&owner_schema).unwrap();
    let owner = sample([true, false, false]);
    assert!(validator.is_valid(&serde_json::to_value(&owner).unwrap()));
}
