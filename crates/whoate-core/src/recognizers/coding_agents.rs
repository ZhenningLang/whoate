use super::{
    ConfigReader, Recognizer,
    dev_runtimes::{fact, parts},
};
use crate::{
    model::{FactValue, Owner},
    snapshot::{Proc, Snapshot},
    sources,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct CodingAgents;

/// A package entry, not a substring in a shell's command text.
pub fn mcp_name(proc: &Proc) -> Option<String> {
    let executable = proc
        .exe_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name()?.to_str())
        .unwrap_or(&proc.comm);
    let args = &proc.argv;
    if executable == "uvx" {
        return (args.get(1)?.as_str() == "mcp-server-fetch").then(|| "mcp-server-fetch".into());
    }
    let python = matches!(executable, "python" | "python3")
        || executable
            .strip_prefix("python3.")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.'));
    if python {
        return (args.get(1)?.as_str() == "-m" && args.get(2)?.as_str() == "mcp_server_fetch")
            .then(|| "mcp-server-fetch".into());
    }
    if executable == "docker" {
        if args.get(1)?.as_str() != "run" {
            return None;
        }
        // Only the documented stdio forms. Unknown options may consume an image-
        // shaped value, so never skip arbitrary flags while searching for an image.
        let image = args
            .iter()
            .skip(2)
            .find(|a| !matches!(a.as_str(), "-i" | "--interactive" | "--rm" | "-t" | "--tty"))?;
        return matches!(image.as_str(), "mcp/fetch" | "mcp/filesystem").then(|| image.clone());
    }
    if !matches!(executable, "node" | "bun" | "deno" | "npm" | "npx") {
        return None;
    }
    let name = crate::swarm::logical_service(proc).or_else(|| {
        if !matches!(executable, "node" | "bun" | "deno") {
            return None;
        }
        Path::new(args.get(1)?)
            .file_name()?
            .to_str()
            .map(|s| s.trim_end_matches(".js").to_owned())
    })?;
    (name.ends_with("-mcp")
        || name.starts_with("mcp-")
        || name == "mcp_server"
        || name.starts_with("@modelcontextprotocol/server-"))
    .then_some(name)
}

