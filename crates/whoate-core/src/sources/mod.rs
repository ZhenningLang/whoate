mod cache;
pub mod claude_code;
mod codex;
pub mod kilo;
pub mod opencode;

pub fn idle_seconds(agent: &crate::model::Agent, snap: &crate::snapshot::Snapshot) -> Option<u64> {
    if agent.stale {
        return None;
    }
    let at = time::OffsetDateTime::parse(
        agent.last_activity_at.as_deref()?,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()?;
    let at = u64::try_from(at.unix_timestamp_nanos() / 1_000).ok()?;
    crate::swarm::timestamp_us(snap)?
        .checked_sub(at)
        .map(|us| us / 1_000_000)
}

#[cfg(test)]
mod phase02_tests;

use crate::{
    model::{Agent, Confidence},
    recognizers::ConfigReader,
    snapshot::{Proc, SampleIssue},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::MetadataExt,
};

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
struct SessionKey {
    tool: String,
    pid: i32,
    uid: u32,
    start: u64,
    cwd: Option<String>,
    home: std::path::PathBuf,
    home_identity: Option<(std::path::PathBuf, u64, u64)>,
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

/// A process whose session the sources look up: an agent CLI, or a Codex
/// tool call (CODEX_THREAD_ID, phase 10 R7).
pub fn has_session_source(proc: &Proc) -> bool {
    tool(proc).is_some() || proc.codex_thread_id.is_some()
}

#[derive(Default)]
pub struct Sessions {
    cache: BTreeMap<SessionKey, Agent>,
    source_reads: usize,
    memo: BTreeMap<SessionKey, cache::Memo>,
    databases: opencode::Cached,
    codex: codex::Cached,
}
impl Sessions {
    /// Successful memo hits do not increment this source-parser counter.
    pub fn source_reads(&self) -> usize {
        self.source_reads
    }
    pub fn collect(
        &mut self,
        procs: &[Proc],
        cfg: &ConfigReader,
    ) -> (BTreeMap<i32, Agent>, Vec<SampleIssue>) {
        let mut agents = BTreeMap::new();
        let mut issues = vec![];
        let mut active = BTreeSet::new();
        let mut codex = BTreeMap::new();
        // A symlink/root replacement must not authorize a stale value from the
        // previous physical home, even if its configured spelling is unchanged.
        let home_identity = cfg.home.canonicalize().ok().and_then(|path| {
            std::fs::metadata(&path)
                .ok()
                .map(|meta| (path, meta.dev(), meta.ino()))
        });
        for proc in procs {
            // Capability check precedes every source open, including SQLite.
            if cfg.session_pid().is_some_and(|pid| proc.pid != pid) {
                continue;
            }
            let Some(tool) = tool(proc) else {
                // Codex ids are looked up together after the loop; the
                // single-session capability never reads them.
                if let Some(id) = &proc.codex_thread_id
                    && cfg.session_pid().is_none()
                {
                    codex.insert(proc.pid, id.clone());
                }
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
                home_identity: home_identity.clone(),
                session_pid: cfg.session_pid(),
            });
            if let Some(key) = &key {
                active.insert(key.clone());
            }
            if let Some(memo) = key.as_ref().and_then(|k| self.memo.get(k))
                && memo.sources.iter().all(|stamp| stamp.unchanged(cfg))
            {
                agents.insert(proc.pid, memo.agent.clone());
                continue;
            }
            if let Some(key) = &key {
                self.memo.remove(key);
            }
            let result = match tool {
                "claude" => {
                    self.source_reads += 1;
                    claude_code::read(proc, cfg).map(|read| {
                        if let Some(key) = &key
                            && let Some(sources) = read.sources
                        {
                            self.memo.insert(
                                key.clone(),
                                cache::Memo {
                                    agent: read.agent.clone(),
                                    sources,
                                },
                            );
                        }
                        for code in read.issues {
                            issues.push(SampleIssue {
                                stage: "claude".into(),
                                pid: Some(proc.pid),
                                code: code.into(),
                            });
                        }
                        Some(read.agent)
                    })
                }
                "opencode" | "kilo" => {
                    let path = cfg.home.join(format!(".local/share/{tool}/{tool}.db"));
                    if !cfg.allows(&path) {
                        Err("outside_fixture_root")
                    } else {
                        self.databases
                            .read(&path, proc, tool, key.as_ref(), &mut self.source_reads)
                    }
                }
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
        let (threads, codes) = self.codex.lookup(&codex.values().cloned().collect(), cfg);
        for (pid, id) in codex {
            if let Some(agent) = threads.get(&id) {
                agents.insert(pid, agent.clone());
            }
        }
        issues.extend(codes.into_iter().map(|code| SampleIssue {
            stage: "codex".into(),
            pid: None,
            code: code.into(),
        }));
        self.cache.retain(|key, _| active.contains(key));
        self.memo.retain(|key, _| active.contains(key));
        self.databases.retain(&active);
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
