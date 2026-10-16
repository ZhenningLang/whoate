pub mod claude_code;
pub mod kilo;
pub mod opencode;

#[cfg(test)]
mod phase02_tests;

use crate::{
    model::{Agent, Confidence},
    recognizers::ConfigReader,
    snapshot::{Proc, SampleIssue},
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SessionKey {
    tool: String,
    pid: i32,
    uid: u32,
    start: u64,
    cwd: Option<String>,
    home: std::path::PathBuf,
    session_pid: Option<i32>,
}

pub fn tool(proc: &Proc) -> Option<&'static str> {
    let names = [
        Some(proc.comm.as_str()),
        proc.exe_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name()?.to_str()),
        proc.argv
            .first()
            .and_then(|p| std::path::Path::new(p).file_name()?.to_str()),
    ];
    for name in names.into_iter().flatten() {
        match name {
            "claude" => return Some("claude"),
            "opencode" | "opencode.exe" => return Some("opencode"),
            "kilo" | "kilo.exe" => return Some("kilo"),
            _ => {}
        }
        let version: Vec<_> = name.split('.').collect();
        if version.len() == 3
            && version
                .iter()
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
        {
            return Some("claude");
        }
    }
    if matches!(proc.comm.as_str(), "node" | "bun" | "deno")
        && let Some(entry) = proc
            .argv
            .get(1)
            .and_then(|p| std::path::Path::new(p).file_name()?.to_str())
    {
        match entry.strip_suffix(".js").unwrap_or(entry) {
            "opencode" => return Some("opencode"),
            "kilo" => return Some("kilo"),
            "claude" => return Some("claude"),
            _ => {}
        }
    }
    None
}

#[derive(Default)]
pub struct Sessions {
    cache: BTreeMap<SessionKey, Agent>,
}
impl Sessions {
    pub fn collect(
        &mut self,
        procs: &[Proc],
        cfg: &ConfigReader,
    ) -> (BTreeMap<i32, Agent>, Vec<SampleIssue>) {
        let mut agents = BTreeMap::new();
        let mut issues = vec![];
        let mut active = BTreeSet::new();
        for proc in procs {
            // Capability check precedes every source open, including SQLite.
            if cfg.session_pid().is_some_and(|pid| proc.pid != pid) {
                continue;
            }
            let Some(tool) = tool(proc) else {
                continue;
            };
            // The limited transcript reader is Claude-specific. Never silently
            // fall back to an unrestricted database reader for the same PID.
            if cfg.session_pid().is_some() && tool != "claude" {
                continue;
            }
            let key = proc.start_time.map(|start| SessionKey {
                tool: tool.to_owned(),
                pid: proc.pid,
                uid: proc.uid,
                start,
                cwd: proc.cwd.clone(),
                home: cfg.home.clone(),
                session_pid: cfg.session_pid(),
            });
            if let Some(key) = &key {
                active.insert(key.clone());
            }
            let result = match tool {
                "claude" => claude_code::read(proc, cfg).map(|read| {
                    for code in read.issues {
                        issues.push(SampleIssue {
                            stage: "claude".into(),
                            pid: Some(proc.pid),
                            code: code.into(),
                        });
                    }
                    Some(read.agent)
                }),
                "opencode" => opencode::read(
                    &cfg.home.join(".local/share/opencode/opencode.db"),
                    proc,
                    tool,
                ),
                "kilo" => kilo::read(&cfg.home.join(".local/share/kilo/kilo.db"), proc),
                _ => unreachable!(),
            };
            let agent = match result {
                Ok(Some(agent)) => {
                    if let Some(key) = key {
                        self.cache.insert(key, agent.clone());
                    }
                    agent
                }
                Ok(None) => {
                    if let Some(key) = &key {
                        self.cache.remove(key);
                    }
                    let mut a = Agent::unknown(tool);
                    a.cwd = proc.cwd.clone();
                    a
                }
                Err(code) => {
                    issues.push(SampleIssue {
                        stage: tool.into(),
                        pid: Some(proc.pid),
                        code: code.into(),
                    });
                    let mut a = key
                        .as_ref()
                        .and_then(|k| self.cache.get(k))
                        .cloned()
                        .unwrap_or_else(|| Agent::unknown(tool));
                    a.stale = true;
                    a.confidence = Confidence::Weak;
                    a
                }
            };
            agents.insert(proc.pid, agent);
        }
        self.cache.retain(|key, _| active.contains(key));
        (agents, issues)
    }
}

#[cfg(test)]
#[path = "../../tests/common/mod.rs"]
mod capture_test_helpers;

#[cfg(test)]
pub(crate) mod testing {
    pub use super::capture_test_helpers::Temp;
    use std::path::PathBuf;
    impl Temp {
        pub fn write(&self, path: &str, text: impl AsRef<[u8]>) -> PathBuf {
            let path = self.0.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, text).unwrap();
            path
        }
    }
}
