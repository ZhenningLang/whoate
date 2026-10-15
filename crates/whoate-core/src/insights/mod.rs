//! Data-only knowledge matching. No trigger can run code, models or networking.
use crate::{
    model::{FactValue, Owner},
    recognizers::ConfigReader,
};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Localized {
    pub zh: String,
    pub en: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Trigger {
    Count {
        signal: String,
        min: u64,
    },
    Memory {
        min_bytes: u64,
    },
    Config {
        key: String,
        equals: serde_json::Value,
    },
    All {
        triggers: Vec<Trigger>,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Insight {
    pub id: String,
    pub recognizer: String,
    pub trigger: Trigger,
    pub cause: Localized,
    pub advice: Localized,
    pub source: String,
    pub checked: String,
}
pub fn all() -> Vec<Insight> {
    vec![]
}

#[derive(Clone, Default)]
pub struct Catalog {
    pub entries: Vec<Insight>,
}
impl Catalog {
    pub fn new(entries: Vec<Insight>) -> Self {
        Self { entries }
    }
    pub fn all(&self) -> &[Insight] {
        &self.entries
    }
    pub fn attach(
        &self,
        recognizer: &str,
        owner: &mut Owner,
        cfg: &ConfigReader,
    ) -> Result<(), &'static str> {
        for entry in self.entries.iter().filter(|e| e.recognizer == recognizer) {
            if !entry.source.starts_with("https://")
                || entry.cause.zh.is_empty()
                || entry.cause.en.is_empty()
                || entry.advice.zh.is_empty()
                || entry.advice.en.is_empty()
                || time::Date::parse(
                    &entry.checked,
                    &time::format_description::well_known::Iso8601::DATE,
                )
                .is_err()
            {
                return Err("invalid_insight_metadata");
            }
            if evaluate(&entry.trigger, owner, cfg)? && !owner.insights.contains(&entry.id) {
                owner.insights.push(entry.id.clone());
            }
        }
        Ok(())
    }
}
pub fn evaluate(
    trigger: &Trigger,
    owner: &Owner,
    cfg: &ConfigReader,
) -> Result<bool, &'static str> {
    // Iterative walk prevents an unbounded recursive stack on parsed trigger trees.
    let mut pending = vec![trigger];
    let mut matched = true;
    while let Some(trigger) = pending.pop() {
        let current = match trigger {
            Trigger::Memory { min_bytes } => owner.mem_bytes >= *min_bytes,
            Trigger::Count { signal, min } => {
                let value = match signal.as_str() {
                    "process_count" => owner.pids.len() as u64,
                    "mcp_count" => owner
                        .parts
                        .iter()
                        .filter(|p| p.name.starts_with("MCP:"))
                        .map(|p| p.process_count as u64)
                        .sum(),
                    _ => owner
                        .facts
                        .iter()
                        .find_map(|f| match (&f.value, f.key.as_str()) {
                            (FactValue::Count(n), key) if key == format!("count.{signal}") => {
                                Some(*n)
                            }
                            _ => None,
                        })
                        .ok_or("unknown_count_signal")?,
                };
                value >= *min
            }
            Trigger::Config { key, equals } => {
                cfg.values.get(key).ok_or("unknown_config_signal")? == equals
            }
            Trigger::All { triggers } => {
                pending.extend(triggers);
                true
            }
        };
        matched &= current;
    }
    Ok(matched)
}

#[cfg(test)]
mod tests {
    use crate::{model::*, sources::testing::Temp};
    #[test]
    fn phase02_insights_registry_threshold_and_recognizer_filter() {
        use super::*;
        let temp = Temp::new();
        let mut cfg = ConfigReader::isolated(temp.0.clone());
        cfg.values.insert("enabled".into(), serde_json::json!(true));
        let mut owner = Owner::new("agent:demo".into(), OwnerKind::AgentSession, "demo".into());
        owner.pids = vec![999001];
        owner.mem_bytes = 1024;
        let entry = Insight {
            id: "demo".into(),
            recognizer: "coding-agents".into(),
            trigger: Trigger::Memory { min_bytes: 1024 },
            cause: Localized {
                zh: "原因".into(),
                en: "Cause".into(),
            },
            advice: Localized {
                zh: "建议".into(),
                en: "Advice".into(),
            },
            source: "https://example.com/evidence".into(),
            checked: "2026-10-15".into(),
        };
        let mut catalog = Catalog::new(vec![entry]);
        assert_eq!(catalog.all().len(), 1);
        catalog.attach("dev-runtimes", &mut owner, &cfg).unwrap();
        assert!(owner.insights.is_empty());
        catalog.attach("coding-agents", &mut owner, &cfg).unwrap();
        assert_eq!(owner.insights, vec!["demo"]);
        catalog.entries[0].trigger = Trigger::Count {
            signal: "unknown".into(),
            min: 0,
        };
        assert_eq!(
            catalog.attach("coding-agents", &mut owner, &cfg),
            Err("unknown_count_signal")
        );
        assert!(all().is_empty());
    }
}
