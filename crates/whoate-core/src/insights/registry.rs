//! Explicit, location-independent library and signal contract.
use super::{Insight, Trigger};
use serde::Deserialize;
use std::{collections::BTreeSet, sync::OnceLock};

pub const SOURCES: &[(&str, &str)] = &[
    ("agents.toml", include_str!("agents.toml")),
    ("browsers.toml", include_str!("browsers.toml")),
    ("desktop.toml", include_str!("desktop.toml")),
    ("runtimes.toml", include_str!("runtimes.toml")),
    ("system.toml", include_str!("system.toml")),
];

/// Producers: recognizers::registry::prepare (software counts), Chromium and
/// Docker configure (preferences), and CodingAgents (MCP subtree membership).
pub const COUNT_SIGNALS: &[&str] = &[
    "process_count",
    "mcp_count",
    "software_claude",
    "software_opencode",
    "software_vite",
    "software_next",
    "software_webpack",
    "software_wrangler",
    "software_tsserver",
    "software_python",
    "software_chrome",
    "software_brave",
    "software_vscode",
    "software_jetbrains",
    "software_docker",
    "python_kernel",
    "chrome_extensions",
    "brave_extensions",
    "vscode_extension_hosts",
    "vscode_watchers",
];
pub const CONFIG_SIGNALS: &[&str] = &[
    "chrome.memory_saver_state",
    "brave.memory_saver_state",
    "docker.memory_default",
    "docker.resource_saver_disabled",
];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    entries: Vec<Insight>,
    #[serde(default)]
    kernel_zones: Option<super::system_policy::ZonePolicy>,
}

pub fn load() -> Result<Vec<Insight>, &'static str> {
    static BUILTIN: OnceLock<Result<Vec<Insight>, &'static str>> = OnceLock::new();
    BUILTIN.get_or_init(|| load_sources(SOURCES)).clone()
}

/// Shared by full-library validation and the fallible Engine constructor.
/// Never skip an invalid file or return a partial library.
pub fn load_sources(sources: &[(&str, &str)]) -> Result<Vec<Insight>, &'static str> {
    let mut entries = Vec::new();
    let mut ids = BTreeSet::new();
    for (_, text) in sources {
        let document: Document = toml::from_str(text).map_err(|_| "invalid_insight_toml")?;
        if let Some(policy) = &document.kernel_zones {
            policy.validate()?;
        }
        for entry in document.entries {
            if entry.id.trim().is_empty() || !ids.insert(entry.id.clone()) {
                return Err("invalid_insight_id");
            }
            if !matches!(
                entry.recognizer.as_str(),
                "coding-agents"
                    | "dev-runtimes"
                    | "chromium"
                    | "webkit-host"
                    | "docker"
                    | "ide"
                    | "system-services"
            ) {
                return Err("unknown_insight_recognizer");
            }
            if !super::system_policy::valid_source(&entry)
                || [
                    &entry.cause.zh,
                    &entry.cause.en,
                    &entry.advice.zh,
                    &entry.advice.en,
                ]
                .iter()
                .any(|s| s.trim().is_empty())
                || time::Date::parse(
                    &entry.checked,
                    &time::format_description::well_known::Iso8601::DATE,
                )
                .is_err()
            {
                return Err("invalid_insight_metadata");
            }
            super::system_policy::validate(&entry)?;
            let mut pending = vec![&entry.trigger];
            while let Some(trigger) = pending.pop() {
                match trigger {
                    Trigger::Count { signal, .. } if !COUNT_SIGNALS.contains(&signal.as_str()) => {
                        return Err("unknown_count_signal");
                    }
                    Trigger::Config { key, .. } if !CONFIG_SIGNALS.contains(&key.as_str()) => {
                        return Err("unknown_config_signal");
                    }
                    Trigger::All { triggers } => {
                        if triggers.is_empty() {
                            return Err("empty_insight_trigger");
                        }
                        pending.extend(triggers);
                    }
                    _ => {}
                }
            }
            entries.push(entry);
        }
    }
    if entries.is_empty() {
        return Err("empty_insight_catalog");
    }
    Ok(entries)
}
