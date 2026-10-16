//! One ordered table. Platform protection precedes every actionable diagnosis.
use super::Language;
use crate::{history::History, model::*, snapshot::Snapshot, swarm::timestamp_us};

struct Context<'a> {
    owner: &'a Owner,
    snapshot: &'a Snapshot,
    history: &'a History,
    continuous: bool,
    platform: bool,
}
struct Rule {
    id: &'static str,
    level: Level,
    test: Option<fn(&Context<'_>) -> bool>,
    shape: Option<Shape>,
}
fn orphan(c: &Context<'_>) -> bool {
    c.owner.kind == OwnerKind::Orphan
}
fn current_trend(c: &Context<'_>) -> bool {
    c.continuous
        && c.owner.unknown_mem_count == 0
        && timestamp_us(c.snapshot)
            .is_some_and(|at| c.history.ends_at(&c.owner.id, (at / 1_000_000) as i64))
}
fn disconnected(c: &Context<'_>) -> bool {
    let ps: Vec<_> = c
        .snapshot
        .procs
        .iter()
        .filter(|p| c.owner.pids.contains(&p.pid))
        .collect();
    orphan(c)
        && ps
            .iter()
            .any(|p| p.listen_ports.as_ref().is_some_and(|v| !v.is_empty()))
        && !ps.is_empty()
        && ps.iter().all(|p| p.established_count == Some(0))
}
static RULES: &[Rule] = &[
    Rule {
        id: "system",
        level: Level::Ok,
        test: Some(|c| c.platform || c.owner.kind == OwnerKind::System),
        shape: None,
    },
    Rule {
        id: "orphan-dev-server",
        level: Level::Act,
        test: Some(disconnected),
        shape: None,
    },
    Rule {
        id: "orphan-heavy",
        level: Level::Act,
        test: Some(|c| orphan(c) && c.owner.mem_bytes >= 1024 * 1024 * 1024),
        shape: None,
    },
    Rule {
        id: "swarm-burst",
        level: Level::Act,
        test: None,
        shape: Some(Shape::Burst),
    },
    Rule {
        id: "swarm-spread",
        level: Level::Watch,
        test: None,
        shape: Some(Shape::Spread),
    },
    Rule {
        id: "leak-suspect",
        level: Level::Act,
        test: Some(|c| {
            current_trend(c)
                && c.history
                    .window(&c.owner.id, 1800)
                    .is_some_and(|w| w.monotonic && w.growth_bytes >= 1024 * 1024 * 1024)
        }),
        shape: None,
    },
    Rule {
        id: "fast-growth",
        level: Level::Watch,
        test: Some(|c| {
            current_trend(c)
                && c.history
                    .window(&c.owner.id, 600)
                    .is_some_and(|w| w.growth_bytes >= 500 * 1024 * 1024)
                && c.history
                    .window(&c.owner.id, 60)
                    .is_some_and(|w| w.growth_bytes > 0)
        }),
        shape: None,
    },
    Rule {
        id: "idle-agent",
        level: Level::Watch,
        test: Some(|c| {
            c.owner.mem_bytes >= 500 * 1024 * 1024
                && c.owner.agent.as_ref().is_some_and(|a| {
                    a.status
                        .as_deref()
                        .is_none_or(|s| matches!(s, "idle" | "waiting" | "stopped"))
                        && a.updated_at_ms.zip(timestamp_us(c.snapshot)).is_some_and(
                            |(updated, now)| {
                                updated >= 0 && (now / 1000) as i128 - updated as i128 >= 7_200_000
                            },
                        )
                })
        }),
        shape: None,
    },
    Rule {
        id: "default",
        level: Level::Ok,
        test: Some(|_| true),
        shape: None,
    },
];
pub fn owner(
    o: &Owner,
    snap: &Snapshot,
    history: &History,
    continuous: bool,
    platform: bool,
    language: Language,
) -> Verdict {
    let c = Context {
        owner: o,
        snapshot: snap,
        history,
        continuous,
        platform,
    };
    let rule = RULES
        .iter()
        .find(|r| r.test.is_some_and(|test| test(&c)))
        .expect("default rule");
    let gib = o.mem_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
    let text = match rule.id {
        "system" => language
            .choose("系统进程，勿关", "System process; do not terminate")
            .into(),
        "orphan-dev-server" => language
            .choose(
                "无人管理的开发服务器，建议关闭",
                "Unmanaged development server; consider closing it",
            )
            .into(),
        "orphan-heavy" => {
            match (
                o.origin.as_ref().and_then(|o| o.pane_alive) == Some(false),
                language,
            ) {
                (true, Language::Zh) => {
                    format!("启动它的 pane 已关闭（时长未知），占 {gib:.2} GiB")
                }
                (true, Language::En) => {
                    format!("Its launching pane is closed (duration unknown); using {gib:.2} GiB")
                }
                (false, Language::Zh) => format!("无人管理的开发进程，占 {gib:.2} GiB"),
                (false, Language::En) => {
                    format!("Unmanaged development process; using {gib:.2} GiB")
                }
            }
        }
        "leak-suspect" => language
            .choose(
                "30 分钟只涨不降，疑似泄漏",
                "Memory never fell for 30 minutes; possible leak",
            )
            .into(),
        "fast-growth" => {
            let growth = history
                .window(&o.id, 600)
                .map(|w| w.growth_bytes)
                .unwrap_or(0)
                / (1024 * 1024);
            if language == Language::Zh {
                format!("近 10 分钟 +{growth} MiB，还在涨")
            } else {
                format!("+{growth} MiB over 10 minutes and still growing")
            }
        }
        "idle-agent" => {
            let seconds = o
                .agent
                .as_ref()
                .and_then(|a| a.updated_at_ms)
                .zip(timestamp_us(snap))
                .map(|(u, n)| (n / 1000) as i128 - u as i128)
                .unwrap_or(0)
                / 1000;
            if language == Language::Zh {
                format!("空闲 {seconds} 秒，可关")
            } else {
                format!("Idle for {seconds} seconds; consider closing it")
            }
        }
        _ => language.choose("正常", "Normal").into(),
    };
    let action =
        matches!(rule.id, "orphan-dev-server" | "orphan-heavy").then(|| kill_command(&o.pids));
    Verdict {
        level: rule.level,
        rule: rule.id.into(),
        text,
        action,
    }
}
fn kill_command(pids: &[i32]) -> String {
    format!(
        "whoate kill {}",
        pids.iter()
            .map(i32::to_string)
            .collect::<Vec<_>>()
            .join(" ")
    )
}
pub fn swarm(s: &Swarm, language: Language) -> Verdict {
    let rule = RULES
        .iter()
        .find(|rule| rule.shape == Some(s.shape))
        .expect("registered swarm shape");
    let mib = s.mem_bytes as f64 / (1024.0 * 1024.0);
    let span = s
        .started_span_s
        .map(|s| format!("{s:.0}s"))
        .unwrap_or_else(|| language.choose("未知时长", "unknown interval").into());
    match s.shape {
        Shape::Burst => Verdict {
            level: rule.level,
            rule: rule.id.into(),
            text: if language == Language::Zh {
                format!(
                    "父进程 {:?} 在 {span} 内拉起了 {} 个相同的 {}，合计 {mib:.1} MiB，疑似失控",
                    s.parents, s.count, s.label
                )
            } else {
                format!(
                    "Parent(s) {:?} started {} identical {} processes within {span}, using {mib:.1} MiB; possible runaway",
                    s.parents, s.count, s.label
                )
            },
            action: Some(kill_command(&s.pids)),
        },
        Shape::Spread => Verdict {
            level: rule.level,
            rule: rule.id.into(),
            text: if language == Language::Zh {
                format!(
                    "各归属分别启动 {}，共 {} 份 {mib:.1} MiB；关掉闲置归属能一起释放",
                    s.label, s.count
                )
            } else {
                format!(
                    "Owners independently started {}: {} copies, {mib:.1} MiB; close idle owners to release them together",
                    s.label, s.count
                )
            },
            action: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use crate::{history::History, model::*, snapshot::*};
    #[test]
    fn phase02_verdict_orphan_unknown_connections_and_system_precedence() {
        let mut p = Proc::identity(999001, 1, 501, Some(1), "node".into());
        p.listen_ports = Some(vec![8799]);
        p.established_count = Some(0);
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut o = Owner::new("orphan:999001".into(), OwnerKind::Orphan, "node".into());
        o.pids = vec![999001];
        for language in [super::super::Language::Zh, super::super::Language::En] {
            let v = super::owner(&o, &snap, &History::default(), false, false, language);
            assert_eq!(v.rule, "orphan-dev-server");
            assert_eq!(v.level, Level::Act);
            assert_eq!(v.action.as_deref(), Some("whoate kill 999001"));
            assert_eq!(
                super::owner(&o, &snap, &History::default(), false, true, language).rule,
                "system"
            );
        }
        snap.procs[0].established_count = None;
        assert_eq!(
            super::owner(
                &o,
                &snap,
                &History::default(),
                false,
                false,
                super::super::Language::En
            )
            .rule,
            "default"
        );
        o.mem_bytes = 1073741823;
        assert_eq!(
            super::owner(
                &o,
                &snap,
                &History::default(),
                false,
                false,
                super::super::Language::En
            )
            .rule,
            "default"
        );
        o.mem_bytes = 1073741824;
        assert_eq!(
            super::owner(
                &o,
                &snap,
                &History::default(),
                false,
                false,
                super::super::Language::En
            )
            .rule,
            "orphan-heavy"
        );
    }
}
