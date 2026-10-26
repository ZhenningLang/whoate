//! Phase 10 round 10, R25 (owner 2026-10-26 17:55): every row says where it was
//! started from. The core adds optional `origin.launched_from`: a pane is tmux,
//! named by the terminal of its session's most recently active client; outside
//! tmux the first launcher above the owner's anchor (its session's topmost
//! process, else its root; the anchor itself never counts): a terminal, or a
//! desktop app LaunchServices started as itself (ChatGPT for its Codex sessions;
//! a runtime that merely lives in an app bundle, like framework Python, is
//! passed over); else the terminal responsible for an adopted root; nothing when
//! nothing says. Process tables are built here; tests/fixtures is not touched.
mod common;

use common::Temp;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use whoate_core::{
    attribute::attribute,
    model::{Agent, LaunchVia, LaunchedFrom, Origin, Owner, OwnerKind},
    recognizers::ConfigReader,
    sample::{fixture, tmux},
    snapshot::{Proc, Snapshot, TmuxClient, TmuxPane},
};

const UID: u32 = 501;
const GHOSTTY: &str = "Applications/Ghostty.app";
const ITERM: &str = "Applications/iTerm.app";
const CHATGPT: &str = "Applications/ChatGPT.app";
/// Review B1: Homebrew's framework Python runs from a Python.app it was not launched as.
const PYTHON: &str = "opt/homebrew/Frameworks/Python.framework/Versions/3.14/Resources/Python.app";
/// Review A [should]: a command-line agent shipped inside a non-terminal app bundle.
const TOOL: &str = "Applications/Tool.app";

struct World {
    t: Temp,
    cfg: ConfigReader,
}

impl World {
    fn new() -> Self {
        let t = Temp::new();
        std::fs::create_dir_all(t.0.join("home")).unwrap();
        let cfg = ConfigReader::isolated(t.0.clone());
        let w = Self { t, cfg };
        w.bundle(GHOSTTY, "com.mitchellh.ghostty", "Ghostty");
        w.bundle(ITERM, "com.googlecode.iterm2", "iTerm2");
        w.bundle(CHATGPT, "com.openai.codex", "ChatGPT");
        w.bundle(PYTHON, "org.python.python", "Python");
        w.bundle(TOOL, "com.example.tool", "Tool");
        w
    }
    fn bundle(&self, rel: &str, id: &str, name: &str) {
        let path = self.t.0.join(rel).join("Contents/Info.plist");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), plist::Value::String(id.into()));
        d.insert("CFBundleName".into(), plist::Value::String(name.into()));
        plist::Value::Dictionary(d).to_file_binary(&path).unwrap();
    }
    fn proc(&self, pid: i32, ppid: i32, comm: &str, exe: &str) -> Proc {
        let mut p = Proc::identity(pid, ppid, UID, Some(pid as u64), comm.into());
        p.exe_path = Some(if exe.starts_with('/') {
            exe.into()
        } else {
            self.t.0.join(exe).to_string_lossy().into()
        });
        p.responsible_pid = Some(pid);
        p.cwd = Some(self.t.0.join("home").to_string_lossy().into());
        p.footprint_bytes = Some(10 << 20);
        p
    }
    /// A terminal app and the login under it; returns (app pid, login pid) procs.
    fn terminal(&self, pid: i32, bundle: &str, exe: &str) -> Vec<Proc> {
        vec![
            self.proc(pid, 1, exe, &format!("{bundle}/Contents/MacOS/{exe}")),
            self.proc(pid + 1, pid, "login", "/usr/bin/login"),
        ]
    }
}

