//! Approved product heuristics, not Apple baselines. No action here is executed.
use super::{Insight, Localized, Trigger};
use crate::{
    model::{Level, Owner, OwnerKind, Verdict},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Provenance {
    LocalDoc { reference: String },
    OwnerReport { date: String, description: String },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    pub bytes: u64,
    pub uptime_seconds: u64,
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restart {
    None,
    Fseventsd,
    Spotlight,
}
impl Restart {
    pub fn command(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::Fseventsd => Some("sudo launchctl kickstart -k system/com.apple.fseventsd"),
            Self::Spotlight => Some("sudo launchctl kickstart -k system/com.apple.metadata.mds"),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub member: String,
    pub baseline: Localized,
    pub watch: Option<Gate>,
    pub act: Option<Gate>,
    pub restart: Restart,
    pub restart_safety: Localized,
    pub launchd: Localized,
    pub permissions: Localized,
    pub side_effects: Localized,
    pub provenance: Vec<Provenance>,
}
pub const MEMBERS: &[&str] = &[
    "fseventsd",
    "mds",
    "mds_stores",
    "mdworker",
    "mdworker_shared",
    "WindowServer",
    "kernel_task",
];
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ZonePolicy {
    pub watch_bytes: u64,
    pub act_bytes: u64,
    pub source: String,
    pub checked: String,
}
impl ZonePolicy {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.watch_bytes == 0
            || self.act_bytes < self.watch_bytes
            || self.source != "man 1 zprint"
            || time::Date::parse(
                &self.checked,
                &time::format_description::well_known::Iso8601::DATE,
            )
            .is_err()
        {
            return Err("invalid_zone_policy");
        }
        Ok(())
    }
}
pub fn zone_policy() -> &'static ZonePolicy {
    static POLICY: std::sync::OnceLock<ZonePolicy> = std::sync::OnceLock::new();
    POLICY.get_or_init(|| {
        #[derive(Deserialize)]
        struct Document {
            kernel_zones: ZonePolicy,
        }
        let doc: Document =
            toml::from_str(include_str!("system.toml")).expect("embedded zone policy");
        doc.kernel_zones
            .validate()
            .expect("embedded zone policy metadata");
        doc.kernel_zones
    })
}
pub fn zone_diagnostics(
    zones: &crate::sample::system_memory::KernelZones,
    lang: Language,
    policy: &ZonePolicy,
) -> Vec<Verdict> {
    if zones.status != crate::snapshot::Freshness::Fresh {
        return vec![];
    }
    zones.top.iter().filter(|z| z.bytes >= policy.watch_bytes).map(|z| {
        let only_restart = z.collectible == Some(false) && z.pageable == Some(false);
        let explanation = if only_restart {
            lang.choose(
                "zprint 标记为非 collectible、非 pageable：此异常形态只有重启能回收；并非所有内核区均如此",
                "zprint marks this zone non-collectible and non-pageable: only reboot can reclaim this abnormal form; this does not apply to all kernel zones",
            )
        } else if z.pageable == Some(true) {
            lang.choose(
                "占用大；zprint 标记为 pageable（可换页），不能断言只能重启才能回收",
                "High occupancy; zprint marked pageable; reboot-only reclaim is not established",
            )
        } else {
            lang.choose(
                "占用大；回收属性可回收或未知，不能断言只有重启能回收",
                "High occupancy; reclaimability is collectible or unknown; reboot-only reclaim is not established",
            )
        };
        Verdict {
            level: if z.bytes >= policy.act_bytes { Level::Act } else { Level::Watch },
            rule: "kernel-zone-large".into(),
            text: format!("{}: {} bytes; as_of={}; {}. {}", z.name, z.bytes,
                zones.as_of.as_deref().unwrap_or("missing"),
                lang.choose("产品绝对阈值启发式，未测增长窗口", "Product absolute-threshold heuristic; growth window not measured"),
                explanation),
            action: None,
        }
    }).collect()
}
pub fn member(p: &Proc) -> Option<&str> {
    MEMBERS.iter().copied().find(|name| p.comm == *name)
}
pub fn valid_source(e: &Insight) -> bool {
    if let Some(p) = &e.system {
        e.recognizer == "system-services" && p.provenance.iter().any(|source| matches!(source, Provenance::LocalDoc { reference } if reference == &e.source))
    } else {
        e.source.starts_with("https://") && !e.source[8..].trim().is_empty()
    }
}
pub fn validate(e: &Insight) -> Result<(), &'static str> {
    let Some(p) = &e.system else {
        return if e.recognizer == "system-services"
            || matches!(e.trigger, Trigger::SystemService { .. })
        {
            Err("missing_system_policy")
        } else {
            Ok(())
        };
    };
    if !MEMBERS.contains(&p.member.as_str())
        || !matches!(&e.trigger, Trigger::SystemService { member } if member == &p.member)
    {
        return Err("invalid_system_member");
    }
    for text in [
        &p.baseline,
        &p.restart_safety,
        &p.launchd,
        &p.permissions,
        &p.side_effects,
    ] {
        if text.zh.trim().is_empty() || text.en.trim().is_empty() {
            return Err("invalid_system_safety");
        }
    }
    if p.watch.as_ref().is_some_and(|g| g.bytes == 0)
        || p.act.as_ref().is_some_and(|g| g.bytes == 0)
        || p.act.as_ref().is_some_and(|a| {
            p.watch
                .as_ref()
                .is_none_or(|w| a.bytes < w.bytes || a.uptime_seconds < w.uptime_seconds)
        })
    {
        return Err("invalid_system_gate");
    }
    if !matches!(
        (&*p.member, p.restart),
        ("fseventsd", Restart::Fseventsd)
            | ("mds" | "mds_stores", Restart::Spotlight)
            | (
                "mdworker" | "mdworker_shared" | "WindowServer" | "kernel_task",
                Restart::None
            )
    ) {
        return Err("invalid_system_restart");
    }
    if p.provenance.is_empty() {
        return Err("missing_system_provenance");
    }
    for source in &p.provenance {
        let valid = match source {
            Provenance::LocalDoc { reference } => {
                // Closed catalog of documents verified during P07 research.
                // Adding a source requires checking it, not executing its string.
                matches!(
                    reference.as_str(),
                    "man 1 launchctl"
                        | "man 1 zprint"
                        | "man 8 mds"
                        | "man 8 mdworker"
                        | "/System/Library/LaunchDaemons/com.apple.fseventsd.plist"
                        | "/System/Library/LaunchDaemons/com.apple.metadata.mds.plist"
                        | "/System/Library/LaunchDaemons/com.apple.WindowServer.plist"
                )
            }
            Provenance::OwnerReport { date, description } => {
                time::Date::parse(date, &time::format_description::well_known::Iso8601::DATE)
                    .is_ok()
                    && !description.trim().is_empty()
            }
        };
        if !valid {
            return Err("invalid_system_provenance");
        }
    }
    Ok(())
}
fn local(text: &Localized, lang: Language) -> &str {
    lang.choose(&text.zh, &text.en)
}
pub fn diagnose(
    o: &Owner,
    snap: &Snapshot,
    platform_pids: &std::collections::BTreeSet<i32>,
    lang: Language,
    catalog: &[Insight],
) -> Option<Verdict> {
    if !matches!(o.kind, OwnerKind::System | OwnerKind::Service) {
        return None;
    }
    let now = crate::swarm::timestamp_us(snap);
    let mut best: Option<Verdict> = None;
    for p in snap
        .procs
        .iter()
        .filter(|p| o.pids.contains(&p.pid) && (p.uid == 0 || platform_pids.contains(&p.pid)))
    {
        let Some(entry) = catalog.iter().find(|e| {
            e.system
                .as_ref()
                .is_some_and(|policy| member(p) == Some(policy.member.as_str()))
        }) else {
            continue;
        };
        let policy = entry.system.as_ref().expect("matched policy");
        let uptime = p
            .start_time
            .zip(now)
            .and_then(|(start, now)| now.checked_sub(start))
            .map(|us| us / 1_000_000);
        let meets = |g: &Gate| {
            p.footprint_bytes.is_some_and(|n| n >= g.bytes)
                && uptime.is_some_and(|s| s >= g.uptime_seconds)
        };
        let level = if policy.act.as_ref().is_some_and(meets) {
            Level::Act
        } else if policy.watch.as_ref().is_some_and(|g| {
            meets(g) || uptime.is_none() && p.footprint_bytes.is_some_and(|n| n >= g.bytes)
        }) {
            Level::Watch
        } else {
            Level::Ok
        };
        let mut text = format!(
            "{}: {} bytes; {}. {} {} {} {} {}",
            policy.member,
            p.footprint_bytes
                .map(|n| n.to_string())
                .unwrap_or_else(|| "missing".into()),
            lang.choose(
                "产品启发式，非 Apple 常态基线",
                "Product heuristic, not an Apple baseline"
            ),
            local(&entry.cause, lang),
            local(&policy.baseline, lang),
            local(&policy.permissions, lang),
            local(&policy.launchd, lang),
            local(&policy.side_effects, lang)
        );
        if level != Level::Ok || policy.member == "kernel_task" {
            text.push_str(&format!(
                " {} {}",
                local(&entry.advice, lang),
                local(&policy.restart_safety, lang)
            ));
        }
        if uptime.is_none() {
            text.push_str(lang.choose("；运行时长缺测", "; uptime missing"));
        }
        let verdict = Verdict {
            level,
            rule: entry.id.clone(),
            text,
            action: (level != Level::Ok)
                .then(|| policy.restart.command().map(str::to_owned))
                .flatten(),
        };
        let rank = |l| match l {
            Level::Ok => 0,
            Level::Watch => 1,
            Level::Act => 2,
        };
        if best
            .as_ref()
            .is_none_or(|old| rank(level) > rank(old.level))
        {
            best = Some(verdict);
        }
    }
    best
}

/// Projection revalidates the command against root or Engine-verified members;
/// a caller-provided action/rule string alone never grants this exemption.
pub fn public_action(
    o: &Owner,
    snap: &Snapshot,
    platform_pids: &std::collections::BTreeSet<i32>,
) -> Option<String> {
    if o.verdict.action.is_none() || !o.verdict.rule.starts_with("system.") {
        return None;
    }
    let entries = super::all();
    let verdict = diagnose(o, snap, platform_pids, Language::En, &entries)?;
    (verdict.rule == o.verdict.rule
        && verdict.level == o.verdict.level
        && verdict.action == o.verdict.action)
        .then_some(verdict.action)
        .flatten()
}
