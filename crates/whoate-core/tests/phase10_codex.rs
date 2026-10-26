//! Phase 10 R7 (QA43): a Codex tool call carries CODEX_THREAD_ID; under
//! ChatGPT.app it belongs to that Codex session. Everything runs on a made-up
//! ChatGPT.app and `.codex/state_5.sqlite` in a temporary HOME; the owner's
//! real `~/.codex` is never read.
mod common;

use common::Temp;
use std::collections::BTreeMap;
use whoate_core::{
    attribute::attribute,
    engine::{Context, Engine, SystemClock},
    model::{Agent, Confidence, Owner, OwnerKind},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler, fixture, sysctl::parse_procargs},
    snapshot::{Proc, SampleIssue, Snapshot, TmuxPane},
    sources::{Sessions, has_session_source},
    verdict::Language,
};

const UID: u32 = 501;
const CHATGPT: &str = "Applications/ChatGPT.app";
const CODEX: &str = "Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app";
/// Stands in for conversation text in the `title` column; must never surface.
const SECRET: &str = "SECRET-FIRST-MESSAGE-7731";

fn bundle(t: &Temp, rel: &str, id: &str, name: &str) {
    let path = t.0.join(rel).join("Contents/Info.plist");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut d = plist::Dictionary::new();
    d.insert("CFBundleIdentifier".into(), plist::Value::String(id.into()));
    d.insert("CFBundleName".into(), plist::Value::String(name.into()));
    plist::Value::Dictionary(d).to_file_binary(&path).unwrap();
}

fn database(t: &Temp, threads: &str) {
    std::fs::create_dir_all(t.0.join(".codex")).unwrap();
    // An older version that must not be chosen over state_5.
    std::fs::write(t.0.join(".codex/state_4.sqlite"), "").unwrap();
    let conn = rusqlite::Connection::open(t.0.join(".codex/state_5.sqlite")).unwrap();
    conn.execute_batch(&format!(
        "CREATE TABLE threads({threads});
         CREATE TABLE thread_spawn_edges(parent_thread_id TEXT, child_thread_id TEXT, status TEXT);
         INSERT INTO thread_spawn_edges VALUES('t-a','t-b','open'),('t-b','t-c','open');"
    ))
    .unwrap();
}
const FULL: &str = "id TEXT PRIMARY KEY, name TEXT, cwd TEXT, git_branch TEXT, updated_at_ms INTEGER, archived INTEGER, title TEXT";

fn rows(t: &Temp) {
    let conn = rusqlite::Connection::open(t.0.join(".codex/state_5.sqlite")).unwrap();
    for (id, name) in [("t-a", Some("修复登录页")), ("t-b", None), ("t-c", None)] {
        conn.execute(
            "INSERT INTO threads VALUES(?1, ?2, '/work/demo', 'main', 1760000000000, 0, ?3)",
            rusqlite::params![id, name, SECRET],
        )
        .unwrap();
    }
}

fn proc(t: &Temp, pid: i32, ppid: i32, comm: &str, exe: &str, thread: Option<&str>) -> Proc {
    let mut p = Proc::identity(pid, ppid, UID, Some(pid as u64), comm.into());
    p.exe_path = Some(if exe.starts_with('/') {
        exe.into()
    } else {
        t.0.join(exe).to_string_lossy().into()
    });
    p.codex_thread_id = thread.map(Into::into);
    p.cwd = Some("/elsewhere/worktree".into());
    p.footprint_bytes = Some(1 << 20);
    p
}