fn agents(list: &[(i32, &str)]) -> BTreeMap<i32, Agent> {
    list.iter()
        .map(|(pid, tool)| (*pid, Agent::unknown(tool)))
        .collect()
}
fn owner_of(owners: &[Owner], pid: i32) -> &Owner {
    owners.iter().find(|o| o.pids.contains(&pid)).unwrap()
}
fn launch(o: &Owner) -> Option<&LaunchedFrom> {
    o.origin.as_ref()?.launched_from.as_ref()
}
fn pane(id: &str, target: &str, pid: i32) -> TmuxPane {
    TmuxPane {
        pane_id: id.into(),
        target: target.into(),
        window_name: None,
        pane_title: None,
        pane_pid: pid,
        cwd: None,
    }
}

/// tmux + Ghostty, the owner's case: tmux client 87708 → login → Ghostty. Two
/// clients on the session: the most recently active one names the terminal.
/// A session without a client is tmux, not attached.
#[test]
fn qa69_pane_rows_name_the_terminal_of_the_latest_client() {
    let w = World::new();
    let mut procs = w.terminal(500, GHOSTTY, "ghostty");
    procs.push(w.proc(502, 501, "tmux", "/opt/homebrew/bin/tmux"));
    procs.extend(w.terminal(510, ITERM, "iTerm2"));
    procs.push(w.proc(512, 511, "tmux", "/opt/homebrew/bin/tmux"));
    procs.push(w.proc(600, 1, "tmux", "/opt/homebrew/bin/tmux"));
    procs.push(w.proc(601, 600, "zsh", "/bin/zsh"));
    procs.push(w.proc(602, 601, "kilo", "/opt/homebrew/bin/kilo"));
    procs.push(w.proc(611, 600, "zsh", "/bin/zsh"));
    procs.push(w.proc(612, 611, "node", "/opt/homebrew/bin/node"));
    let mut snap = Snapshot::historical("2026-10-26T17:55:00+08:00".into(), procs);
    snap.tmux_panes = vec![
        pane("%100", "main:10.1", 601),
        pane("%200", "spare:0.0", 611),
    ];
    snap.tmux_clients = Some(vec![
        TmuxClient {
            pid: 512,
            session: "main".into(),
            activity: Some(100),
        },
        TmuxClient {
            pid: 502,
            session: "main".into(),
            activity: Some(200),
        },
    ]);
    let owners = attribute(&snap, UID, &w.cfg, &agents(&[(602, "kilo")]));
    let session = owner_of(&owners, 602);
    assert_eq!(session.kind, OwnerKind::AgentSession);
    assert_eq!(
        launch(session),
        Some(&LaunchedFrom {
            via: LaunchVia::Tmux,
            bundle_id: Some("com.mitchellh.ghostty".into()),
            name: Some("Ghostty".into()),
            attached: Some(true),
        })
    );
    assert_eq!(
        session.origin.as_ref().unwrap().tmux_target.as_deref(),
        Some("main:10.1")
    );
    let spare = owner_of(&owners, 612);
    assert_eq!(spare.kind, OwnerKind::TmuxPane);
    assert_eq!(
        launch(spare),
        Some(&LaunchedFrom {
            via: LaunchVia::Tmux,
            bundle_id: None,
            name: None,
            attached: Some(false)
        })
    );
    // Clients not captured (a fixture, an archive): tmux, terminal unknown, never "unattached".
    snap.tmux_clients = None;
    let owners = attribute(&snap, UID, &w.cfg, &agents(&[(602, "kilo")]));
    for pid in [602, 612] {
        assert_eq!(
            launch(owner_of(&owners, pid)),
            Some(&LaunchedFrom {
                via: LaunchVia::Tmux,
                bundle_id: None,
                name: None,
                attached: None
            })
        );
    }
    // The terminals' own processes are apps: macOS started them.
    assert_eq!(launch(owner_of(&owners, 500)), None);
    assert_eq!(owner_of(&owners, 500).kind, OwnerKind::App);
}

