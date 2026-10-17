use std::time::Duration;
use whoate_core::{
    engine::{Clock, Context, Engine},
    model::Level,
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};
mod common;

struct Replay(Snapshot);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.clone())
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

#[test]
fn diagnosis_fseventsd_public_engine_explains_restart_without_enabling_kill() {
    let home = common::Temp::new();
    let mut p = Proc::identity(
        999001,
        1,
        0,
        Some(1_759_968_000_000_000),
        "fseventsd".into(),
    );
    p.footprint_bytes = Some(6_012_954_214);
    let snapshot = Snapshot::historical("2025-10-26T00:00:00Z".into(), vec![p]);
    let mut engine = Engine::with_sampler(
        Replay(snapshot),
        Time::default(),
        Context {
            cfg: ConfigReader::isolated(home.0.clone()),
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let report = engine.sample_once().unwrap();
    assert_eq!(report.owners[0].verdict.level, Level::Act);
    assert_eq!(
        report.owners[0].verdict.action.as_deref(),
        Some("sudo launchctl kickstart -k system/com.apple.fseventsd")
    );
    assert!(report.owners[0].verdict.text.contains("launchd"));
    assert!(report.owners[0].verdict.text.contains("administrator"));
    assert!(report.owners[0].verdict.text.contains("watcher"));
}

fn replay(
    name: &str,
    uid: u32,
    bytes: Option<u64>,
    uptime: Option<u64>,
    platform: bool,
) -> whoate_core::model::Report {
    let home = common::Temp::new();
    let now = 1_760_486_400_u64;
    let mut p = Proc::identity(
        999001,
        1,
        uid,
        uptime.map(|s| (now - s) * 1_000_000),
        name.into(),
    );
    p.footprint_bytes = bytes;
    let snap = Snapshot::historical("2025-10-15T00:00:00Z".into(), vec![p]);
    let mut engine = Engine::with_sampler(
        Replay(snap),
        Time::default(),
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
    engine.sample_once().unwrap()
}
#[test]
fn diagnosis_independent_policy_boundaries_and_spoof_controls() {
    // Independent approved policy, not generated from the production catalog.
    for (name, watch_bytes, watch_s, act) in [
        (
            "fseventsd",
            1_073_741_824,
            259_200,
            Some((3_221_225_472, 604_800)),
        ),
        ("mds", 1_610_612_736, 86_400, Some((4_294_967_296, 259_200))),
        (
            "mds_stores",
            1_610_612_736,
            86_400,
            Some((4_294_967_296, 259_200)),
        ),
        ("mdworker", 1_073_741_824, 0, None),
        ("mdworker_shared", 1_073_741_824, 0, None),
        (
            "WindowServer",
            2_147_483_648,
            86_400,
            Some((5_368_709_120, 259_200)),
        ),
    ] {
        for delta in [-1_i64, 0, 1] {
            let report = replay(
                name,
                0,
                Some((watch_bytes as i64 + delta) as u64),
                Some(watch_s),
                false,
            );
            assert_eq!(
                report.owners[0].verdict.level,
                if delta < 0 { Level::Ok } else { Level::Watch },
                "{name} resource boundary {delta}"
            );
            if watch_s > 0 {
                assert_eq!(
                    replay(
                        name,
                        0,
                        Some(watch_bytes),
                        Some((watch_s as i64 + delta) as u64),
                        false
                    )
                    .owners[0]
                        .verdict
                        .level,
                    if delta < 0 { Level::Ok } else { Level::Watch },
                    "{name} uptime boundary {delta}"
                );
            }
        }
        if let Some((bytes, seconds)) = act {
            for delta in [-1_i64, 0, 1] {
                assert_eq!(
                    replay(
                        name,
                        0,
                        Some((bytes as i64 + delta) as u64),
                        Some(seconds),
                        false
                    )
                    .owners[0]
                        .verdict
                        .level,
                    if delta < 0 { Level::Watch } else { Level::Act },
                    "{name} act memory {delta}"
                );
                assert_eq!(
                    replay(
                        name,
                        0,
                        Some(bytes),
                        Some((seconds as i64 + delta) as u64),
                        false
                    )
                    .owners[0]
                        .verdict
                        .level,
                    if delta < 0 { Level::Watch } else { Level::Act },
                    "{name} act uptime {delta}"
                );
            }
        }
        assert_eq!(
            replay(name, 0, None, Some(1_000_000), false).owners[0]
                .verdict
                .level,
            Level::Ok
        );
        assert_eq!(
            replay(name, 501, Some(10_000_000_000), Some(1_000_000), false).owners[0]
                .verdict
                .action,
            None,
            "user spoof must not get administrator commands"
        );
        let unknown = replay(name, 0, Some(10_000_000_000), None, false);
        assert_eq!(unknown.owners[0].verdict.level, Level::Watch);
        assert!(unknown.owners[0].verdict.text.contains("uptime missing"));
    }
    assert_eq!(
        replay("fseventsd", 0, Some(6_000_000), Some(1_468_800), false).owners[0]
            .verdict
            .level,
        Level::Ok
    );
    let kernel = replay(
        "kernel_task",
        0,
        Some(99_000_000_000),
        Some(1_468_800),
        false,
    );
    assert_eq!(kernel.owners[0].verdict.level, Level::Ok);
    assert_eq!(kernel.owners[0].verdict.action, None);
    let window = replay(
        "WindowServer",
        88,
        Some(6_000_000_000),
        Some(1_000_000),
        true,
    );
    assert_eq!(window.owners[0].verdict.level, Level::Act);
    assert_eq!(window.owners[0].verdict.action, None);
    assert!(window.owners[0].verdict.text.contains("graphical session"));
}

#[test]
fn catalog_rejects_fictitious_local_documents_even_with_valid_shape() {
    use whoate_core::insights::{self, registry, system_policy::Provenance};
    let original = insights::all()
        .into_iter()
        .find(|e| e.id == "system.fseventsd")
        .unwrap();
    for reference in [
        "man 1 fake-whoate-no-such-command",
        "man 1 launchctl; sudo reboot",
        "/System/Library/LaunchDaemons/imaginary.plist",
        "/System/Library/LaunchDaemons/../secrets.plist",
    ] {
        let mut entry = original.clone();
        entry.source = reference.into();
        entry.system.as_mut().unwrap().provenance = vec![Provenance::LocalDoc {
            reference: reference.into(),
        }];
        assert_eq!(
            insights::system_policy::validate(&entry),
            Err("invalid_system_provenance"),
            "{reference}"
        );
    }
    let (_, source) = registry::SOURCES
        .iter()
        .find(|(name, _)| *name == "system.toml")
        .unwrap();
    let bad = source.replace("man 1 launchctl", "man 1 fake-whoate-no-such-command");
    assert!(registry::load_sources(&[("system.toml", &bad)]).is_err());
}
#[test]
fn catalog_preserves_old_entries_and_validates_new_policy_and_provenance() {
    use whoate_core::insights::{
        self, registry,
        system_policy::{Provenance, Restart},
    };
    let old_sources: Vec<_> = registry::SOURCES
        .iter()
        .copied()
        .filter(|(name, _)| *name != "system.toml")
        .collect();
    let old = registry::load_sources(&old_sources).unwrap();
    assert_eq!(old.len(), 28);
    let all = insights::all();
    assert_eq!(all.len(), 35);
    for entry in old {
        assert_eq!(all.iter().find(|e| e.id == entry.id), Some(&entry));
    }
    for name in [
        "fseventsd",
        "mds",
        "mds_stores",
        "mdworker",
        "mdworker_shared",
        "WindowServer",
        "kernel_task",
    ] {
        let e = all
            .iter()
            .find(|e| e.id == format!("system.{name}"))
            .unwrap();
        assert!(e.cause.zh.contains('。'));
        assert!(!e.advice.en.is_empty());
        assert_eq!(e.checked, "2026-10-17");
        let p = e.system.as_ref().unwrap();
        assert_eq!(p.member, name);
        assert!(!p.side_effects.zh.is_empty());
        assert!(!p.launchd.en.is_empty());
    }
    let entry = all
        .iter()
        .find(|e| e.id == "system.fseventsd")
        .unwrap()
        .clone();
    let mut bad = entry.clone();
    bad.system.as_mut().unwrap().restart = Restart::None;
    assert_eq!(
        insights::system_policy::validate(&bad),
        Err("invalid_system_restart")
    );
    let mut bad = entry.clone();
    bad.system.as_mut().unwrap().provenance = vec![Provenance::LocalDoc {
        reference: "https://fake.invalid".into(),
    }];
    assert_eq!(
        insights::system_policy::validate(&bad),
        Err("invalid_system_provenance")
    );
    let mut bad = entry.clone();
    bad.system.as_mut().unwrap().watch.as_mut().unwrap().bytes = 0;
    assert_eq!(
        insights::system_policy::validate(&bad),
        Err("invalid_system_gate")
    );
    let mut bad = entry.clone();
    bad.system.as_mut().unwrap().permissions.en.clear();
    assert_eq!(
        insights::system_policy::validate(&bad),
        Err("invalid_system_safety")
    );
    let mut bad = entry;
    bad.system.as_mut().unwrap().member = "user-controlled".into();
    assert_eq!(
        insights::system_policy::validate(&bad),
        Err("invalid_system_member")
    );
    assert!(registry::load_sources(&[registry::SOURCES[0], registry::SOURCES[0]]).is_err());
}

#[test]
fn diagnosis_zone_absolute_boundaries_only_limit_reboot_advice_to_known_flags() {
    use whoate_core::{
        insights::system_policy::{zone_diagnostics, zone_policy},
        sample::system_memory::{BASIS, KernelZone, KernelZones},
        snapshot::Freshness,
    };
    for (bytes, level) in [
        (2_147_483_647, None),
        (2_147_483_648, Some(Level::Watch)),
        (2_147_483_649, Some(Level::Watch)),
        (4_294_967_295, Some(Level::Watch)),
        (4_294_967_296, Some(Level::Act)),
        (4_294_967_297, Some(Level::Act)),
    ] {
        for flags in [
            (Some(false), Some(false)),
            (Some(true), Some(false)),
            (Some(false), Some(true)),
            (None, None),
        ] {
            let mut zones = KernelZones {
                status: Freshness::Fresh,
                as_of: Some("2026-10-17T00:00:00Z".into()),
                basis: BASIS.into(),
                reason: None,
                top: vec![KernelZone {
                    name: "data.kalloc.1024".into(),
                    bytes,
                    collectible: flags.0,
                    pageable: flags.1,
                }],
            };
            let diagnoses = zone_diagnostics(&zones, Language::En, zone_policy());
            assert_eq!(diagnoses.first().map(|v| v.level), level);
            if let Some(v) = diagnoses.first() {
                assert!(v.text.contains("data.kalloc.1024"));
                assert!(v.text.contains(&bytes.to_string()));
                assert_eq!(
                    v.text.contains("only reboot can reclaim"),
                    flags == (Some(false), Some(false))
                );
                assert_eq!(v.action, None);
            }
            zones.status = Freshness::Stale;
            assert!(zone_diagnostics(&zones, Language::En, zone_policy()).is_empty());
        }
    }
}

#[test]
fn diagnosis_kernel_task_explains_host_zones_even_when_ok() {
    let report = replay(
        "kernel_task",
        0,
        Some(6_000_000_000),
        Some(17 * 86400),
        false,
    );
    assert!(
        report.owners[0]
            .verdict
            .text
            .contains("Inspect host kernel zones")
    );
    assert_eq!(report.owners[0].verdict.level, Level::Ok);
    assert_eq!(report.owners[0].verdict.action, None);
}

#[test]
fn diagnosis_pageable_zone_reports_its_known_property_without_reboot_only_claim() {
    use whoate_core::{
        insights::system_policy::{zone_diagnostics, zone_policy},
        sample::system_memory::{BASIS, KernelZone, KernelZones},
        snapshot::Freshness,
    };
    let zones = KernelZones {
        status: Freshness::Fresh,
        as_of: Some("2026-10-17T00:00:00Z".into()),
        basis: BASIS.into(),
        reason: None,
        top: vec![KernelZone {
            name: "pageable.zone".into(),
            bytes: 4_294_967_296,
            collectible: Some(false),
            pageable: Some(true),
        }],
    };
    for (language, property, wrong_property) in [
        (Language::En, "marked pageable", "collectible or unknown"),
        (Language::Zh, "标记为 pageable", "可回收或未知"),
    ] {
        let advice = zone_diagnostics(&zones, language, zone_policy());
        assert!(advice[0].text.contains(property));
        assert!(!advice[0].text.contains(wrong_property));
        assert_eq!(advice[0].level, Level::Act);
        assert_eq!(advice[0].action, None);
    }
}
