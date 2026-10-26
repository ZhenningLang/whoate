//! Ordered, total attribution. Recognizers never run until ownership is fixed.
use crate::{
    model::*,
    recognizers::ConfigReader,
    snapshot::{Proc, Snapshot},
};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

mod codex;
mod terminals;

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
/// The outermost bundle in an executable path: `X.app`, or `X.app.bundle`
/// (Chrome's code-sign clone runs from one; owner ruling D3).
fn bundle_root(exe: &str) -> Option<&str> {
    [".app/Contents/", ".app.bundle/Contents/"]
        .iter()
        .filter_map(|marker| {
            exe.find(marker)
                .map(|i| &exe[..i + marker.len() - "/Contents/".len()])
        })
        .min_by_key(|root| root.len())
}
pub fn app(exe: &str, cfg: &ConfigReader) -> Result<Option<(String, String)>, &'static str> {
    let Some(root) = bundle_root(exe) else {
        return Ok(None);
    };
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

/// A detached `git fsmonitor--daemon` (R17, owner 2026-10-25): git starts it
/// for a repository with `core.fsmonitor` and detaches it from the terminal on
/// purpose, so a closed pane in its environment says nothing about it.
pub fn git_fsmonitor(argv: &[String]) -> bool {
    argv.first()
        .is_some_and(|exe| exe.rsplit('/').next() == Some("git"))
        && argv.get(1).is_some_and(|a| a == "fsmonitor--daemon")
        && argv.iter().any(|a| a == "--detach")
}

/// The git dir a daemon serves: from its `<git dir>/fsmonitor--daemon.ipc`
/// socket, else from its open directories: `<repo>/.git`, or a worktree's
/// `<repo>/.git/worktrees/<name>` (preferred: it names the worktree).
pub fn git_dir_of(paths: &[String]) -> Option<String> {
    let is_git_dir = |d: &str| {
        git_repository(d).is_some_and(|repo| {
            let tail = &d[repo.len()..];
            tail == "/.git"
                || tail
                    .strip_prefix("/.git/worktrees/")
                    .is_some_and(|name| !name.is_empty() && !name.contains('/'))
        })
    };
    if let Some(dir) = paths
        .iter()
        .filter_map(|p| p.strip_suffix("/fsmonitor--daemon.ipc"))
        .find(|d| is_git_dir(d))
    {
        return Some(dir.into());
    }
    paths
        .iter()
        .filter(|d| is_git_dir(d))
        .max_by_key(|d| d.len())
        .cloned()
}

/// An opaque, stable key for a repository path (FNV-1a, 64 bit). Owner ids stay
/// as they are under `--redact`, so a path never goes into one; the path itself
/// is the owner's `origin.cwd`, which `--redact` hides (round 7 review B1).
pub fn repository_key(path: &str) -> String {
    let hash = path.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// The repository a git dir belongs to: the path before its `/.git` component.
pub fn git_repository(git_dir: &str) -> Option<&str> {
    git_dir
        .match_indices("/.git")
        .find(|(i, m)| matches!(git_dir.as_bytes().get(i + m.len()), None | Some(b'/')))
        .map(|(i, _)| &git_dir[..i])
        .filter(|repo| !repo.is_empty())
}

/// Session display name. A name the tool derived itself (Claude Code's
/// `7-74`) says less than the session title; a name the user chose wins.
pub fn agent_label(agent: &Agent) -> String {
    if agent.name_source.as_deref() == Some("derived")
        && let Some(title) = agent.title.as_deref().filter(|t| !t.trim().is_empty())
    {
        return title.to_string();
    }
    agent
        .name
        .clone()
        .or_else(|| agent.title.clone())
        .unwrap_or_else(|| agent.tool.clone())
}

fn agent_owner(
    agent_proc: &Proc,
    agent: &Agent,
    origin: &mut Origin,
    evidence: &mut Vec<Evidence>,
) -> (String, OwnerKind, String, Option<Agent>) {
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
    if let Some(thread) = agent_proc
        .codex_thread_id
        .as_ref()
        .filter(|_| agent.tool == "codex")
    {
        let detail = match &agent.session_id {
            Some(root) if root != thread => format!("{thread} (subagent of {root})"),
            _ => thread.clone(),
        };
        evidence.push(Evidence::new(
            "codex_thread_env",
            detail,
            Confidence::Certain,
        ));
    }
    (
        format!("agent:{}:{key}", agent.tool),
        OwnerKind::AgentSession,
        agent_label(agent),
        Some(agent.clone()),
    )
}

/// A process outside any pane and without an agent on its parent chain.
struct Chain<'a> {
    proc: &'a Proc,
    chain: &'a [&'a Proc],
    root: &'a Proc,
    reaches_launchd: bool,
    uid: u32,
}
enum Unowned<'a> {
    Agent(&'a Proc, &'a Agent),
    Orphan,
    App(String, String),
    Other,
}