/// iTerm2 directly: claude under login under iTerm2, no tmux.
#[test]
fn qa69_a_session_run_in_a_terminal_names_that_terminal() {
    let w = World::new();
    let mut procs = w.terminal(700, ITERM, "iTerm2");
    procs.push(w.proc(702, 701, "zsh", "/bin/zsh"));
    procs.push(w.proc(703, 702, "claude", "/opt/homebrew/bin/claude"));
    let snap = Snapshot::historical("2026-10-26T17:55:00+08:00".into(), procs);
    let owners = attribute(&snap, UID, &w.cfg, &agents(&[(703, "claude")]));
    let session = owner_of(&owners, 703);
    assert_eq!(session.kind, OwnerKind::AgentSession);
    assert_eq!(
        launch(session),
        Some(&LaunchedFrom {
            via: LaunchVia::Terminal,
            bundle_id: Some("com.googlecode.iterm2".into()),
            name: Some("iTerm2".into()),
            attached: None,
        })
    );
}

/// Codex in ChatGPT: the session is split out of the app, and says the app.
#[test]
fn qa69_a_session_started_in_a_desktop_app_names_the_app() {
    let w = World::new();
    let mut app = w.proc(
        800,
        1,
        "ChatGPT",
        &format!("{CHATGPT}/Contents/MacOS/ChatGPT"),
    );
    // R22's signal: LaunchServices started it as itself.
    app.launched_app = Some("com.openai.codex".into());
    let mut codex = w.proc(801, 800, "codex", "/opt/codex/bin/codex");
    codex.codex_thread_id = Some("thread-1".into());
    let snap = Snapshot::historical("2026-10-26T17:55:00+08:00".into(), vec![app, codex]);
    let mut agent = Agent::unknown("codex");
    agent.session_id = Some("thread-1".into());
    let owners = attribute(&snap, UID, &w.cfg, &BTreeMap::from([(801, agent)]));
    let session = owner_of(&owners, 801);
    assert_eq!(session.kind, OwnerKind::AgentSession);
    assert_eq!(
        launch(session),
        Some(&LaunchedFrom {
            via: LaunchVia::App,
            bundle_id: Some("com.openai.codex".into()),
            name: Some("ChatGPT".into()),
            attached: None,
        })
    );
    let chatgpt = owner_of(&owners, 800);
    assert_eq!((chatgpt.kind, launch(chatgpt)), (OwnerKind::App, None));
}

/// Orphans: adopted by launchd with nothing on the chain says nothing; a closed
/// pane says tmux only; a terminal responsible for the root (rule ④) names it.
#[test]
fn qa69_orphans_say_tmux_the_terminal_or_nothing() {
    let w = World::new();
    let mut procs = w.terminal(500, GHOSTTY, "ghostty");
    let unknown = w.proc(900, 1, "node", "/opt/homebrew/bin/node");
    let mut closed = w.proc(910, 1, "node", "/opt/homebrew/bin/node");
    closed.tmux_pane = Some("%505".into());
    let mut from_terminal = w.proc(920, 1, "node", "/opt/homebrew/bin/node");
    from_terminal.responsible_pid = Some(500);
    procs.extend([unknown, closed, from_terminal]);
    let mut snap = Snapshot::historical("2026-10-26T17:55:00+08:00".into(), procs);
    snap.tmux_panes = vec![pane("%1", "main:0.0", 999)];
    let owners = attribute(&snap, UID, &w.cfg, &BTreeMap::new());
    for pid in [900, 910, 920] {
        assert_eq!(owner_of(&owners, pid).kind, OwnerKind::Orphan, "{pid}");
    }
    assert_eq!(
        launch(owner_of(&owners, 900)),
        None,
        "nothing says: no label"
    );
    assert_eq!(
        launch(owner_of(&owners, 910)),
        Some(&LaunchedFrom {
            via: LaunchVia::Tmux,
            bundle_id: None,
            name: None,
            attached: None
        })
    );
    assert_eq!(
        owner_of(&owners, 910).origin.as_ref().unwrap().pane_alive,
        Some(false)
    );
    assert_eq!(
        launch(owner_of(&owners, 920)).map(|l| (l.via, l.name.as_deref())),
        Some((LaunchVia::Terminal, Some("Ghostty")))
    );
}