/// ChatGPT → codex app-server → tool calls and MCP servers, plus one tool
/// call in a live tmux pane with no Codex app on its chain.
fn world(t: &Temp) -> Snapshot {
    bundle(t, CHATGPT, "com.openai.codex", "ChatGPT");
    bundle(t, CODEX, "com.openai.codex.cli", "ChatGPT");
    let procs = vec![
        proc(
            t,
            999877,
            1,
            "ChatGPT",
            &format!("{CHATGPT}/Contents/MacOS/ChatGPT"),
            None,
        ),
        proc(
            t,
            999445,
            999877,
            "codex",
            &format!("{CODEX}/Contents/MacOS/codex"),
            None,
        ),
        proc(
            t,
            995001,
            999445,
            "python3.13",
            "/opt/homebrew/bin/python3.13",
            Some("t-a"),
        ),
        // A child whose environment was cleared still follows its parent.
        proc(t, 995002, 995001, "node", "/opt/homebrew/bin/node", None),
        proc(t, 995010, 999445, "node", "/opt/homebrew/bin/node", None),
        proc(
            t,
            995020,
            999445,
            "python3.13",
            "/opt/homebrew/bin/python3.13",
            Some("t-b"),
        ),
        proc(
            t,
            995030,
            999445,
            "python3.13",
            "/opt/homebrew/bin/python3.13",
            Some("t-c"),
        ),
        proc(t, 996000, 1, "zsh", "/bin/zsh", None),
        proc(
            t,
            996001,
            996000,
            "python3.13",
            "/opt/homebrew/bin/python3.13",
            Some("t-a"),
        ),
    ];
    let mut snap = Snapshot::historical("2026-10-21T15:00:00+08:00".into(), procs);
    snap.tmux_panes.push(TmuxPane {
        pane_id: "%x".into(),
        target: "demo:0.0".into(),
        window_name: None,
        pane_title: None,
        pane_pid: 996000,
        cwd: None,
    });
    snap
}

fn collect(snap: &Snapshot, cfg: &ConfigReader) -> (BTreeMap<i32, Agent>, Vec<SampleIssue>) {
    let own: Vec<_> = snap
        .procs
        .iter()
        .filter(|p| has_session_source(p))
        .cloned()
        .collect();
    Sessions::default().collect(&own, cfg)
}

fn owner_of(owners: &[Owner], pid: i32) -> &Owner {
    owners.iter().find(|o| o.pids.contains(&pid)).unwrap()
}

#[test]
fn qa43_procargs_keeps_only_the_codex_thread_variable() {
    let mut bytes = 1_i32.to_ne_bytes().to_vec();
    bytes.extend(b"/opt/homebrew/bin/python3.13\0\0\0python3.13\0");
    bytes.extend(
        b"HOME=/Users/demo\0CODEX_THREAD_ID=t-a\0OPENAI_API_KEY=sk-secret\0CODEX_HOME=/x\0XPC_SERVICE_NAME=0\0",
    );
    let args = parse_procargs(&bytes).unwrap();
    assert_eq!(args.codex_thread_id.as_deref(), Some("t-a"));
    assert_eq!(args.argv, vec!["python3.13"]);
    assert_eq!((&args.tmux, &args.tmux_pane), (&None, &None));
    // R22 (review B1): XPC_SERVICE_NAME is read only for its derived app id; a shell's `0` is none.
    assert_eq!(args.launched_app, None);
    let text = format!("{args:?}");
    for leaked in ["sk-secret", "/Users/demo", "CODEX_HOME"] {
        assert!(!text.contains(leaked), "{leaked}");
    }
    let mut empty = 1_i32.to_ne_bytes().to_vec();
    empty.extend(b"/bin/sh\0sh\0CODEX_THREAD_ID=\0");
    assert_eq!(parse_procargs(&empty).unwrap().codex_thread_id, None);
}

