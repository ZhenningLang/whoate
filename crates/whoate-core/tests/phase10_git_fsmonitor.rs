//! Phase 10 round 7, R17 (owner 2026-10-25): a detached `git fsmonitor--daemon`
//! belongs to its repository, is never an orphan, and reads as normal. The owner's
//! real case: five daemons for `~/Projects/Forge` and four of its worktrees, each
//! with the closed pane %229 in its environment and cwd `~`. Process tables are
//! built here; tests/fixtures is not touched.
mod common;

use common::Temp;
use std::collections::BTreeMap;
use whoate_core::{
    attribute::{attribute, git_dir_of, git_fsmonitor, git_repository, repository_key},
    history::History,
    model::{Confidence, Level, Owner, OwnerKind},
    recognizers::ConfigReader,
    sample::{SampleResult, libproc, sysctl::ProcArgs},
    snapshot::{Proc, Snapshot},
    verdict::{Language, rules},
};

const UID: u32 = 501;
const GIT: &str = "/opt/homebrew/opt/git/libexec/git-core/git";

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|w| w.to_string()).collect()
}

/// A detached daemon as git starts it: adopted by launchd, cwd HOME, the closed pane in its environment.
fn daemon(t: &Temp, pid: i32, git_dir: Option<&str>) -> Proc {
    let mut p = Proc::identity(pid, 1, UID, Some(pid as u64), "git".into());
    p.exe_path = Some("/opt/homebrew/Cellar/git/2.55.0/bin/git".into());
    p.argv = argv(&[
        GIT,
        "fsmonitor--daemon",
        "run",
        "--detach",
        "--ipc-threads=8",
    ]);
    p.cwd = Some(t.0.to_string_lossy().into());
    p.tmux_pane = Some("%229".into());
    p.footprint_bytes = Some(4 << 20);
    p.git_dir = git_dir.map(Into::into);
    p
}

fn run(t: &Temp, procs: Vec<Proc>) -> Vec<Owner> {
    let snap = Snapshot::historical("2026-10-25T20:30:00+08:00".into(), procs);
    attribute(
        &snap,
        UID,
        &ConfigReader::isolated(t.0.clone()),
        &BTreeMap::new(),
    )
}

fn verdict(owner: &Owner, procs: Vec<Proc>, language: Language) -> whoate_core::model::Verdict {
    let snap = Snapshot::historical("2026-10-25T20:30:00+08:00".into(), procs);
    rules::owner(owner, &snap, &History::default(), false, false, language)
}

#[test]
fn qa59_daemon_with_its_repository() {
    let t = Temp::new();
    let procs = vec![daemon(&t, 99655, Some("/Users/demo/Projects/Forge/.git"))];
    let owners = run(&t, procs.clone());
    assert_eq!(owners.len(), 1);
    let o = &owners[0];
    // An opaque key, never the path (owner ids survive --redact; review round 7 B1).
    assert_eq!(
        o.id,
        format!(
            "service:git-fsmonitor:{}",
            repository_key("/Users/demo/Projects/Forge")
        )
    );
    assert!(!o.id.contains('/') && !o.id.contains("Forge"));
    assert_eq!(repository_key("/x/A"), repository_key("/x/A"));
    assert_ne!(repository_key("/x/A"), repository_key("/x/B"));
    assert_eq!(o.kind, OwnerKind::Service);
    assert_eq!(o.label, "git fsmonitor · Forge");
    assert_eq!(
        o.origin.as_ref().unwrap().tmux_pane,
        None,
        "the closed pane is no part of it"
    );
    assert_eq!(
        o.origin.as_ref().unwrap().cwd.as_deref(),
        Some("/Users/demo/Projects/Forge")
    );
    assert!(
        o.evidence
            .iter()
            .all(|e| e.kind != "tmux_environment" && e.kind != "runtime_home")
    );
    let e = o
        .evidence
        .iter()
        .find(|e| e.kind == "git_fsmonitor")
        .unwrap();
    assert_eq!(
        (e.detail.as_str(), e.confidence),
        ("/Users/demo/Projects/Forge/.git", Confidence::Certain)
    );
    // Normal, never offered for closing; the verdict table (frozen) is untouched, the
    // plain-words advice is the GUI's (R19 forbids changing verdict/).
    for language in [Language::Zh, Language::En] {
        let v = verdict(o, procs.clone(), language);
        assert_eq!(
            (v.rule.as_str(), v.level, v.action.is_none()),
            ("default", Level::Ok, true)
        );
    }
}

#[test]
fn qa59_repository_unreadable_is_still_no_orphan() {
    let t = Temp::new();
    let owners = run(&t, vec![daemon(&t, 99655, None)]);
    assert_eq!(owners.len(), 1);
    assert_eq!(
        (owners[0].id.as_str(), owners[0].label.as_str()),
        ("service:git-fsmonitor", "git fsmonitor")
    );
    assert_ne!(owners[0].kind, OwnerKind::Orphan);
    let e = owners[0]
        .evidence
        .iter()
        .find(|e| e.kind == "git_fsmonitor")
        .unwrap();
    assert_eq!(
        (e.detail.as_str(), e.confidence),
        ("repository unreadable", Confidence::Weak)
    );
}

#[test]
fn qa59_worktrees_share_their_repository_row() {
    let t = Temp::new();
    let forge = "/Users/demo/Projects/Forge/.git";
    let mut procs = vec![daemon(&t, 99655, Some(forge))];
    for (i, name) in ["Forge-a", "Forge-b", "Forge-c", "Forge-d"]
        .iter()
        .enumerate()
    {
        procs.push(daemon(
            &t,
            26467 + i as i32,
            Some(&format!("{forge}/worktrees/{name}")),
        ));
    }
    let owners = run(&t, procs);
    assert_eq!(owners.len(), 1, "{owners:?}");
    assert_eq!(owners[0].pids.len(), 5);
    assert_eq!(owners[0].mem_bytes, 5 * (4 << 20));
    assert_eq!(
        owners[0]
            .evidence
            .iter()
            .filter(|e| e.kind == "git_fsmonitor")
            .count(),
        5,
        "one per worktree"
    );
}

