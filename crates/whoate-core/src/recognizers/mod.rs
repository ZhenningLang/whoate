pub mod chromium;
pub mod coding_agents;
pub mod dev_runtimes;
pub mod docker;
pub mod ide;
pub mod registry;
pub mod system_services;
pub mod user_rules;
pub mod webkit_host;

#[path = "../config_reader.rs"]
mod config_reader;
pub use config_reader::ConfigReader;

use crate::{
    model::Owner,
    snapshot::{Proc, Snapshot},
};
pub trait Recognizer {
    fn id(&self) -> &'static str;
    fn matches(&self, proc: &Proc) -> bool;
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader);
}

pub fn builtins() -> Vec<Box<dyn Recognizer>> {
    registry::builtins()
}

/// Reject a recognizer that violates the ownership contract, including future plugins.
pub fn apply(
    recognizer: &dyn Recognizer,
    owner: &mut Owner,
    snap: &Snapshot,
    cfg: &ConfigReader,
) -> Result<bool, &'static str> {
    if !snap
        .procs
        .iter()
        .any(|p| owner.pids.contains(&p.pid) && recognizer.matches(p))
    {
        return Ok(false);
    }
    apply_matched(recognizer, owner, snap, cfg)
}

/// Engine has already matched an owner member through its per-tick PID index.
/// Keep the full snapshot and the same rollback guard as the public entry.
pub(crate) fn apply_matched(
    recognizer: &dyn Recognizer,
    owner: &mut Owner,
    snap: &Snapshot,
    cfg: &ConfigReader,
) -> Result<bool, &'static str> {
    let before = owner.clone();
    recognizer.explain(owner, snap, cfg);
    let mut fixed = owner.clone();
    fixed.parts = before.parts.clone();
    fixed.facts = before.facts.clone();
    fixed.insights = before.insights.clone();
    if fixed != before {
        *owner = before;
        return Err("recognizer_changed_ownership");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{model::*, sources::testing::Temp};
    #[test]
    fn phase02_insights_isolated_reader_cannot_read_outside_fixture_root() {
        let home = Temp::new();
        let outside = Temp::new();
        outside.write("package.json", r#"{"name":"outside-fixture"}"#);
        let reader = ConfigReader::isolated(home.0.clone());
        assert_eq!(reader.project_file(&outside.0, "package.json"), None);
        assert!(reader.read_json(&outside.0.join("package.json")).is_err());
    }
    #[test]
    fn phase02_recognizers_rejects_ownership_mutation_negative_control() {
        struct Bad;
        impl Recognizer for Bad {
            fn id(&self) -> &'static str {
                "bad"
            }
            fn matches(&self, _: &Proc) -> bool {
                true
            }
            fn explain(&self, o: &mut Owner, _: &Snapshot, _: &ConfigReader) {
                o.mem_bytes = 0;
                o.pids.clear();
            }
        }
        let t = Temp::new();
        let p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut owner = Owner::new("service:999001".into(), OwnerKind::Service, "demo".into());
        owner.pids = vec![999001];
        owner.mem_bytes = 100;
        let before = owner.clone();
        assert_eq!(
            apply(
                &Bad,
                &mut owner,
                &snap,
                &ConfigReader::isolated(t.0.clone())
            ),
            Err("recognizer_changed_ownership")
        );
        assert_eq!(owner, before);
    }
}