/// The clients come from the same tmux call as the panes; a bad client line is
/// skipped, never fails the panes. Fixtures never carry clients.
#[test]
fn qa69_clients_are_parsed_leniently_and_never_recorded() {
    let (panes, clients) = tmux::parse(
        "%1\tmain:0.0\tw\t123\t/x\nclient\t87708\tmain\t1793008560\nclient\tbad\tmain\t1\nclient\t9\n",
    )
    .unwrap();
    assert_eq!(panes.len(), 1);
    assert_eq!(
        clients,
        vec![TmuxClient {
            pid: 87708,
            session: "main".into(),
            activity: Some(1793008560)
        }]
    );
    assert!(tmux::parse("%1\tmain:0.0\tw\tbad\t/x\nclient\t1\tmain\t1\n").is_err());
    let mut snap = Snapshot::historical("2026-10-26T17:55:00+08:00".into(), vec![]);
    snap.tmux_clients = Some(clients);
    let safe = fixture::redact(snap.clone());
    assert!(safe.tmux_clients.is_none());
    assert_eq!(
        fixture::validate_safe(&snap).unwrap_err().code,
        "unsafe_tmux_client"
    );
    // Older snapshots without the key still read.
    let old: Snapshot = serde_json::from_value({
        let mut v = serde_json::to_value(Snapshot::historical(
            "2026-10-26T17:55:00+08:00".into(),
            vec![],
        ))
        .unwrap();
        v.as_object_mut().unwrap().remove("tmux_clients");
        v
    })
    .unwrap();
    assert!(old.tmux_clients.is_none());
}