#[test]
fn qa43_codex_tool_calls_become_their_session_and_the_rest_stays_with_chatgpt() {
    let t = Temp::new();
    database(&t, FULL);
    rows(&t);
    let cfg = ConfigReader::isolated(t.0.clone());
    let snap = world(&t);
    let (agents, issues) = collect(&snap, &cfg);
    assert!(issues.is_empty(), "{issues:?}");
    let owners = attribute(&snap, UID, &cfg, &agents);

    let o = owner_of(&owners, 995001);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("agent:codex:t-a", OwnerKind::AgentSession)
    );
    // The subagent t-b and its subagent t-c fold into the topmost session.
    assert_eq!(o.pids, vec![995001, 995002, 995020, 995030]);
    let a = o.agent.as_ref().unwrap();
    assert_eq!(a.tool, "codex");
    assert_eq!(a.session_id.as_deref(), Some("t-a"));
    assert_eq!(a.name.as_deref(), Some("修复登录页"));
    assert_eq!(
        a.cwd.as_deref(),
        Some("/work/demo"),
        "the session's cwd, not the process's"
    );
    assert_eq!(a.branch.as_deref(), Some("main"));
    assert_eq!(a.confidence, Confidence::Certain);
    assert_eq!(
        (a.title.as_deref(), a.first_prompt.as_deref()),
        (None, None)
    );
    let codex: Vec<_> = o
        .evidence
        .iter()
        .filter(|e| e.kind == "codex_thread_env")
        .collect();
    assert!(!codex.is_empty());
    assert!(codex.iter().all(|e| e.confidence == Confidence::Certain));
    assert!(codex.iter().any(|e| e.detail == "t-a"));
    assert!(codex.iter().any(|e| e.detail == "t-b (subagent of t-a)"));
    assert_eq!(o.label, "修复登录页");

    let app = owner_of(&owners, 995010);
    assert_eq!(app.id, "app:com.openai.codex");
    assert_eq!(app.pids, vec![995010, 999445, 999877]);
    // No Codex app on the chain: exactly as before R7, the live pane owns it.
    assert_eq!(owner_of(&owners, 996001).id, "pane:%x");

    let json = serde_json::to_string(&owners).unwrap();
    assert!(!json.contains(SECRET), "conversation text leaked");
}

/// A Sampler that replays one snapshot, so a whole engine tick runs.
struct Fixed(Snapshot);
impl Sampler for Fixed {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.clone())
    }
}

fn tick(t: &Temp) -> whoate_core::model::Report {
    let context = Context {
        cfg: ConfigReader::isolated(t.0.clone()),
        uid: UID,
        language: Language::En,
        platform_probe: |_| Ok(false),
    };
    let mut engine =
        Engine::try_with_sampler(Fixed(world(t)), SystemClock::default(), context).unwrap();
    engine
        .tick()
        .expect("a Codex database problem never fails the tick")
}

#[test]
fn qa43_missing_or_unknown_database_degrades_to_bare_sessions() {
    for (case, code) in [
        ("missing", "codex_db_missing"),
        ("schema", "codex_schema_unknown"),
    ] {
        let t = Temp::new();
        if case == "schema" {
            database(&t, "id TEXT PRIMARY KEY, name TEXT, cwd TEXT");
        }
        let report = tick(&t);
        let o = owner_of(&report.owners, 995001);
        assert_eq!(o.id, "agent:codex:t-a", "{case}");
        let a = o.agent.as_ref().unwrap();
        assert_eq!(
            (a.name.as_deref(), a.cwd.as_deref()),
            (None, None),
            "{case}"
        );
        assert!(
            report
                .issues
                .iter()
                .any(|i| i.stage == "codex" && i.code == code),
            "{case}: {:?}",
            report.issues
        );
        // Without a readable schema no subagent edge is followed.
        assert_eq!(
            owner_of(&report.owners, 995020).id,
            "agent:codex:t-b",
            "{case}"
        );
    }
}

#[test]
fn qa43_unknown_thread_id_still_gets_a_session_row() {
    let t = Temp::new();
    database(&t, FULL);
    let report = tick(&t);
    assert_eq!(owner_of(&report.owners, 995001).id, "agent:codex:t-a");
    assert!(
        report
            .issues
            .iter()
            .any(|i| i.code == "codex_thread_unknown")
    );
}