/// Explicit MCP subtrees outrank software-specific and generic fallback parts.
/// Paths stop at the owner's boundary; unrelated parents cannot contribute.
pub(crate) fn mcp_members(owner: &Owner, snap: &Snapshot) -> BTreeMap<i32, String> {
    if owner.agent.is_none() {
        return BTreeMap::new();
    }
    let members: BTreeSet<_> = owner.pids.iter().copied().collect();
    let index: BTreeMap<_, _> = snap
        .procs
        .iter()
        .filter(|p| members.contains(&p.pid))
        .map(|p| (p.pid, p))
        .collect();
    let mut result = BTreeMap::new();
    // ceiling: walk each owner's finite ancestry; memoize if deep process trees
    // make this measurable, without extending traversal outside owner membership.
    for proc in index.values() {
        let mut current = *proc;
        let mut seen = BTreeSet::new();
        while seen.insert(current.pid) {
            if let Some(name) = mcp_name(current) {
                result.insert(proc.pid, name);
                break;
            }
            let Some(parent) = index.get(&current.ppid) else {
                break;
            };
            current = parent;
        }
    }
    result
}
impl Recognizer for CodingAgents {
    fn id(&self) -> &'static str {
        "coding-agents"
    }
    fn matches(&self, proc: &Proc) -> bool {
        sources::tool(proc).is_some()
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        if owner.agent.is_none() {
            return;
        }
        let members: BTreeSet<_> = owner.pids.iter().copied().collect();
        let mcp = mcp_members(owner, snap);
        let mut groups: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        for proc in snap.procs.iter().filter(|p| members.contains(&p.pid)) {
            let classification = mcp.get(&proc.pid).map(|name| format!("MCP: {name}"));
            let label = classification.unwrap_or_else(|| {
                if self.matches(proc) && members.contains(&proc.ppid) {
                    format!(
                        "{}: {}",
                        cfg.language.choose("子助手", "Sub-agent"),
                        sources::tool(proc).unwrap_or(&proc.comm)
                    )
                } else {
                    cfg.language.choose("会话", "Session").into()
                }
            });
            groups.entry(label).or_default().push(proc);
        }
        parts(owner, groups);
        if let Some(a) = owner.agent.clone() {
            fact(
                owner,
                "agent_tool",
                cfg.language.choose("编程助手", "Coding agent"),
                FactValue::Text(a.tool),
            );
            if let Some(status) = a.status {
                fact(
                    owner,
                    "agent_status",
                    cfg.language.choose("状态", "Status"),
                    FactValue::Text(status),
                );
            }
            if let Some(branch) = a.branch {
                fact(
                    owner,
                    "branch",
                    cfg.language.choose("分支", "Branch"),
                    FactValue::Text(branch),
                );
            }
            if a.stale {
                fact(
                    owner,
                    "source_stale",
                    cfg.language.choose("会话信息过期", "Stale session data"),
                    FactValue::Bool(true),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::*,
        recognizers::{ConfigReader, Recognizer},
        snapshot::*,
        sources::testing::Temp,
    };
    #[test]
    fn phase02_recognizers_direct_bin_mcp_counts_its_descendants() {
        let t = Temp::new();
        let root = Proc::identity(999001, 1, 501, Some(1), "claude".into());
        let mut mcp = Proc::identity(999002, 999001, 501, Some(2), "node".into());
        mcp.argv = vec!["node".into(), "/demo/node_modules/.bin/example-mcp".into()];
        let child = Proc::identity(999003, 999002, 501, Some(3), "worker".into());
        let mut ordinary = Proc::identity(999004, 999001, 501, Some(4), "node".into());
        ordinary.argv = vec!["node".into(), "/demo/node_modules/.bin/vite".into()];
        let snap = Snapshot::historical(
            "2026-10-14T19:40:00+08:00".into(),
            vec![root, mcp, child, ordinary],
        );
        let mut owner = Owner::new(
            "agent:claude:example".into(),
            OwnerKind::AgentSession,
            "Example".into(),
        );
        owner.agent = Some(Agent::unknown("claude"));
        owner.pids = vec![999001, 999002, 999003, 999004];
        let cfg = ConfigReader::isolated(t.0.clone());
        super::CodingAgents.explain(&mut owner, &snap, &cfg);
        assert_eq!(
            owner
                .parts
                .iter()
                .find(|p| p.name == "MCP: example-mcp")
                .map(|p| p.pids.clone()),
            Some(vec![999002, 999003])
        );
        assert_eq!(
            crate::insights::evaluate(
                &crate::insights::Trigger::Count {
                    signal: "mcp_count".into(),
                    min: 2
                },
                &owner,
                &cfg
            ),
            Ok(true)
        );
        assert_eq!(super::mcp_name(&snap.procs[3]), None);
    }
    #[test]
    fn phase02_recognizers_agent_mcp_parts_do_not_move_ownership() {
        let temp = Temp::new();
        let mut p = Proc::identity(999001, 1, 501, Some(1), "claude".into());
        p.footprint_bytes = Some(100);
        let mut mcp = Proc::identity(999002, 999001, 501, Some(2), "node".into());
        mcp.footprint_bytes = Some(40);
        mcp.argv = vec![
            "node".into(),
            "/demo/node_modules/example-mcp/index.js".into(),
        ];
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p, mcp]);
        let mut o = Owner::new(
            "agent:claude:demo".into(),
            OwnerKind::AgentSession,
            "Demo".into(),
        );
        o.pids = vec![999001, 999002];
        o.mem_bytes = 140;
        o.agent = Some(Agent::unknown("claude"));
        let before = o.clone();
        super::CodingAgents.explain(&mut o, &snap, &ConfigReader::isolated(temp.0.clone()));
        assert_eq!(o.id, before.id);
        assert_eq!(o.kind, before.kind);
        assert_eq!(o.pids, before.pids);
        assert_eq!(o.mem_bytes, before.mem_bytes);
        assert_eq!(o.agent, before.agent);
        assert_eq!(
            o.parts
                .iter()
                .find(|p| p.name == "MCP: example-mcp")
                .unwrap()
                .mem_bytes,
            40
        );
        assert_eq!(
            o.parts
                .iter()
                .find(|p| p.name == "MCP: example-mcp")
                .unwrap()
                .process_count,
            1
        );
        assert!(o.insights.is_empty());
    }
}