/// Owner ruling D1 = C, rules ② to ④, then U-BEH-3 and the fallbacks:
/// ② an app on the chain owns it when the chain's root is an app itself;
/// ③ otherwise launchd adopted the root, and the root's responsible process,
///   a running app or agent that is not a terminal, launched it;
/// ④ a terminal as the responsible process is handled by the orphan rule:
///   a runtime working in HOME, with no app or agent on its chain, is an
///   orphan, and the terminal is recorded as where it came from. Anything
///   else a terminal is responsible for (its XPC services, daemons it
///   started) stays with the terminal app (U-BEH-3), as before Phase 10.
/// Anything left: the chain's app, else system or service.
fn unowned<'a>(
    c: &Chain<'a>,
    cfg: &ConfigReader,
    agents: &'a BTreeMap<i32, Agent>,
    index: &BTreeMap<i32, &'a Proc>,
    evidence: &mut Vec<Evidence>,
) -> Unowned<'a> {
    let mut found = None;
    for p in c.chain {
        if let Some(exe) = &p.exe_path {
            match app(exe, cfg) {
                Ok(Some(a)) => {
                    found = Some(a);
                    break;
                }
                Err(code) => evidence.push(Evidence::new("app_metadata", code, Confidence::Weak)),
                Ok(None) => {}
            }
        }
    }
    let in_bundle = |p: &Proc| {
        p.exe_path
            .as_deref()
            .is_some_and(|exe| bundle_root(exe).is_some())
    };
    if in_bundle(c.root)
        && let Some(app) = found.take()
    {
        return bundle_owns(app, evidence);
    }
    let launcher = c
        .root
        .responsible_pid
        .filter(|pid| c.reaches_launchd && *pid > 1)
        .and_then(|pid| index.get(&pid).copied());
    let launcher_app = launcher
        .and_then(|r| r.exe_path.as_deref())
        .and_then(|exe| app(exe, cfg).ok().flatten());
    let terminal = launcher_app
        .as_ref()
        .is_some_and(|(id, _)| terminals::is_terminal(id));
    if !terminal {
        if let Some(r) = launcher.filter(|r| r.pid != c.root.pid)
            && let Some(agent) = agents
                .get(&r.pid)
                .filter(|a| r.uid == c.uid || a.confidence == Confidence::Certain)
        {
            evidence.push(Evidence::new(
                "responsible_launcher",
                format!("{} pid {}", agent.tool, r.pid),
                Confidence::Inferred,
            ));
            return Unowned::Agent(r, agent);
        }
        if let Some((id, label)) = launcher_app.clone() {
            // A system service the app asked for (XPC, /System) is the app's own
            // (U-BEH-3, certain). Anything else the app started is ③: inferred.
            evidence.push(if found.is_none() && system_service(c.root) {
                Evidence::new("responsible_app", id.clone(), Confidence::Certain)
            } else {
                Evidence::new("responsible_launcher", id.clone(), Confidence::Inferred)
            });
            return Unowned::App(id, label);
        }
    }
    let identified = c
        .chain
        .iter()
        .any(|p| in_bundle(p) || agents.contains_key(&p.pid));
    let runtime_home = is_runtime(c.proc)
        && c.proc
            .cwd
            .as_deref()
            .is_some_and(|cwd| Path::new(cwd).starts_with(&cfg.home));
    if c.reaches_launchd && c.proc.uid == c.uid && !identified && runtime_home {
        if let Some((id, _)) = launcher_app.as_ref().filter(|_| terminal) {
            evidence.push(Evidence::new(
                "responsible_terminal",
                id.clone(),
                Confidence::Inferred,
            ));
        }
        evidence.push(Evidence::new(
            "runtime_home",
            "runtime in HOME; launchd ancestry",
            Confidence::Weak,
        ));
        return Unowned::Orphan;
    }
    if let Some(app) = found {
        return bundle_owns(app, evidence);
    }
    if let Some((id, label)) = launcher_app {
        // A terminal's own services and daemons: U-BEH-3.
        evidence.push(Evidence::new(
            "responsible_app",
            id.clone(),
            Confidence::Certain,
        ));
        return Unowned::App(id, label);
    }
    if launcher.is_some() {
        evidence.push(Evidence::new(
            "responsible_unavailable",
            "no readable responsible app",
            Confidence::Weak,
        ));
    }
    Unowned::Other
}

