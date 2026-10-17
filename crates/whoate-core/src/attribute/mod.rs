//! Ordered, total attribution. Recognizers never run until ownership is fixed.
use crate::{
    model::*,
    recognizers::ConfigReader,
    snapshot::{Proc, Snapshot},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

fn ancestors<'a>(proc: &'a Proc, index: &BTreeMap<i32, &'a Proc>) -> (Vec<&'a Proc>, bool) {
    // ceiling: one cycle-guarded walk per process for desktop snapshots; memoize
    // ancestry if measured tick cost grows beyond the one-second refresh budget.
    let mut seen = BTreeSet::new();
    let mut chain = vec![];
    let mut current = proc;
    loop {
        if !seen.insert(current.pid) {
            return (chain, false);
        }
        chain.push(current);
        if current.ppid == 1 {
            return (chain, true);
        }
        let Some(parent) = index.get(&current.ppid) else {
            return (chain, false);
        };
        current = parent;
    }
}
pub fn app(exe: &str, cfg: &ConfigReader) -> Result<Option<(String, String)>, &'static str> {
    let Some(end) = exe.find(".app/Contents/") else {
        return Ok(None);
    };
    let root = &exe[..end + 4];
    let value = cfg.read_plist(&Path::new(root).join("Contents/Info.plist"))?;
    let dict = value.as_dictionary().ok_or("app_metadata_invalid")?;
    let id = dict
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string)
        .ok_or("app_id_missing")?;
    let label = dict
        .get("CFBundleDisplayName")
        .or_else(|| dict.get("CFBundleName"))
        .and_then(plist::Value::as_string)
        .unwrap_or(id);
    Ok(Some((id.into(), label.into())))
}
pub fn is_runtime(proc: &Proc) -> bool {
    let name = proc
        .exe_path
        .as_deref()
        .and_then(|p| Path::new(p).file_name()?.to_str())
        .unwrap_or(&proc.comm);
    matches!(name, "node" | "bun" | "deno" | "python3")
        || name
            .strip_prefix("python3.")
            .is_some_and(|v| !v.is_empty() && v.bytes().all(|b| b.is_ascii_digit() || b == b'.'))
}

