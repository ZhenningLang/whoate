use super::{ConfigReader, Recognizer};
use crate::{
    insights::system_policy,
    model::{Fact, FactValue, Owner, OwnerKind},
    snapshot::{Proc, Snapshot},
};
pub struct SystemServices;
impl Recognizer for SystemServices {
    fn id(&self) -> &'static str {
        "system-services"
    }
    fn matches(&self, p: &Proc) -> bool {
        system_policy::member(p).is_some()
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        if !matches!(owner.kind, OwnerKind::System | OwnerKind::Service) {
            return;
        }
        for p in snap
            .procs
            .iter()
            .filter(|p| owner.pids.contains(&p.pid) && p.uid == 0)
        {
            if let Some(member) = system_policy::member(p) {
                let fact = Fact {
                    key: "system_service".into(),
                    // Phase 11: follows the language like every other label (was both at once).
                    label: cfg.language.choose("系统服务", "System service").into(),
                    value: FactValue::Text(member.into()),
                };
                if !owner.facts.contains(&fact) {
                    owner.facts.push(fact);
                }
            }
        }
    }
}