/// XPC services and system binaries run on behalf of the app responsible for
/// them; an unknown executable path is treated the same (the U-BEH-3 default).
fn system_service(p: &Proc) -> bool {
    p.exe_path.as_deref().is_none_or(|exe| {
        exe.contains(".xpc/Contents/")
            || exe.starts_with("/System/")
            || exe.starts_with("/usr/libexec/")
    })
}

/// The chain's own app owns it.
fn bundle_owns<'a>((id, label): (String, String), evidence: &mut Vec<Evidence>) -> Unowned<'a> {
    evidence.push(Evidence::new("app_bundle", id.clone(), Confidence::Certain));
    Unowned::App(id, label)
}

/// Codex sessions whose process has no Codex app on its chain are dropped (R7).
/// Codex sessions count only when a Codex app started the session's first
/// process: above the topmost process carrying this session id, not counting
/// it. A Codex binary that inherited the id (the managed app-server daemon,
/// adopted by launchd) does not vouch for itself.
fn codex_under_app(
    agents: &BTreeMap<i32, Agent>,
    index: &BTreeMap<i32, &Proc>,
    cfg: &ConfigReader,
) -> BTreeMap<i32, Agent> {
    let launched_by_codex = |proc: &Proc| {
        let (chain, _) = ancestors(proc, index);
        let top = chain
            .iter()
            .take_while(|p| p.codex_thread_id == proc.codex_thread_id)
            .count();
        chain[top..].iter().any(|p| {
            p.exe_path
                .as_deref()
                .and_then(|exe| app(exe, cfg).ok().flatten())
                .is_some_and(|(id, _)| codex::is_codex_app(&id))
        })
    };
    let mut kept = agents.clone();
    kept.retain(|pid, agent| {
        agent.tool != "codex" || index.get(pid).is_some_and(|proc| launched_by_codex(proc))
    });
    kept
}