pub fn attribute(
    snap: &Snapshot,
    uid: u32,
    cfg: &ConfigReader,
    agents: &BTreeMap<i32, Agent>,
) -> Vec<Owner> {
    let index: BTreeMap<_, _> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    // Generic command errors may also come from top. Without a source-specific
    // signal we cannot prove that a missing pane has actually closed.
    let pane_table_known = !snap
        .issues
        .iter()
        .any(|issue| matches!(issue.stage.as_str(), "tmux" | "command" | "command_spawn"));
    let mut owners: BTreeMap<String, Owner> = BTreeMap::new();
    for proc in &snap.procs {
        let (chain, reaches_launchd) = ancestors(proc, &index);
        let root = *chain.last().unwrap_or(&proc);
        let pane = chain
            .iter()
            .find_map(|p| snap.tmux_panes.iter().find(|pane| pane.pane_pid == p.pid));
        // The pane pid itself may exec an agent. Prefer an actual session;
        // name-only subdivision is valid only inside a pane, at its outer root.
        let depth = pane
            .and_then(|pane| chain.iter().position(|p| p.pid == pane.pane_pid))
            .map_or(chain.len(), |i| i + 1);
        let candidates: Vec<_> = chain
            .iter()
            .take(depth)
            .filter_map(|p| {
                agents
                    .get(&p.pid)
                    .filter(|a| p.uid == uid || a.confidence == Confidence::Certain)
                    .map(|a| (*p, a))
            })
            .collect();
        let agent = candidates
            .iter()
            .find(|(_, a)| a.session_id.is_some())
            .copied()
            .or_else(|| pane.and_then(|_| candidates.last().copied()));
        let environment_pane = reaches_launchd
            .then(|| chain.iter().find_map(|p| p.tmux_pane.as_ref()))
            .flatten();
        let mut evidence = vec![];
        let mut origin = Origin {
            cwd: proc.cwd.clone(),
            ports: proc.listen_ports.clone(),
            ..Origin::default()
        };
        if let Some(pane) = pane {
            evidence.push(Evidence::new(
                "pane_ancestor",
                pane.pane_id.clone(),
                Confidence::Certain,
            ));
            origin.tmux_pane = Some(pane.pane_id.clone());
            origin.pane_alive = Some(true);
            origin.pane_title = pane.pane_title.clone();
            origin.window_name = pane.window_name.clone();
            if origin.cwd.is_none() {
                origin.cwd = pane.cwd.clone();
            }
        } else if let Some(pane_id) = environment_pane {
            let live = snap.tmux_panes.iter().any(|p| &p.pane_id == pane_id);
            origin.tmux_pane = Some(pane_id.clone());
            origin.pane_alive = if live {
                Some(true)
            } else {
                pane_table_known.then_some(false)
            };
            if origin.cwd.is_none() {
                origin.cwd = chain.iter().find_map(|p| p.cwd.clone());
            }
            if let Some(pane) = snap.tmux_panes.iter().find(|p| &p.pane_id == pane_id) {
                origin.pane_title = pane.pane_title.clone();
                origin.window_name = pane.window_name.clone();
            }
            evidence.push(Evidence::new(
                "tmux_environment",
                pane_id.clone(),
                if origin.pane_alive.is_some() {
                    Confidence::Certain
                } else {
                    Confidence::Weak
                },
            ));
        }
        let (id, kind, label, owner_agent) = if let Some((agent_proc, agent)) = agent {
            if origin.cwd.is_none() {
                origin.cwd = agent.cwd.clone();
            }
            let key = agent
                .session_id
                .clone()
                .unwrap_or_else(|| format!("pid:{}", agent_proc.pid));
            evidence.push(Evidence::new(
                "agent_source",
                agent.tool.clone(),
                agent.confidence,
            ));
            (
                format!("agent:{}:{key}", agent.tool),
                OwnerKind::AgentSession,
                agent
                    .name
                    .clone()
                    .or_else(|| agent.title.clone())
                    .unwrap_or_else(|| agent.tool.clone()),
                Some(agent.clone()),
            )
        } else if let Some(pane) = pane {
            (
                format!("pane:{}", pane.pane_id),
                OwnerKind::TmuxPane,
                pane.target.clone(),
                None,
            )
        } else if let Some(pane_id) = environment_pane {
            let live = snap.tmux_panes.iter().find(|p| &p.pane_id == pane_id);
            if let Some(pane) = live {
                (
                    format!("pane:{pane_id}"),
                    OwnerKind::TmuxPane,
                    pane.target.clone(),
                    None,
                )
            } else if pane_table_known {
                (
                    format!("orphan:{}", root.pid),
                    OwnerKind::Orphan,
                    root.comm.clone(),
                    None,
                )
            } else {
                evidence.push(Evidence::new(
                    "pane_status_unknown",
                    "pane capture unavailable",
                    Confidence::Weak,
                ));
                (
                    format!("service:{}", root.pid),
                    OwnerKind::Service,
                    root.comm.clone(),
                    None,
                )
            }
        } else if reaches_launchd
            && proc.uid == uid
            && is_runtime(proc)
            && proc
                .cwd
                .as_deref()
                .is_some_and(|cwd| Path::new(cwd).starts_with(&cfg.home))
        {
            evidence.push(Evidence::new(
                "runtime_home",
                "runtime in HOME; launchd ancestry",
                Confidence::Weak,
            ));
            (
                format!("orphan:{}", root.pid),
                OwnerKind::Orphan,
                root.comm.clone(),
                None,
            )
        } else {
            let mut found = None;
            for p in &chain {
                if let Some(exe) = &p.exe_path {
                    match app(exe, cfg) {
                        Ok(Some(a)) => {
                            evidence.push(Evidence::new(
                                "app_bundle",
                                a.0.clone(),
                                Confidence::Certain,
                            ));
                            found = Some(a);
                            break;
                        }
                        Err(code) => {
                            evidence.push(Evidence::new("app_metadata", code, Confidence::Weak))
                        }
                        Ok(None) => {}
                    }
                }
            }
            if found.is_none()
                && reaches_launchd
                && let Some(responsible) = root
                    .responsible_pid
                    .filter(|pid| *pid > 1)
                    .and_then(|pid| index.get(&pid))
            {
                match responsible.exe_path.as_deref().map(|exe| app(exe, cfg)) {
                    Some(Ok(Some(a))) => {
                        evidence.push(Evidence::new(
                            "responsible_app",
                            a.0.clone(),
                            Confidence::Certain,
                        ));
                        found = Some(a);
                    }
                    _ => evidence.push(Evidence::new(
                        "responsible_unavailable",
                        "no readable responsible app",
                        Confidence::Weak,
                    )),
                }
            }
            if let Some((id, label)) = found {
                (format!("app:{id}"), OwnerKind::App, label, None)
            } else if proc.uid != uid {
                evidence.push(Evidence::new(
                    "other_uid",
                    proc.uid.to_string(),
                    Confidence::Certain,
                ));
                (
                    format!("system:{}", root.pid),
                    OwnerKind::System,
                    root.comm.clone(),
                    None,
                )
            } else {
                evidence.push(Evidence::new(
                    "service_fallback",
                    if reaches_launchd {
                        "launchd ancestry"
                    } else {
                        "missing or cyclic ancestry"
                    },
                    Confidence::Weak,
                ));
                // Broken chains use the individual pid, never traversal order as a group key.
                let key = if reaches_launchd { root.pid } else { proc.pid };
                (
                    format!("service:{key}"),
                    OwnerKind::Service,
                    root.comm.clone(),
                    None,
                )
            }
        };
        let owner = owners.entry(id.clone()).or_insert_with(|| {
            let mut o = Owner::new(id, kind, label);
            o.agent = owner_agent;
            o.origin = Some(Origin::default());
            o
        });
        owner.pids.push(proc.pid);
        if let Some(mem) = proc.footprint_bytes {
            owner.mem_bytes += mem;
        } else {
            owner.unknown_mem_count += 1;
        }
        if let Some(cpu) = proc.cpu_pct_sampled {
            owner.cpu_pct = Some(owner.cpu_pct.unwrap_or(0.0) + cpu);
        }
        let target = owner.origin.as_mut().unwrap();
        if target.tmux_pane.is_none() {
            target.tmux_pane = origin.tmux_pane;
            target.pane_alive = origin.pane_alive;
            target.pane_title = origin.pane_title;
            target.window_name = origin.window_name;
        }
        // Deterministic context choice when several children provide cwd values.
        if let Some(cwd) = origin.cwd
            && target.cwd.as_ref().is_none_or(|old| &cwd < old)
        {
            target.cwd = Some(cwd);
        }
        if let Some(ports) = origin.ports {
            let all = target.ports.get_or_insert_with(Vec::new);
            all.extend(ports);
            all.sort_unstable();
            all.dedup();
        }
        for item in evidence {
            if !owner.evidence.contains(&item) {
                owner.evidence.push(item);
            }
        }
    }
    let mut output: Vec<_> = owners.into_values().collect();
    for owner in &mut output {
        owner.reclaimable_physical = Some(PhysicalEstimate::from_procs(
            owner.pids.iter().filter_map(|pid| index.get(pid).copied()),
        ));
        owner.pids.sort_unstable();
        owner.evidence.sort_by_key(|e| {
            (
                match e.kind.as_str() {
                    "pane_ancestor" => 0,
                    "agent_source" => 1,
                    "tmux_environment" => 2,
                    "runtime_home" => 3,
                    "app_bundle" => 4,
                    "responsible_app" => 5,
                    _ => 6,
                },
                e.kind.clone(),
                e.detail.clone(),
            )
        });
    }
    output
}