#[test]
fn qa43_snapshots_round_trip_and_redact_the_thread_id() {
    let bytes = include_bytes!("../../../tests/fixtures/capture/snapshot-20261014-1940.json");
    let old: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    let snap: Snapshot = serde_json::from_slice(bytes).unwrap();
    assert_eq!(
        serde_json::to_value(&snap).unwrap(),
        old,
        "old fixtures round-trip unchanged"
    );

    let mut a = Proc::identity(42, 1, UID, Some(1), "python3.13".into());
    a.codex_thread_id = Some("019a2b3c-real-session".into());
    let mut b = a.clone();
    b.pid = 43;
    let mut c = a.clone();
    c.pid = 44;
    c.codex_thread_id = Some("019a2b3c-other-session".into());
    let raw = Snapshot::historical("2026-10-21T15:00:00+08:00".into(), vec![a, b, c]);
    let safe = fixture::redact(raw);
    let ids: Vec<_> = safe
        .procs
        .iter()
        .map(|p| p.codex_thread_id.as_deref().unwrap())
        .collect();
    assert_eq!(ids, vec!["thread-1", "thread-1", "thread-2"]);
    fixture::validate_safe(&safe).unwrap();
    let mut leaked = safe.clone();
    leaked.procs[0].codex_thread_id = Some("019a2b3c-real-session".into());
    assert_eq!(
        fixture::validate_safe(&leaked).unwrap_err().code,
        "unsafe_codex_thread"
    );
}

#[test]
fn qa43_codex_source_never_names_conversation_columns() {
    let source = include_str!("../src/sources/codex.rs");
    for word in ["title", "first_user_message", "preview", "rollout_path"] {
        assert!(!source.contains(word), "{word}");
    }
}

/// Review B round 3 B1: the managed app-server daemon (adopted by launchd, its
/// own executable inside CodexCLI.app) inherited a session id. Nothing above it
/// is a Codex app, so neither it nor its child is that session.
#[test]
fn qa43_a_codex_daemon_does_not_vouch_for_itself() {
    let t = Temp::new();
    database(&t, FULL);
    rows(&t);
    let cfg = ConfigReader::isolated(t.0.clone());
    let daemon_app = "home/.codex/packages/app-server-daemon/releases/1/CodexCLI.app";
    bundle(&t, daemon_app, "com.openai.codex.cli", "ChatGPT");
    let mut snap = world(&t);
    snap.procs.push(proc(
        &t,
        997000,
        1,
        "codex",
        &format!("{daemon_app}/Contents/MacOS/codex"),
        Some("t-a"),
    ));
    snap.procs.push(proc(
        &t,
        997001,
        997000,
        "codex-code-mode-host",
        &format!("{daemon_app}/Contents/MacOS/codex-code-mode-host"),
        Some("t-a"),
    ));
    let (agents, _) = collect(&snap, &cfg);
    let owners = attribute(&snap, UID, &cfg, &agents);
    for pid in [997000, 997001] {
        let o = owner_of(&owners, pid);
        assert_eq!(o.id, "app:com.openai.codex.cli", "pid {pid}");
        assert!(
            !o.evidence.iter().any(|e| e.kind == "codex_thread_env"),
            "pid {pid}"
        );
    }
    // The ordinary tool call under ChatGPT is still the session.
    assert_eq!(owner_of(&owners, 995001).id, "agent:codex:t-a");
}

/// Review A round 3: a schema read that fails for a moment (here an exclusive
/// lock) is retried next tick, even though no file changed.
#[test]
fn qa43_a_transient_read_failure_is_retried_without_a_file_change() {
    let t = Temp::new();
    database(&t, FULL);
    rows(&t);
    let cfg = ConfigReader::isolated(t.0.clone());
    let snap = world(&t);
    let own: Vec<_> = snap
        .procs
        .iter()
        .filter(|p| has_session_source(p))
        .cloned()
        .collect();
    let mut sessions = Sessions::default();
    let writer = rusqlite::Connection::open(t.0.join(".codex/state_5.sqlite")).unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
    let (agents, issues) = sessions.collect(&own, &cfg);
    assert!(
        issues.iter().any(|i| i.code == "codex_db_read_failed"),
        "{issues:?}"
    );
    assert_eq!(agents[&995001].name, None);
    writer.execute_batch("ROLLBACK").unwrap();
    let (agents, issues) = sessions.collect(&own, &cfg);
    assert!(issues.is_empty(), "{issues:?}");
    assert_eq!(agents[&995001].name.as_deref(), Some("修复登录页"));
}
