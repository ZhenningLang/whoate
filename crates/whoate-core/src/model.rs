//! Shared whoate/v1 contract. Unknown measurements remain explicit.
use crate::snapshot::{Host, SampleIssue, Snapshot, SystemProcs};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub schema: String,
    pub taken_at: String,
    pub host: Option<Host>,
    pub system_procs: SystemProcs,
    pub owners: Vec<Owner>,
    pub swarms: Vec<Swarm>,
    pub issues: Vec<SampleIssue>,
    // In-process authority only. JSON cannot mint a platform verification.
    #[serde(skip)]
    pub(crate) verified_platform: std::collections::BTreeMap<i32, (u32, Option<u64>)>,
}
impl Report {
    pub fn from_snapshot(snap: &Snapshot) -> Self {
        Self {
            schema: "whoate/v1".into(),
            taken_at: snap.taken_at.clone(),
            host: snap.host.clone(),
            system_procs: snap.system_procs.clone(),
            owners: vec![],
            swarms: vec![],
            issues: snap.issues.clone(),
            verified_platform: Default::default(),
        }
    }
    pub fn verified_platform_pids(&self, snap: &Snapshot) -> std::collections::BTreeSet<i32> {
        snap.procs
            .iter()
            .filter(|p| self.verified_platform.get(&p.pid) == Some(&(p.uid, p.start_time)))
            .map(|p| p.pid)
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OwnerKind {
    TmuxPane,
    AgentSession,
    App,
    Orphan,
    Service,
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Certain,
    Inferred,
    Weak,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    pub kind: String,
    pub detail: String,
    pub confidence: Confidence,
}
impl Evidence {
    pub fn new(kind: &str, detail: impl Into<String>, confidence: Confidence) -> Self {
        Self {
            kind: kind.into(),
            detail: detail.into(),
            confidence,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Origin {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pane_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window_name: Option<String>,
    pub tmux_pane: Option<String>,
    pub pane_alive: Option<bool>,
    pub cwd: Option<String>,
    pub ports: Option<Vec<u16>>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Agent {
    pub tool: String,
    pub session_id: Option<String>,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub title: Option<String>,
    pub first_prompt: Option<String>,
    pub first_reply: Option<String>,
    pub status: Option<String>,
    pub cwd: Option<String>,
    pub branch: Option<String>,
    pub model: Option<String>,
    pub cost_usd: Option<f64>,
    pub confidence: Confidence,
    pub updated_at_ms: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_activity_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub idle_seconds: Option<u64>,
    pub stale: bool,
}
impl Agent {
    pub fn unknown(tool: &str) -> Self {
        Self {
            tool: tool.into(),
            session_id: None,
            name: None,
            name_source: None,
            title: None,
            first_prompt: None,
            first_reply: None,
            status: None,
            cwd: None,
            branch: None,
            model: None,
            cost_usd: None,
            confidence: Confidence::Weak,
            updated_at_ms: None,
            last_activity_at: None,
            idle_seconds: None,
            stale: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Owner {
    pub id: String,
    pub kind: OwnerKind,
    pub label: String,
    /// Sum of known footprints; unknown count is separate, not a fabricated sample.
    pub mem_bytes: u64,
    /// RSS sum is a coarse estimate, not guaranteed exclusive physical pages.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reclaimable_physical: Option<PhysicalEstimate>,
    pub unknown_mem_count: usize,
    pub cpu_pct: Option<f64>,
    pub pids: Vec<i32>,
    pub origin: Option<Origin>,
    pub agent: Option<Agent>,
    pub evidence: Vec<Evidence>,
    pub parts: Vec<Part>,
    pub facts: Vec<Fact>,
    /// Stable ids into the knowledge base, not a second copy of each entry.
    pub insights: Vec<String>,
    #[serde(default)]
    pub history: Vec<HistoryPoint>,
    pub verdict: Verdict,
}
impl Owner {
    pub fn new(id: String, kind: OwnerKind, label: String) -> Self {
        Self {
            id,
            kind,
            label,
            mem_bytes: 0,
            reclaimable_physical: None,
            unknown_mem_count: 0,
            cpu_pct: None,
            pids: vec![],
            origin: None,
            agent: None,
            evidence: vec![],
            parts: vec![],
            facts: vec![],
            insights: vec![],
            history: vec![],
            verdict: Verdict::normal(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PhysicalEstimate {
    pub bytes: Option<u64>,
    pub unknown_count: usize,
    pub basis: String,
    pub reason: Option<String>,
}
impl PhysicalEstimate {
    pub fn from_procs<'a>(procs: impl IntoIterator<Item = &'a crate::snapshot::Proc>) -> Self {
        let mut seen = std::collections::BTreeSet::new();
        let mut total = Some(0_u64);
        let mut unknown_count = 0;
        for proc in procs {
            if !seen.insert(proc.pid) {
                continue;
            }
            if let Some(bytes) = proc.resident_bytes {
                total = total.and_then(|n| n.checked_add(bytes));
            } else {
                unknown_count += 1;
            }
        }
        Self {
            bytes: (unknown_count == 0).then_some(total).flatten(),
            unknown_count,
            basis: "RSS sum; includes shared pages; estimate, not guaranteed reclaim".into(),
            reason: if unknown_count > 0 {
                Some("resident_measurement_missing".into())
            } else if total.is_none() {
                Some("resident_sum_overflow".into())
            } else {
                None
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HistoryPoint {
    pub timestamp_s: i64,
    pub mem_bytes: u64,
    pub cpu_pct: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Part {
    pub name: String,
    pub mem_bytes: u64,
    pub process_count: usize,
    pub pids: Vec<i32>,
    pub unknown_mem_count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FactValue {
    Text(String),
    Count(u64),
    Number(f64),
    Bool(bool),
    Strings(Vec<String>),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Fact {
    pub key: String,
    pub label: String,
    pub value: FactValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Ok,
    Watch,
    Act,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub level: Level,
    pub rule: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<String>,
}
impl Verdict {
    pub fn normal() -> Self {
        Self {
            level: Level::Ok,
            rule: "default".into(),
            text: "Normal".into(),
            action: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Burst,
    Spread,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SwarmOwner {
    pub id: String,
    pub count: usize,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swarm {
    pub signature: String,
    pub label: String,
    pub count: usize,
    pub mem_bytes: u64,
    pub unknown_mem_count: usize,
    pub cpu_pct: Option<f64>,
    pub shape: Shape,
    pub owners: Vec<SwarmOwner>,
    pub parents: Vec<i32>,
    pub started_span_s: Option<f64>,
    pub pids: Vec<i32>,
    pub evidence: Vec<Evidence>,
    pub verdict: Verdict,
}

#[cfg(test)]
mod tests {
    #[test]
    fn phase02_model_all_enum_wire_spellings() {
        use super::*;
        for (value, wire) in [
            (OwnerKind::TmuxPane, "tmux_pane"),
            (OwnerKind::AgentSession, "agent_session"),
            (OwnerKind::App, "app"),
            (OwnerKind::Orphan, "orphan"),
            (OwnerKind::Service, "service"),
            (OwnerKind::System, "system"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
        for (value, wire) in [
            (Confidence::Certain, "certain"),
            (Confidence::Inferred, "inferred"),
            (Confidence::Weak, "weak"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
        for (value, wire) in [
            (Level::Ok, "ok"),
            (Level::Watch, "watch"),
            (Level::Act, "act"),
        ] {
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
        for (value, wire) in [(Shape::Burst, "burst"), (Shape::Spread, "spread")] {
            assert_eq!(serde_json::to_value(value).unwrap(), wire);
        }
    }
    #[test]
    fn phase02_model_populated_report_keys_units_and_nulls() {
        use super::*;
        use crate::snapshot::{Host, Pressure, Snapshot};
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![]);
        snap.host = Some(Host {
            kernel_zones: None,
            gpu_memory: None,
            memory_diagnostics: None,
            mem_total_bytes: 17179869184,
            mem_used_bytes: 8589934592,
            compressed_bytes: 1048576,
            wired_bytes: 2097152,
            swap_used_bytes: 3145728,
            pressure: Pressure::Warn,
        });
        let mut report = Report::from_snapshot(&snap);
        let mut owner = Owner::new("service:999001".into(), OwnerKind::Service, "demo".into());
        owner.mem_bytes = 8589934592;
        owner.cpu_pct = Some(125.5);
        owner.pids = vec![999001];
        report.owners.push(owner);
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(
            json.as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec![
                "host",
                "issues",
                "owners",
                "schema",
                "swarms",
                "system_procs",
                "taken_at"
            ]
        );
        assert_eq!(
            json["host"],
            serde_json::json!({"mem_total_bytes":17179869184_u64,"mem_used_bytes":8589934592_u64,"compressed_bytes":1048576,"wired_bytes":2097152,"swap_used_bytes":3145728,"pressure":"warn"})
        );
        assert_eq!(
            json["owners"][0],
            serde_json::json!({"id":"service:999001","kind":"service","label":"demo","mem_bytes":8589934592_u64,"unknown_mem_count":0,"cpu_pct":125.5,"pids":[999001],"origin":null,"agent":null,"evidence":[],"parts":[],"facts":[],"insights":[],"history":[],"verdict":{"level":"ok","rule":"default","text":"Normal"}})
        );
        assert_eq!(
            time::OffsetDateTime::parse(
                json["taken_at"].as_str().unwrap(),
                &time::format_description::well_known::Rfc3339
            )
            .unwrap()
            .offset()
            .whole_seconds(),
            28800
        );
    }
    #[test]
    fn phase02_contract_locked_dependencies_modules_and_public_interface() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for path in [
            "Cargo.toml",
            "Cargo.lock",
            "crates/whoate-core/Cargo.toml",
            "crates/whoate-cli/Cargo.toml",
            "crates/whoate-ffi/Cargo.toml",
            "crates/whoate-core/src/lib.rs",
        ] {
            let output = std::process::Command::new("git")
                .current_dir(&root)
                .args([
                    "show",
                    &format!("7f9c20146457ceec50c30a1dc7a35edb742bb85b:{path}"),
                ])
                .output()
                .unwrap();
            assert!(output.status.success(), "baseline missing: {path}");
            assert_eq!(
                std::fs::read(root.join(path)).unwrap(),
                output.stdout,
                "locked contract drift: {path}"
            );
        }
        let builtins = crate::recognizers::builtins();
        assert_eq!(
            builtins.iter().take(6).map(|r| r.id()).collect::<Vec<_>>(),
            vec![
                "chromium",
                "webkit-host",
                "ide",
                "docker",
                "coding-agents",
                "dev-runtimes"
            ]
        );
        let json = serde_json::to_value(super::Owner::new(
            "service:1".into(),
            super::OwnerKind::Service,
            "demo".into(),
        ))
        .unwrap();
        for key in [
            "id",
            "kind",
            "label",
            "mem_bytes",
            "cpu_pct",
            "pids",
            "origin",
            "agent",
            "evidence",
            "parts",
            "facts",
            "insights",
            "history",
            "verdict",
        ] {
            assert!(json.get(key).is_some(), "missing public key {key}");
        }
        assert_eq!(json["agent"], serde_json::Value::Null);
        assert_eq!(json["parts"], serde_json::json!([]));
    }
    #[test]
    fn phase02_sample_shared_types_roundtrip_and_references() {
        let bytes = include_bytes!("../../../tests/fixtures/attribution/sample-v1.json");
        let report: super::Report = serde_json::from_slice(bytes).unwrap();
        assert_eq!(
            report.owners[0]
                .agent
                .as_ref()
                .unwrap()
                .first_prompt
                .as_deref(),
            Some("请解释这些进程属于谁。")
        );
        assert_eq!(
            report.owners[0]
                .agent
                .as_ref()
                .unwrap()
                .first_reply
                .as_deref(),
            Some("我会先核对进程树与会话证据。")
        );
        assert_eq!(report.owners[0].parts.len(), 2);
        assert_eq!(report.owners[0].facts[0].key, "sample");
        assert_eq!(report.owners[0].insights, vec!["demo.insight"]);
        assert_eq!(report.swarms[0].count, 20);
        assert_eq!(report.swarms[0].owners[0].id, report.owners[0].id);
        assert!(
            report.swarms[0]
                .pids
                .iter()
                .all(|pid| report.owners[0].pids.contains(pid))
        );
        assert_eq!(
            serde_json::to_value(report).unwrap(),
            serde_json::from_slice::<serde_json::Value>(bytes).unwrap()
        );
    }
    #[test]
    fn phase02_model_empty_report_preserves_unknown_and_arrays() {
        let report = super::Report::from_snapshot(&crate::snapshot::Snapshot::historical(
            "2026-10-14T19:40:00+08:00".into(),
            vec![],
        ));
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["schema"], "whoate/v1");
        assert_eq!(json["host"], serde_json::Value::Null);
        assert_eq!(json["owners"], serde_json::json!([]));
        assert_eq!(json["swarms"], serde_json::json!([]));
        let restored: super::Report = serde_json::from_value(json).unwrap();
        assert_eq!(restored, report);
    }
}
