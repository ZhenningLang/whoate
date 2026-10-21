//! Phase 10: processes under a running app are not orphans (F1, QA4), the
//! responsible/parent-chain ruling (QA5) and `.app.bundle` paths (QA6).
//!
//! D1 ruling, owner 2026-10-20 23:5x (spec 末节): C「分层判断」 — ① an agent
//! process on the chain owns it (no session: grouped by pid); ② an app on the
//! chain owns it when the root is an app; ③ an adopted root whose responsible
//! process is a running non-terminal app or agent belongs to that launcher;
//! ④ a terminal as responsible process means orphan. D3: fix `.app.bundle`.
//!
//! The ChatGPT tree is the owner's real one (research F1), rebuilt inline with
//! made-up pids and paths under a temporary directory.
mod common;

use common::Temp;
use std::{collections::BTreeMap, path::Path};
use whoate_core::{
    attribute::attribute,
    model::{Agent, Confidence, Owner, OwnerKind},
    recognizers::ConfigReader,
    snapshot::{Proc, Snapshot},
};

const UID: u32 = 501;

struct World {
    t: Temp,
    cfg: ConfigReader,
}

impl World {
    fn new() -> Self {
        let t = Temp::new();
        std::fs::create_dir_all(t.0.join("home/project")).unwrap();
        // The isolated reader only reads plists under its HOME: the temp root.
        let cfg = ConfigReader::isolated(t.0.clone());
        Self { t, cfg }
    }
    fn path(&self, rel: &str) -> String {
        self.t.0.join(rel).to_string_lossy().into()
    }
    /// Info.plist for the bundle at `rel` (an `.app` or `.app.bundle` directory).
    fn bundle(&self, rel: &str, id: &str, name: &str) {
        let path = self.t.0.join(rel).join("Contents/Info.plist");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), plist::Value::String(id.into()));
        d.insert("CFBundleName".into(), plist::Value::String(name.into()));
        plist::Value::Dictionary(d).to_file_binary(&path).unwrap();
    }
    fn proc(&self, pid: i32, ppid: i32, comm: &str, exe: &str, responsible: Option<i32>) -> Proc {
        let mut p = Proc::identity(pid, ppid, UID, Some(pid as u64), comm.into());
        p.exe_path = Some(if exe.starts_with('/') {
            exe.into()
        } else {
            self.path(exe)
        });
        p.responsible_pid = responsible;
        p.footprint_bytes = Some(1 << 20);
        p
    }
    fn in_home(&self, mut p: Proc) -> Proc {
        p.cwd = Some(self.path("home/project"));
        p
    }
    fn run(&self, procs: Vec<Proc>, agents: &BTreeMap<i32, Agent>) -> Vec<Owner> {
        let snap = Snapshot::historical("2026-10-20T23:00:00+08:00".into(), procs);
        attribute(&snap, UID, &self.cfg, agents)
    }
}

fn owner_of(owners: &[Owner], pid: i32) -> &Owner {
    owners.iter().find(|o| o.pids.contains(&pid)).unwrap()
}
fn kinds(o: &Owner) -> Vec<(&str, Confidence)> {
    o.evidence
        .iter()
        .map(|e| (e.kind.as_str(), e.confidence))
        .collect()
}

const CHATGPT: &str = "Applications/ChatGPT.app";
const CODEX: &str = "Applications/ChatGPT.app/Contents/Resources/codex-cli/CodexCLI.app";

fn chatgpt(w: &World) -> Vec<Proc> {
    w.bundle(CHATGPT, "com.openai.codex", "ChatGPT");
    w.bundle(CODEX, "com.openai.codex.cli", "ChatGPT");
    vec![
        w.proc(
            999877,
            1,
            "ChatGPT",
            &format!("{CHATGPT}/Contents/MacOS/ChatGPT"),
            Some(999877),
        ),
        w.proc(
            999445,
            999877,
            "codex",
            &format!("{CODEX}/Contents/MacOS/codex"),
            Some(999877),
        ),
        w.in_home(w.proc(
            992018,
            999445,
            "node",
            "/opt/homebrew/bin/node",
            Some(999877),
        )),
        w.in_home(w.proc(
            992021,
            999445,
            "python3.13",
            "/opt/homebrew/bin/python3.13",
            Some(999877),
        )),
        w.in_home(w.proc(
            992030,
            999877,
            "node",
            &format!("{CHATGPT}/Contents/Resources/cua_node/bin/node"),
            Some(999877),
        )),
    ]
}