#[test]
fn qa59_an_ordinary_git_child_keeps_the_old_rule() {
    // `git status` left behind by a closed pane: not the daemon, so the closed-pane orphan rule applies.
    let t = Temp::new();
    let mut p = Proc::identity(4242, 1, UID, Some(1), "git".into());
    p.exe_path = Some("/opt/homebrew/Cellar/git/2.55.0/bin/git".into());
    p.argv = argv(&[GIT, "status"]);
    p.tmux_pane = Some("%229".into());
    let owners = run(&t, vec![p]);
    assert_eq!(
        (owners[0].id.as_str(), owners[0].kind),
        ("orphan:4242", OwnerKind::Orphan)
    );
    // A daemon run in the foreground (no --detach) is the user's own: no special case.
    assert!(!git_fsmonitor(&argv(&["git", "fsmonitor--daemon", "run"])));
    assert!(git_fsmonitor(&argv(&[
        "git",
        "fsmonitor--daemon",
        "run",
        "--detach"
    ])));
    assert!(!git_fsmonitor(&argv(&[
        "/usr/bin/legit",
        "fsmonitor--daemon",
        "run",
        "--detach"
    ])));
}

#[test]
fn qa59_git_dir_from_open_directories() {
    // The owner's real list for a worktree daemon (lsof, 2026-10-25), paths renamed.
    let dirs: Vec<String> = [
        "/Users/demo/Projects/Forge-wt",
        "/Users/demo/Projects",
        "/Users/demo",
        "/Users",
        "/System/Volumes/Data",
        "/Users/demo/Projects/Forge/.git/worktrees/Forge-wt",
        "/Users/demo/Projects/Forge/.git/worktrees",
        "/Users/demo/Projects/Forge/.git",
        "/Users/demo/Projects/Forge",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        git_dir_of(&dirs).as_deref(),
        Some("/Users/demo/Projects/Forge/.git/worktrees/Forge-wt")
    );
    // The main repository's daemon holds no .git directory open (owner's pid 99655): its socket names it.
    let main: Vec<String> = [
        "/Users/demo/Projects/Forge/.git/fsmonitor--daemon.ipc",
        "/Users/demo/Projects/Forge",
        "/Users",
    ]
    .map(String::from)
    .to_vec();
    assert_eq!(
        git_dir_of(&main).as_deref(),
        Some("/Users/demo/Projects/Forge/.git")
    );
    assert_eq!(git_dir_of(&main[1..]), None);
    // The socket wins over the directories.
    let both = [
        vec!["/r/.git/worktrees/w/fsmonitor--daemon.ipc".to_string()],
        dirs.clone(),
    ]
    .concat();
    assert_eq!(git_dir_of(&both).as_deref(), Some("/r/.git/worktrees/w"));
    assert_eq!(git_dir_of(&dirs[..1]), None);
    assert_eq!(
        git_dir_of(&["/x/.git/objects".into(), "/x/.github".into()]),
        None
    );
    assert_eq!(
        git_repository("/x/Forge/.git/worktrees/a"),
        Some("/x/Forge")
    );
    assert_eq!(git_repository("/x/.github/y"), None);
}

/// Answers only what enrich_proc asks; counts open-directory reads.
struct Reader {
    argv: Vec<String>,
    dir_reads: usize,
}
impl libproc::ProcessReader for Reader {
    fn identity(&mut self, pid: i32) -> Option<Proc> {
        Some(Proc::identity(pid, 1, UID, Some(1), "git".into()))
    }
    fn footprint(&mut self, _: i32) -> SampleResult<(u64, u64)> {
        Ok((4 << 20, 0))
    }
    fn exe_path(&mut self, _: i32) -> SampleResult<String> {
        Ok(GIT.into())
    }
    fn cwd(&mut self, _: i32) -> SampleResult<String> {
        Ok("/Users/demo".into())
    }
    fn args(&mut self, _: i32) -> SampleResult<ProcArgs> {
        Ok(ProcArgs {
            exe_path: GIT.into(),
            argv: self.argv.clone(),
            tmux: None,
            tmux_pane: None,
            codex_thread_id: None,
        })
    }
    fn responsible_pid(&mut self, _: i32) -> Option<i32> {
        None
    }
    fn open_paths(&mut self, _: i32) -> Vec<String> {
        self.dir_reads += 1;
        vec![
            "/Users/demo/Projects/Forge/.git/fsmonitor--daemon.ipc".into(),
            "/Users/demo".into(),
        ]
    }
}

#[test]
fn qa59_only_daemons_have_their_directories_read() {
    for (words, reads, dir) in [
        (
            vec![GIT, "fsmonitor--daemon", "run", "--detach"],
            1,
            Some("/Users/demo/Projects/Forge/.git"),
        ),
        (vec![GIT, "status"], 0, None),
    ] {
        let mut proc = Proc::identity(7, 1, UID, Some(1), "git".into());
        let mut reader = Reader {
            argv: argv(&words),
            dir_reads: 0,
        };
        assert!(libproc::enrich_proc(&mut proc, UID, &mut reader).is_empty());
        assert_eq!(
            (reader.dir_reads, proc.git_dir.as_deref()),
            (reads, dir),
            "{words:?}"
        );
    }
}