pub fn attribute(
    snap: &Snapshot,
    uid: u32,
    cfg: &ConfigReader,
    agents: &BTreeMap<i32, Agent>,
) -> Vec<Owner> {
    let index: BTreeMap<_, _> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    let agents = if agents.values().any(|a| a.tool == "codex") {
        Cow::Owned(codex_under_app(agents, &index, cfg))
    } else {
        Cow::Borrowed(agents)
    };
    let agents = &*agents;
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
        // R17: a git fsmonitor daemon belongs to its repository, never to a pane.
        let monitor = git_fsmonitor(&proc.argv);
        let pane = chain
            .iter()
            .filter(|_| !monitor)
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
        let inherited_pane = (reaches_launchd && !monitor)
            .then(|| chain.iter().find_map(|p| p.tmux_pane.as_ref()))
            .flatten();
        // R22 (owner 2026-10-26): `open` from a pane has LaunchServices start the
        // app; TMUX_PANE is only inherited. When the root was launched as its own
        // non-terminal app, it keeps its app identity (rule ②) and the pane stays
        // as weak evidence. A runtime inside a bundle (framework Python) or a CLI
        // shipped in an app was not launched as one, so the pane rules stay.
        let opened_app = inherited_pane.is_some()
            && root.launched_app.as_deref().is_some_and(|launched| {
                root.exe_path
                    .as_deref()
                    .and_then(|exe| app(exe, cfg).ok().flatten())
                    .is_some_and(|(id, _)| id == launched && !terminals::is_terminal(&id))
            });
        let environment_pane = inherited_pane.filter(|_| !opened_app);
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
            origin.tmux_target = Some(pane.target.clone());
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
                origin.tmux_target = Some(pane.target.clone());
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
        } else if let Some(pane_id) = inherited_pane {
            evidence.push(Evidence::new(
                "tmux_launch",
                pane_id.clone(),
                Confidence::Weak,
            ));
        }
        let (id, kind, label, owner_agent) = if monitor {
            // Worktrees share their main repository's row (spec R17 leaves the choice).
            let repo = proc.git_dir.as_deref().and_then(git_repository);
            evidence.push(match &proc.git_dir {
                Some(dir) => Evidence::new("git_fsmonitor", dir.clone(), Confidence::Certain),
                None => Evidence::new("git_fsmonitor", "repository unreadable", Confidence::Weak),
            });
            if let Some(repo) = repo {
                origin.cwd = Some(repo.into());
            }
            let name = repo.and_then(|r| Path::new(r).file_name()?.to_str());
            (
                repo.map_or_else(
                    || "service:git-fsmonitor".into(),
                    |r| format!("service:git-fsmonitor:{}", repository_key(r)),
                ),
                OwnerKind::Service,
                name.map_or_else(
                    || "git fsmonitor".into(),
                    |n| format!("git fsmonitor · {n}"),
                ),
                None,
            )
        } else if let Some((agent_proc, agent)) = agent {
            agent_owner(agent_proc, agent, &mut origin, &mut evidence)
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
                // D1 ① also covers a closed pane (owner 2026-10-21 14:3x): an
                // agent on the chain owns it; the closed pane stays as a fact.
                match candidates.last().copied() {
                    Some((agent_proc, agent)) => {
                        agent_owner(agent_proc, agent, &mut origin, &mut evidence)
                    }
                    None => (
                        format!("orphan:{}", root.pid),
                        OwnerKind::Orphan,
                        root.comm.clone(),
                        None,
                    ),
                }
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
        } else if let Some((agent_proc, agent)) = candidates.last().copied() {
            // D1 = C ①: outside a pane, an agent on the chain still owns it,
            // grouped by the agent's pid when it has no session.
            agent_owner(agent_proc, agent, &mut origin, &mut evidence)
        } else {
            let context = Chain {
                proc,
                chain: &chain,
                root,
                reaches_launchd,
                uid,
            };
            match unowned(&context, cfg, agents, &index, &mut evidence) {
                Unowned::Agent(agent_proc, agent) => {
                    agent_owner(agent_proc, agent, &mut origin, &mut evidence)
                }
                Unowned::Orphan => (
                    format!("orphan:{}", root.pid),
                    OwnerKind::Orphan,
                    root.comm.clone(),
                    None,
                ),
                Unowned::App(id, label) => (format!("app:{id}"), OwnerKind::App, label, None),
                Unowned::Other if proc.uid != uid => {
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
                }
                Unowned::Other => {
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
            target.tmux_target = origin.tmux_target;
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
                    "agent_source" | "codex_thread_env" => 1,
                    "tmux_environment" => 2,
                    "runtime_home" | "responsible_terminal" => 3,
                    "app_bundle" => 4,
                    "responsible_app" | "responsible_launcher" => 5,
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
