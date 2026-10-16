use super::{Language, rules};
use crate::{history::History, model::*, snapshot::*};
fn fixture(kind: OwnerKind, mem: u64) -> (Owner, Snapshot) {
    let mut o = Owner::new("fixture".into(), kind, "demo".into());
    o.mem_bytes = mem;
    o.pids = vec![999001];
    let p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
    let s = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
    (o, s)
}
fn texts(o: &Owner, s: &Snapshot, h: &History, continuous: bool) -> (String, String) {
    (
        rules::owner(o, s, h, continuous, false, Language::Zh).text,
        rules::owner(o, s, h, continuous, false, Language::En).text,
    )
}
#[test]
fn phase02_verdict_language_default() {
    let (o, s) = fixture(OwnerKind::Service, 1);
    let (zh, en) = texts(&o, &s, &History::default(), false);
    assert_eq!(zh, "正常");
    assert_eq!(en, "Normal");
}
#[test]
fn phase02_verdict_language_system() {
    let (o, s) = fixture(OwnerKind::System, 1);
    let (zh, en) = texts(&o, &s, &History::default(), false);
    assert_eq!(zh, "系统进程，勿关");
    assert_eq!(en, "System process; do not terminate");
}
#[test]
fn phase02_verdict_language_orphan_server() {
    let (o, mut s) = fixture(OwnerKind::Orphan, 1);
    s.procs[0].listen_ports = Some(vec![8799]);
    s.procs[0].established_count = Some(0);
    let (zh, en) = texts(&o, &s, &History::default(), false);
    assert_eq!(zh, "无人管理的开发服务器，建议关闭");
    assert_eq!(en, "Unmanaged development server; consider closing it");
}
#[test]
fn phase02_verdict_weak_orphan_never_claims_a_closed_pane() {
    let (o, s) = fixture(OwnerKind::Orphan, 1073741824);
    let (zh, en) = texts(&o, &s, &History::default(), false);
    assert_eq!(zh, "无人管理的开发进程，占 1.00 GiB");
    assert_eq!(en, "Unmanaged development process; using 1.00 GiB");
}
#[test]
fn phase02_verdict_language_orphan_heavy() {
    let (mut o, s) = fixture(OwnerKind::Orphan, 1073741824);
    o.origin = Some(Origin {
        pane_alive: Some(false),
        ..Origin::default()
    });
    let (zh, en) = texts(&o, &s, &History::default(), false);
    assert!(zh.contains("启动它的 pane 已关闭（时长未知），占 1.00 GiB"));
    assert!(en.contains("Its launching pane is closed (duration unknown); using 1.00 GiB"));
}
#[test]
fn phase02_verdict_language_leak() {
    let (mut o, s) = fixture(OwnerKind::Service, 0);
    let now = (crate::swarm::timestamp_us(&s).unwrap() / 1_000_000) as i64;
    let mut h = History::default();
    for t in 0..=1800 {
        o.mem_bytes = if t == 0 { 0 } else { 1073741824 };
        h.record_at(&o, &s.procs, now - 1800 + t);
    }
    let (zh, en) = texts(&o, &s, &h, true);
    assert_eq!(zh, "30 分钟只涨不降，疑似泄漏");
    assert_eq!(en, "Memory never fell for 30 minutes; possible leak");
}
#[test]
fn phase02_verdict_language_fast_growth() {
    let (mut o, s) = fixture(OwnerKind::Service, 0);
    let now = (crate::swarm::timestamp_us(&s).unwrap() / 1_000_000) as i64;
    let mut h = History::default();
    for t in 0..=600 {
        o.mem_bytes = if t == 0 {
            0
        } else if t < 600 {
            499 * 1024 * 1024
        } else {
            500 * 1024 * 1024
        };
        h.record_at(&o, &s.procs, now - 600 + t);
    }
    let (zh, en) = texts(&o, &s, &h, true);
    assert_eq!(zh, "近 10 分钟 +500 MiB，还在涨");
    assert_eq!(en, "+500 MiB over 10 minutes and still growing");
}
#[test]
fn phase02_verdict_language_idle() {
    let (mut o, s) = fixture(OwnerKind::AgentSession, 500 * 1024 * 1024);
    let mut a = Agent::unknown("claude");
    a.status = Some("idle".into());
    a.updated_at_ms = Some(1791978000000 - 7200000);
    o.agent = Some(a);
    let (zh, en) = texts(&o, &s, &History::default(), false);
    assert_eq!(zh, "空闲 7200 秒，可关");
    assert_eq!(en, "Idle for 7200 seconds; consider closing it");
}
fn group(shape: Shape) -> Swarm {
    Swarm {
        signature: "demo".into(),
        label: "demo".into(),
        count: 20,
        mem_bytes: 104857600,
        unknown_mem_count: 0,
        cpu_pct: None,
        shape,
        owners: vec![],
        parents: vec![999001],
        started_span_s: Some(60.0),
        pids: vec![999002],
        evidence: vec![],
        verdict: Verdict::normal(),
    }
}
#[test]
fn phase02_verdict_language_burst() {
    let s = group(Shape::Burst);
    assert!(
        rules::swarm(&s, Language::Zh)
            .text
            .contains("拉起了 20 个相同的 demo，合计 100.0 MiB，疑似失控")
    );
    assert!(
        rules::swarm(&s, Language::En)
            .text
            .contains("20 identical demo processes within 60s, using 100.0 MiB; possible runaway")
    );
}
#[test]
fn phase02_verdict_language_spread() {
    let s = group(Shape::Spread);
    assert!(
        rules::swarm(&s, Language::Zh)
            .text
            .contains("共 20 份 100.0 MiB；关掉闲置归属能一起释放")
    );
    assert!(
        rules::swarm(&s, Language::En)
            .text
            .contains("20 copies, 100.0 MiB; close idle owners to release them together")
    );
}