#[test]
fn qa4_children_of_a_running_app_belong_to_it() {
    let w = World::new();
    let owners = w.run(chatgpt(&w), &BTreeMap::new());
    assert_eq!(
        owners.len(),
        1,
        "{:?}",
        owners.iter().map(|o| &o.id).collect::<Vec<_>>()
    );
    let o = &owners[0];
    assert_eq!(o.id, "app:com.openai.codex");
    assert_eq!(o.pids, vec![992018, 992021, 992030, 999445, 999877]);
    assert!(kinds(o).contains(&("app_bundle", Confidence::Certain)));
    assert!(!o.evidence.iter().any(|e| e.kind == "runtime_home"));
    assert!(!owners.iter().any(|o| o.id.starts_with("orphan:")));
}

#[test]
fn qa4_controls_real_orphans_stay_orphans_and_terminal_children_do_not() {
    let w = World::new();
    w.bundle(
        "Applications/Ghostty.app",
        "com.mitchellh.ghostty",
        "Ghostty",
    );
    // 1: adopted runtime in HOME, nothing on the chain gives an identity.
    let loose = w.in_home(w.proc(993001, 1, "node", "/opt/homebrew/bin/node", None));
    // 2: TMUX_PANE names a pane that no longer exists.
    let mut closed = w.proc(993101, 1, "node", "/opt/homebrew/bin/node", None);
    closed.tmux_pane = Some("%gone".into());
    // 3: node ← zsh ← login ← Ghostty.app, not in tmux.
    let ghostty = w.proc(
        993200,
        1,
        "ghostty",
        "Applications/Ghostty.app/Contents/MacOS/ghostty",
        Some(993200),
    );
    let login = w.proc(993201, 993200, "login", "/usr/bin/login", Some(993200));
    let zsh = w.proc(993202, 993201, "zsh", "/bin/zsh", Some(993200));
    let node = w.in_home(w.proc(
        993203,
        993202,
        "node",
        "/opt/homebrew/bin/node",
        Some(993200),
    ));
    let owners = w.run(
        vec![loose, closed, ghostty, login, zsh, node],
        &BTreeMap::new(),
    );

    let o = owner_of(&owners, 993001);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("orphan:993001", OwnerKind::Orphan)
    );
    assert_eq!(kinds(o), vec![("runtime_home", Confidence::Weak)]);
    let o = owner_of(&owners, 993101);
    assert_eq!(o.id, "orphan:993101");
    assert_eq!(kinds(o), vec![("tmux_environment", Confidence::Certain)]);
    let o = owner_of(&owners, 993203);
    assert_eq!(
        o.id, "app:com.mitchellh.ghostty",
        "a terminal's live children are not orphans"
    );
    assert_eq!(o.kind, OwnerKind::App);
}

#[test]
fn qa5_form_x_an_agent_on_the_chain_wins_over_responsible() {
    let w = World::new();
    let cc = "home/Library/Application Support/Claude/claude-code/2.1/claude.app";
    w.bundle(cc, "com.anthropic.claude-code", "Claude Code");
    let claude_code = w.proc(
        994100,
        1,
        "claude",
        &format!("{cc}/Contents/MacOS/claude"),
        Some(994100),
    );
    let opencode = w.in_home(w.proc(
        994000,
        1,
        "opencode",
        "/opt/homebrew/bin/opencode",
        Some(994100),
    ));
    let python = w.in_home(w.proc(
        994001,
        994000,
        "python3.13",
        "/opt/homebrew/bin/python3.13",
        Some(994100),
    ));
    let agents = BTreeMap::from([(994000, Agent::unknown("opencode"))]);
    let owners = w.run(vec![claude_code, opencode, python], &agents);
    let o = owner_of(&owners, 994001);
    assert_eq!(
        o.id, "agent:opencode:pid:994000",
        "parent chain first: the opencode process, no session"
    );
    assert_eq!(o.kind, OwnerKind::AgentSession);
    assert_eq!(o.pids, vec![994000, 994001]);
    assert!(o.agent.as_ref().is_some_and(|a| a.session_id.is_none()));
    assert!(kinds(o).contains(&("agent_source", Confidence::Weak)));
    assert_eq!(
        owner_of(&owners, 994100).id,
        "app:com.anthropic.claude-code"
    );
}

