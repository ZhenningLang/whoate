//! Phase 08: session label rule (QA3) and whole-machine CPU (QA4).
use std::collections::BTreeMap;
use whoate_core::{
    attribute::{agent_label, attribute},
    model::{Agent, Report},
    recognizers::ConfigReader,
    sample::host_cpu::{CpuMeter, CpuSource, CpuTicks, usage},
    snapshot::{Freshness, Proc, Snapshot},
};

fn agent(name: Option<&str>, source: Option<&str>, title: Option<&str>) -> Agent {
    let mut a = Agent::unknown("claude");
    a.name = name.map(Into::into);
    a.name_source = source.map(Into::into);
    a.title = title.map(Into::into);
    a
}

#[test]
fn derived_names_yield_to_titles_and_manual_names_win() {
    let title = "X 数据流部署到 Claw 机器";
    let derived = agent(Some("7-74"), Some("derived"), Some(title));
    assert_eq!(agent_label(&derived), title);
    assert_eq!(
        derived.name.as_deref(),
        Some("7-74"),
        "the derived name is kept"
    );
    for empty in [None, Some(""), Some("  ")] {
        assert_eq!(
            agent_label(&agent(Some("7-74"), Some("derived"), empty)),
            "7-74"
        );
    }
    assert_eq!(
        agent_label(&agent(Some("我的会话"), Some("manual"), Some(title))),
        "我的会话"
    );
    assert_eq!(
        agent_label(&agent(None, None, Some(title))),
        title,
        "opencode: title only"
    );
    assert_eq!(agent_label(&agent(None, Some("derived"), None)), "claude");
    assert_eq!(agent_label(&agent(None, None, None)), "claude");
}

#[test]
fn attribution_labels_a_derived_session_by_its_title() {
    let snap = Snapshot::historical(
        "2026-10-20T00:00:00Z".into(),
        vec![Proc::identity(999001, 1, 501, Some(1), "claude".into())],
    );
    let mut session = agent(Some("dotfiles-37"), Some("derived"), Some("整理 dotfiles"));
    session.session_id = Some("s-1".into());
    let agents = BTreeMap::from([(999001, session)]);
    let owners = attribute(
        &snap,
        501,
        &ConfigReader::isolated(std::env::temp_dir()),
        &agents,
    );
    let owner = owners.iter().find(|o| o.agent.is_some()).unwrap();
    assert_eq!(owner.label, "整理 dotfiles");
    assert_eq!(
        owner.agent.as_ref().unwrap().name.as_deref(),
        Some("dotfiles-37")
    );
    assert_eq!(
        owner.agent.as_ref().unwrap().name_source.as_deref(),
        Some("derived")
    );
}

struct Fake {
    ticks: Vec<Result<CpuTicks, &'static str>>,
}
impl CpuSource for Fake {
    fn ticks(&mut self) -> Result<CpuTicks, &'static str> {
        self.ticks.remove(0)
    }
    fn load_1m(&mut self) -> Option<f64> {
        Some(3.5)
    }
    fn logical_cores(&mut self) -> Option<u32> {
        Some(12)
    }
}
fn ticks(user: u64, system: u64, idle: u64, nice: u64) -> CpuTicks {
    CpuTicks {
        user,
        system,
        idle,
        nice,
    }
}

#[test]
fn usage_is_the_busy_share_of_the_tick_difference() {
    let mut meter = CpuMeter::new(Fake {
        ticks: vec![
            Ok(ticks(1000, 500, 8000, 0)),
            // +300 user +100 system +580 idle +20 nice: (1000-580)/1000 busy.
            Ok(ticks(1300, 600, 8580, 20)),
        ],
    });
    let first = meter.sample();
    assert_eq!(first.status, Freshness::Missing);
    assert_eq!(first.usage_pct, None, "no difference yet is never 0%");
    assert_eq!(first.reason.as_deref(), Some("first_sample"));
    assert_eq!((first.load_1m, first.logical_cores), (Some(3.5), Some(12)));
    let second = meter.sample();
    assert_eq!(second.status, Freshness::Fresh);
    assert_eq!(second.usage_pct, Some(42.0));
    assert_eq!(second.reason, None);
}

#[test]
fn reset_read_failure_and_counter_regression_are_missing_with_reasons() {
    let mut meter = CpuMeter::new(Fake {
        ticks: vec![
            Ok(ticks(10, 10, 10, 0)),
            Ok(ticks(5, 10, 10, 0)),
            Err("host_statistics_failed"),
            Ok(ticks(20, 20, 20, 0)),
            Ok(ticks(20, 20, 20, 0)),
        ],
    });
    meter.sample();
    for expected in [
        "counter_reset",
        "host_statistics_failed",
        "first_sample",
        "no_elapsed_ticks",
    ] {
        let cpu = meter.sample();
        assert_eq!(cpu.status, Freshness::Missing, "{expected}");
        assert_eq!(cpu.usage_pct, None, "{expected}");
        assert_eq!(cpu.reason.as_deref(), Some(expected));
    }
    let mut paused = CpuMeter::new(Fake {
        ticks: vec![Ok(ticks(1, 1, 1, 0)), Ok(ticks(2, 2, 2, 0))],
    });
    paused.sample();
    paused.reset();
    assert_eq!(paused.sample().reason.as_deref(), Some("first_sample"));
    assert_eq!(usage(ticks(0, 0, 0, 0), ticks(0, 0, 100, 0)), Ok(0.0));
    assert_eq!(usage(ticks(0, 0, 0, 0), ticks(60, 40, 0, 0)), Ok(100.0));
}

#[test]
fn old_reports_without_cpu_round_trip_unchanged() {
    let text = include_str!("../../../tests/fixtures/attribution/sample-v1.json");
    let original: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(original["host"].get("cpu").is_none());
    let report: Report = serde_json::from_str(text).unwrap();
    assert_eq!(report.host.as_ref().unwrap().cpu, None);
    let again = serde_json::to_value(&report).unwrap();
    assert_eq!(again, original);
    assert_eq!(
        serde_json::to_string(&again).unwrap(),
        serde_json::to_string(&original).unwrap()
    );
}

#[test]
fn schema_registers_host_cpu() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schema/whoate-v1.schema.json")).unwrap();
    let host = schema["$defs"]["host"]["anyOf"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["type"] == "object")
        .unwrap();
    let cpu = &host["properties"]["cpu"];
    let mut required: Vec<_> = cpu["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    required.sort();
    assert_eq!(
        required,
        ["load_1m", "logical_cores", "reason", "status", "usage_pct"]
    );
    assert_eq!(
        cpu["properties"]["status"]["enum"],
        serde_json::json!(["fresh", "missing"])
    );
}

#[cfg(target_os = "macos")]
#[test]
fn native_meter_reads_real_ticks() {
    let mut meter: CpuMeter = CpuMeter::default();
    let first = meter.sample();
    assert_eq!(first.reason.as_deref(), Some("first_sample"));
    assert!(first.logical_cores.is_some_and(|n| n > 0));
    assert!(first.load_1m.is_some());
    // Counters advance in bursts; a gap without ticks is missing, never 0%.
    let mut second = meter.sample();
    for _ in 0..15 {
        std::thread::sleep(std::time::Duration::from_millis(200));
        second = meter.sample();
        if second.status == Freshness::Fresh {
            break;
        }
        assert_eq!(second.reason.as_deref(), Some("no_elapsed_ticks"));
        assert_eq!(second.usage_pct, None);
    }
    assert_eq!(second.status, Freshness::Fresh, "{second:?}");
    assert!(second.usage_pct.is_some_and(|p| (0.0..=100.0).contains(&p)));
}