/// Contract: the field is optional and additive; the schema accepts it and the
/// three sources, rejects an unknown one, and an origin without it is unchanged.
#[test]
fn qa69_launched_from_is_an_optional_field_in_the_schema() {
    let schema: Value = serde_json::from_slice(
        &std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../schema/whoate-v1.schema.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let origin_schema = json!({"$defs": schema["$defs"], "$ref": "#/$defs/origin"});
    let validator = jsonschema::validator_for(&origin_schema).unwrap();
    let plain = serde_json::to_value(Origin::default()).unwrap();
    assert!(plain.get("launched_from").is_none(), "absent when unknown");
    assert!(validator.is_valid(&plain));
    for via in [LaunchVia::Tmux, LaunchVia::Terminal, LaunchVia::App] {
        let origin = Origin {
            launched_from: Some(LaunchedFrom {
                via,
                bundle_id: Some("com.mitchellh.ghostty".into()),
                name: Some("Ghostty".into()),
                attached: Some(true),
            }),
            ..Origin::default()
        };
        let value = serde_json::to_value(&origin).unwrap();
        assert!(validator.is_valid(&value), "{value}");
        assert_eq!(serde_json::from_value::<Origin>(value).unwrap(), origin);
    }
    let mut bad = plain.clone();
    bad["launched_from"] = json!({"via": "dock"});
    assert!(!validator.is_valid(&bad));
}

/// Review B1 / A [should]: a session's source comes from the session's own process
/// and what started it, never from a member's or the session's own app bundle, and
/// never depends on the order processes are listed in.
#[test]
fn qa69_a_members_or_the_agents_own_bundle_is_not_where_it_was_started() {
    let w = World::new();
    let python = format!("{PYTHON}/Contents/MacOS/Python");
    // sshd → zsh → claude, no terminal: a Python child (listed before or after its
    // session, and with a smaller pid) does not make it "Python 桌面版".
    for child_pid in [704, 699] {
        let mut procs = vec![
            w.proc(700, 1, "sshd", "/usr/sbin/sshd"),
            w.proc(702, 700, "zsh", "/bin/zsh"),
            w.proc(703, 702, "claude", "/opt/homebrew/bin/claude"),
            w.proc(child_pid, 703, "Python", &python),
        ];
        for order in 0..2 {
            if order == 1 {
                procs.reverse();
            }
            let snap = Snapshot::historical("2026-10-26T19:40:00+08:00".into(), procs.clone());
            let owners = attribute(&snap, UID, &w.cfg, &agents(&[(703, "claude")]));
            let session = owner_of(&owners, 703);
            assert_eq!(session.kind, OwnerKind::AgentSession);
            assert!(session.pids.contains(&child_pid));
            assert_eq!(launch(session), None, "child {child_pid}, order {order}");
        }
    }
    // The same session in iTerm2: iTerm2 whatever the order.
    let mut procs = w.terminal(710, ITERM, "iTerm2");
    procs.extend([
        w.proc(712, 711, "zsh", "/bin/zsh"),
        w.proc(713, 712, "claude", "/opt/homebrew/bin/claude"),
        w.proc(705, 713, "Python", &python),
    ]);
    for order in 0..2 {
        if order == 1 {
            procs.reverse();
        }
        let snap = Snapshot::historical("2026-10-26T19:40:00+08:00".into(), procs.clone());
        let owners = attribute(&snap, UID, &w.cfg, &agents(&[(713, "claude")]));
        assert_eq!(
            launch(owner_of(&owners, 705)).map(|l| (l.via, l.name.as_deref())),
            Some((LaunchVia::Terminal, Some("iTerm2"))),
            "order {order}"
        );
    }
    // An agent CLI inside a non-terminal app bundle, run in iTerm2: iTerm2, not "Tool 桌面版".
    let mut procs = w.terminal(720, ITERM, "iTerm2");
    procs.extend([
        w.proc(722, 721, "zsh", "/bin/zsh"),
        w.proc(723, 722, "tool", &format!("{TOOL}/Contents/MacOS/tool")),
    ]);
    let snap = Snapshot::historical("2026-10-26T19:40:00+08:00".into(), procs);
    let owners = attribute(&snap, UID, &w.cfg, &agents(&[(723, "tool")]));
    let session = owner_of(&owners, 723);
    assert_eq!(session.kind, OwnerKind::AgentSession);
    assert_eq!(
        launch(session).map(|l| (l.via, l.name.as_deref())),
        Some((LaunchVia::Terminal, Some("iTerm2")))
    );
}

/// Review B [question]: git detaches its fsmonitor from the terminal on purpose, so
/// the terminal responsible for it is not given as where it was started.
#[test]
fn qa69_the_git_monitor_has_no_launch_source() {
    let w = World::new();
    let mut procs = w.terminal(500, GHOSTTY, "ghostty");
    let mut monitor = w.proc(930, 1, "git", "/usr/bin/git");
    monitor.argv = ["/usr/bin/git", "fsmonitor--daemon", "start", "--detach"]
        .map(String::from)
        .to_vec();
    monitor.responsible_pid = Some(500);
    procs.push(monitor);
    let snap = Snapshot::historical("2026-10-26T19:40:00+08:00".into(), procs);
    let owners = attribute(&snap, UID, &w.cfg, &BTreeMap::new());
    let owner = owner_of(&owners, 930);
    assert_eq!(owner.kind, OwnerKind::Service);
    assert_eq!(launch(owner), None);
}

/// Review round 2 B:B1: an ancestor whose executable lives in an app bundle but
/// was not started by LaunchServices as that app (framework Python, a helper) is
/// no desktop app: the search goes on to the terminal, or says nothing.
#[test]
fn qa69_runtimes_in_app_bundles_above_a_session_are_not_desktop_apps() {
    let w = World::new();
    let python = format!("{PYTHON}/Contents/MacOS/Python");
    let run = |procs: Vec<Proc>, agent: i32, tool: &str| {
        let snap = Snapshot::historical("2026-10-26T21:50:00+08:00".into(), procs);
        let owners = attribute(&snap, UID, &w.cfg, &agents(&[(agent, tool)]));
        let o = owner_of(&owners, agent);
        assert_eq!(o.kind, OwnerKind::AgentSession);
        launch(o).map(|l| (l.via, l.name.clone()))
    };
    // iTerm2 → login → zsh → Python script → claude: iTerm2.
    let mut procs = w.terminal(710, ITERM, "iTerm2");
    procs.extend([
        w.proc(712, 711, "zsh", "/bin/zsh"),
        w.proc(713, 712, "Python", &python),
        w.proc(714, 713, "claude", "/opt/homebrew/bin/claude"),
    ]);
    assert_eq!(
        run(procs, 714, "claude"),
        Some((LaunchVia::Terminal, Some("iTerm2".into())))
    );
    // launchd (cron, a LaunchAgent) → Python script → claude: nothing says.
    let procs = vec![
        w.proc(720, 1, "Python", &python),
        w.proc(721, 720, "claude", "/opt/homebrew/bin/claude"),
    ];
    assert_eq!(run(procs, 721, "claude"), None);
    // ChatGPT (opened as itself) → a helper inside its bundle → codex: ChatGPT.
    let mut app = w.proc(
        800,
        1,
        "ChatGPT",
        &format!("{CHATGPT}/Contents/MacOS/ChatGPT"),
    );
    app.launched_app = Some("com.openai.codex".into());
    let helper = w.proc(
        802,
        800,
        "node_repl",
        &format!("{CHATGPT}/Contents/Resources/node/bin/node_repl"),
    );
    let mut codex = w.proc(803, 802, "codex", "/opt/codex/bin/codex");
    codex.codex_thread_id = Some("thread-2".into());
    let mut agent = Agent::unknown("codex");
    agent.session_id = Some("thread-2".into());
    let snap = Snapshot::historical("2026-10-26T21:50:00+08:00".into(), vec![app, helper, codex]);
    let owners = attribute(&snap, UID, &w.cfg, &BTreeMap::from([(803, agent)]));
    assert_eq!(
        launch(owner_of(&owners, 803)).map(|l| (l.via, l.name.as_deref())),
        Some((LaunchVia::App, Some("ChatGPT")))
    );
}

/// Review round 2 A [risk]: one Codex session over several processes that each
/// carry its id (codex → Python → node). The source is read above the session's
/// topmost process, whichever member comes first in the table.
#[test]
fn qa69_a_session_over_several_processes_reads_from_its_top() {
    let w = World::new();
    let mut app = w.proc(
        800,
        1,
        "ChatGPT",
        &format!("{CHATGPT}/Contents/MacOS/ChatGPT"),
    );
    app.launched_app = Some("com.openai.codex".into());
    let mut members = vec![
        w.proc(801, 800, "codex", "/opt/codex/bin/codex"),
        w.proc(
            705,
            801,
            "Python",
            &format!("{PYTHON}/Contents/MacOS/Python"),
        ),
        w.proc(704, 705, "node", "/opt/homebrew/bin/node"),
    ];
    for m in &mut members {
        m.codex_thread_id = Some("thread-3".into());
    }
    let session = |_: i32| {
        let mut a = Agent::unknown("codex");
        a.session_id = Some("thread-3".into());
        a
    };
    let agents: BTreeMap<_, _> = [801, 705, 704].map(|pid| (pid, session(pid))).into();
    for order in 0..2 {
        let mut procs = vec![app.clone()];
        procs.extend(members.clone());
        if order == 1 {
            procs.reverse();
        }
        let snap = Snapshot::historical("2026-10-26T21:50:00+08:00".into(), procs);
        let owners = attribute(&snap, UID, &w.cfg, &agents);
        let o = owner_of(&owners, 704);
        assert_eq!(o.kind, OwnerKind::AgentSession);
        assert!(
            o.pids.contains(&801) && o.pids.contains(&705),
            "{:?}",
            o.pids
        );
        assert_eq!(
            launch(o).map(|l| (l.via, l.name.as_deref())),
            Some((LaunchVia::App, Some("ChatGPT"))),
            "order {order}"
        );
    }
}