#[test]
fn qa5_form_y_an_automation_browser_belongs_to_its_launcher_not_to_the_users_browser() {
    let w = World::new();
    let mut procs = chatgpt(&w);
    w.bundle(
        "Applications/Google Chrome.app",
        "com.google.Chrome",
        "Google Chrome",
    );
    let chrome = "Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
    // The user's own Chrome: its root is itself.
    procs.push(w.proc(996000, 1, "Google Chrome", chrome, Some(996000)));
    procs.push(w.proc(996001, 996000, "Google Chrome Helper", chrome, Some(996000)));
    // agent-browser (not an app, adopted by launchd) started by ChatGPT, and its Chrome.
    procs.push(w.proc(
        995298,
        1,
        "agent-browser",
        "home/.agent-browser/bin/agent-browser-darwin-arm64",
        Some(999877),
    ));
    procs.push(w.proc(995299, 995298, "Google Chrome", chrome, Some(999877)));
    procs.push(w.proc(995300, 995299, "Google Chrome Helper", chrome, Some(999877)));
    let owners = w.run(procs, &BTreeMap::new());

    let mine = owner_of(&owners, 996000);
    assert_eq!(mine.id, "app:com.google.Chrome");
    assert_eq!(
        mine.pids,
        vec![996000, 996001],
        "the automation Chrome is not mixed in"
    );
    let launched = owner_of(&owners, 995299);
    assert_eq!(launched.id, "app:com.openai.codex");
    assert!(launched.pids.contains(&995298) && launched.pids.contains(&995300));
    assert!(kinds(launched).contains(&("responsible_launcher", Confidence::Inferred)));
    let detail = launched
        .evidence
        .iter()
        .find(|e| e.kind == "responsible_launcher")
        .unwrap();
    assert_eq!(detail.detail, "com.openai.codex");
    assert!(
        !kinds(launched).contains(&("responsible_app", Confidence::Certain)),
        "agent-browser is not a system service: ③ says inferred"
    );
}

#[test]
fn qa5_adopted_runtime_started_by_an_app_is_inferred_but_its_xpc_service_is_certain() {
    let w = World::new();
    let mut procs = chatgpt(&w);
    // A dev runtime in HOME, adopted by launchd, responsible = ChatGPT (review B round 2: pid 7318).
    procs.push(w.in_home(w.proc(
        997101,
        1,
        "python3.13",
        "/opt/homebrew/bin/python3.13",
        Some(999877),
    )));
    // An XPC service ChatGPT asked for: U-BEH-3, certain.
    procs.push(w.proc(997102, 1, "WebContent",
        "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.xpc/Contents/MacOS/com.apple.WebKit.WebContent",
        Some(999877)));
    let owners = w.run(procs, &BTreeMap::new());
    let app = owner_of(&owners, 997101);
    assert_eq!(app.id, "app:com.openai.codex");
    assert!(app.pids.contains(&997102));
    let kinds = kinds(app);
    assert!(
        kinds.contains(&("responsible_launcher", Confidence::Inferred)),
        "{kinds:?}"
    );
    assert!(
        kinds.contains(&("responsible_app", Confidence::Certain)),
        "{kinds:?}"
    );
}

#[test]
fn qa5_terminal_as_responsible_process_marks_an_orphan_only_under_the_orphan_rule() {
    let w = World::new();
    w.bundle(
        "Applications/Ghostty.app",
        "com.mitchellh.ghostty",
        "Ghostty",
    );
    let ghostty = w.proc(
        993200,
        1,
        "ghostty",
        "Applications/Ghostty.app/Contents/MacOS/ghostty",
        Some(993200),
    );
    // Started in a Ghostty window that was closed since; launchd adopted it.
    let node = w.in_home(w.proc(997001, 1, "node", "/opt/homebrew/bin/node", Some(993200)));
    // Not a runtime: stays with the terminal, as U-BEH-3 had it (review B:B1).
    let tail = w.proc(997002, 1, "tail", "/usr/bin/tail", Some(993200));
    // The terminal's own XPC service (seen on the owner's machine, review B:B1).
    let xpc = w.proc(997003, 1, "ThemeWidgetControlViewService",
        "/System/Library/Frameworks/AppKit.framework/Versions/C/XPCServices/ThemeWidgetControlViewService.xpc/Contents/MacOS/ThemeWidgetControlViewService",
        Some(993200));
    let owners = w.run(vec![ghostty, node, tail, xpc], &BTreeMap::new());
    let o = owner_of(&owners, 997001);
    assert_eq!(o.id, "orphan:997001");
    assert_eq!(
        kinds(o),
        vec![
            ("responsible_terminal", Confidence::Inferred),
            ("runtime_home", Confidence::Weak)
        ]
    );
    let terminal = owner_of(&owners, 997002);
    assert_eq!(terminal.id, "app:com.mitchellh.ghostty");
    assert!(terminal.pids.contains(&997003) && terminal.pids.contains(&993200));
    assert!(kinds(terminal).contains(&("responsible_app", Confidence::Certain)));
    assert!(
        !owners
            .iter()
            .any(|o| o.id == "orphan:997002" || o.id == "orphan:997003")
    );
}