#[cfg(test)]
mod acceptance;
#[cfg(test)]
mod holdout;

#[cfg(test)]
mod tests {
    use crate::{model::*, recognizers::ConfigReader, snapshot::*, sources::testing::Temp};
    use std::collections::BTreeMap;
    #[test]
    fn phase02_attribution_edges_closed_pane_is_certain_orphan_and_unique() {
        let temp = Temp::new();
        let cfg = ConfigReader::isolated(temp.0.clone());
        let mut root = Proc::identity(999001, 1, 501, Some(100), "node".into());
        root.tmux_pane = Some("%closed".into());
        root.footprint_bytes = Some(1024);
        root.cwd = Some(temp.0.to_string_lossy().into());
        root.listen_ports = Some(vec![8799]);
        let child = Proc::identity(999002, 999001, 501, Some(200), "node".into());
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![child, root]);
        let owners = super::attribute(&snap, 501, &cfg, &BTreeMap::new());
        assert_eq!(owners.len(), 1);
        assert_eq!(owners[0].id, "orphan:999001");
        assert_eq!(owners[0].kind, OwnerKind::Orphan);
        assert_eq!(owners[0].pids, vec![999001, 999002]);
        assert_eq!(owners[0].mem_bytes, 1024);
        assert_eq!(owners[0].unknown_mem_count, 1);
        assert_eq!(owners[0].origin.as_ref().unwrap().pane_alive, Some(false));
        assert_eq!(owners[0].origin.as_ref().unwrap().ports, Some(vec![8799]));
        assert_eq!(owners[0].evidence[0].confidence, Confidence::Certain);
    }
}