#[test]
fn qa6_app_bundle_directories_are_apps() {
    let w = World::new();
    let clone = "private/var/folders/xx/com.google.Chrome.code_sign_clone/code_sign_clone.Ab12/Google Chrome.app.bundle";
    w.bundle(clone, "com.google.Chrome", "Google Chrome");
    let main = w.proc(
        998000,
        1,
        "Google Chrome",
        &format!("{clone}/Contents/MacOS/Google Chrome"),
        Some(998000),
    );
    let helper = w.proc(
        998001,
        998000,
        "Google Chrome Helper",
        "/opt/chrome/helper",
        Some(998000),
    );
    let owners = w.run(vec![main, helper], &BTreeMap::new());
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].id, "app:com.google.Chrome");
    assert_eq!(owners[0].label, "Google Chrome");
    assert_eq!(owners[0].kind, OwnerKind::App);
    assert!(Path::new(&w.path(clone)).ends_with("Google Chrome.app.bundle"));
}

/// D1 改判, owner 2026-10-21 14:3x (phase 10 spec「owner 点验反馈与裁决」): an
/// agent without a session whose TMUX_PANE names a closed pane belongs to the
/// agent (D1 ①), not to the orphan rule; the closed pane stays as a fact.
#[test]
fn qa45_closed_pane_agent_without_session_belongs_to_the_agent() {
    use whoate_core::{
        history::History,
        snapshot::{SampleIssue, TmuxPane},
        verdict::{Language, rules},
    };
    let w = World::new();
    let big = |mut p: Proc| {
        p.footprint_bytes = Some(2 << 30);
        p
    };
    let mut agent =
        big(w.in_home(w.proc(994001, 1, "opencode", "/opt/homebrew/bin/opencode", None)));
    agent.tmux_pane = Some("%gone".into());
    let child = w.in_home(w.proc(994002, 994001, "node", "/opt/homebrew/bin/node", None));
    // Control: the same shape without an agent on the chain stays a certain orphan.
    let mut node = big(w.in_home(w.proc(994101, 1, "node", "/opt/homebrew/bin/node", None)));
    node.tmux_pane = Some("%gone".into());
    let node_child = w.proc(994102, 994101, "node", "/opt/homebrew/bin/node", None);
    let mut agents = BTreeMap::new();
    agents.insert(994001, Agent::unknown("opencode"));
    let mut snap = Snapshot::historical(
        "2026-10-21T15:00:00+08:00".into(),
        vec![agent.clone(), child.clone(), node, node_child],
    );
    // A live pane elsewhere: the pane table was read, so %gone is known closed.
    snap.tmux_panes.push(TmuxPane {
        pane_id: "%live".into(),
        target: "demo:0.0".into(),
        window_name: None,
        pane_title: None,
        pane_pid: 994900,
        cwd: None,
    });
    let owners = attribute(&snap, UID, &w.cfg, &agents);

    let o = owner_of(&owners, 994001);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("agent:opencode:pid:994001", OwnerKind::AgentSession)
    );
    assert_eq!(o.pids, vec![994001, 994002]);
    let a = o.agent.as_ref().unwrap();
    assert_eq!(
        (a.tool.as_str(), a.session_id.as_deref()),
        ("opencode", None)
    );
    let origin = o.origin.as_ref().unwrap();
    assert_eq!(origin.pane_alive, Some(false));
    assert_eq!(origin.tmux_pane.as_deref(), Some("%gone"));
    assert!(kinds(o).contains(&("tmux_environment", Confidence::Certain)));
    for language in [Language::Zh, Language::En] {
        let v = rules::owner(o, &snap, &History::default(), false, false, language);
        assert_ne!(v.rule, "orphan-heavy");
        for word in [
            "建议关闭",
            "pane 已关闭",
            "pane is closed",
            "consider closing",
        ] {
            assert!(!v.text.contains(word), "{language:?}: {}", v.text);
        }
    }

    let o = owner_of(&owners, 994101);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("orphan:994101", OwnerKind::Orphan)
    );
    assert_eq!(o.pids, vec![994101, 994102]);
    assert_eq!(kinds(o), vec![("tmux_environment", Confidence::Certain)]);
    let v = rules::owner(o, &snap, &History::default(), false, false, Language::Zh);
    assert_eq!(v.rule, "orphan-heavy");
    assert!(v.text.contains("pane 已关闭"), "{}", v.text);

    // Control: pane table unavailable — unchanged, service with pane_status_unknown.
    let mut unknown = Snapshot::historical("2026-10-21T15:00:00+08:00".into(), vec![agent, child]);
    unknown.issues.push(SampleIssue {
        stage: "tmux".into(),
        pid: None,
        code: "failed".into(),
    });
    let owners = attribute(&unknown, UID, &w.cfg, &agents);
    let o = owner_of(&owners, 994001);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("service:994001", OwnerKind::Service)
    );
    assert!(kinds(o).contains(&("pane_status_unknown", Confidence::Weak)));
}
