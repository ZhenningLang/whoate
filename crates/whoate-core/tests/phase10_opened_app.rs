//! Phase 10 round 9, R22 (owner 2026-10-26 11:53): an app opened with `open`
//! from a tmux pane is started by LaunchServices and only inherits TMUX_PANE.
//! The owner's real case: Scroll Reverser, ppid 1, `TMUX_PANE=%505` after %505
//! was closed, shown as an orphan; its environment has
//! `XPC_SERVICE_NAME=application.com.pilotmoon.scroll-reverser.659599856.659599862`.
//! A root launched as its own non-terminal app now belongs to the app, whether
//! the pane is closed or alive. Review B1: framework Python (`Python.app` inside
//! `Python.framework`, `XPC_SERVICE_NAME=0`) and CLIs shipped in an app bundle
//! were not launched as apps and keep the pane rules, as do terminals, runtimes
//! and agents. Process tables are built here; tests/fixtures is not touched.
mod common;

use common::Temp;
use std::collections::BTreeMap;
use whoate_core::{
    attribute::attribute,
    history::History,
    model::{Agent, Confidence, Owner, OwnerKind},
    recognizers::ConfigReader,
    snapshot::{Proc, Snapshot, TmuxPane},
    verdict::{Language, rules},
};

const UID: u32 = 501;
const REVERSER: &str = "Applications/Scroll Reverser.app";
const GHOSTTY: &str = "Applications/Ghostty.app";
const PYTHON: &str = "opt/homebrew/Cellar/python@3.14/3.14.7/Frameworks/Python.framework/Versions/3.14/Resources/Python.app";
const DOCKER: &str = "Applications/Docker.app";

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
        w.bundle(REVERSER, "com.pilotmoon.scroll-reverser", "Scroll Reverser");
        w.bundle(GHOSTTY, "com.mitchellh.ghostty", "Ghostty");
        w.bundle(PYTHON, "org.python.python", "Python");
        w.bundle(DOCKER, "com.docker.docker", "Docker");
        w
    }
    fn path(&self, rel: &str) -> String {
        self.t.0.join(rel).to_string_lossy().into()
    }
    fn bundle(&self, rel: &str, id: &str, name: &str) {
        let path = self.t.0.join(rel).join("Contents/Info.plist");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut d = plist::Dictionary::new();
        d.insert("CFBundleIdentifier".into(), plist::Value::String(id.into()));
        d.insert("CFBundleName".into(), plist::Value::String(name.into()));
        plist::Value::Dictionary(d).to_file_binary(&path).unwrap();
    }
    /// Adopted by launchd, working in HOME, with `pane` in its environment.
    fn proc(&self, pid: i32, ppid: i32, comm: &str, exe: &str, pane: Option<&str>) -> Proc {
        let mut p = Proc::identity(pid, ppid, UID, Some(pid as u64), comm.into());
        p.exe_path = Some(if exe.starts_with('/') {
            exe.into()
        } else {
            self.path(exe)
        });
        p.responsible_pid = Some(pid);
        p.cwd = Some(self.path("home"));
        p.tmux_pane = pane.map(Into::into);
        p.footprint_bytes = Some(22 << 20);
        p
    }
    fn reverser(&self, pid: i32, pane: &str) -> Proc {
        let exe = format!("{REVERSER}/Contents/MacOS/Scroll Reverser");
        let mut p = self.proc(pid, 1, "Scroll Reverser", &exe, Some(pane));
        p.launched_app = Some("com.pilotmoon.scroll-reverser".into());
        p
    }
    /// A script run from a pane by Homebrew's framework python (review B1).
    fn python(&self, pid: i32, pane: &str) -> Proc {
        let exe = format!("{PYTHON}/Contents/MacOS/Python");
        self.proc(pid, 1, "Python", &exe, Some(pane))
    }
    /// A snapshot whose pane table was read: `live` is the only pane.
    fn run(
        &self,
        procs: Vec<Proc>,
        live: &str,
        agents: &BTreeMap<i32, Agent>,
    ) -> (Snapshot, Vec<Owner>) {
        let mut snap = Snapshot::historical("2026-10-26T11:53:00+08:00".into(), procs);
        snap.tmux_panes.push(TmuxPane {
            pane_id: live.into(),
            target: "demo:0.0".into(),
            window_name: None,
            pane_title: None,
            pane_pid: 990900,
            cwd: None,
        });
        let owners = attribute(&snap, UID, &self.cfg, agents);
        (snap, owners)
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

/// The pane it was opened from is weak evidence only: not origin, not a pane row.
fn assert_opened_app(o: &Owner, pane: &str) {
    assert_eq!(
        (o.id.as_str(), o.kind, o.label.as_str()),
        (
            "app:com.pilotmoon.scroll-reverser",
            OwnerKind::App,
            "Scroll Reverser"
        )
    );
    assert_eq!(
        kinds(o),
        vec![
            ("app_bundle", Confidence::Certain),
            ("tmux_launch", Confidence::Weak)
        ]
    );
    assert!(
        o.evidence
            .iter()
            .any(|e| e.kind == "tmux_launch" && e.detail == pane)
    );
    let origin = o.origin.as_ref().unwrap();
    assert_eq!((origin.tmux_pane.as_ref(), origin.pane_alive), (None, None));
}

#[test]
fn qa65_app_opened_from_a_closed_pane_belongs_to_the_app() {
    let w = World::new();
    let (snap, owners) = w.run(vec![w.reverser(990001, "%505")], "%live", &BTreeMap::new());
    let o = owner_of(&owners, 990001);
    assert_opened_app(o, "%505");
    assert!(!owners.iter().any(|o| o.kind == OwnerKind::Orphan));
    for language in [Language::Zh, Language::En] {
        let v = rules::owner(o, &snap, &History::default(), false, false, language);
        assert_ne!(v.rule, "orphan-heavy");
        for word in ["pane 已关闭", "pane is closed", "孤儿", "orphan"] {
            assert!(!v.text.contains(word), "{language:?}: {}", v.text);
        }
    }
}

#[test]
fn qa65_app_opened_from_a_live_pane_stays_out_of_the_pane_row() {
    let w = World::new();
    let (_, owners) = w.run(vec![w.reverser(990011, "%live")], "%live", &BTreeMap::new());
    assert_opened_app(owner_of(&owners, 990011), "%live");
    assert!(!owners.iter().any(|o| o.id == "pane:%live"));
}

/// Controls: what R22 must leave as it was.
#[test]
fn qa65_terminals_runtimes_and_agents_keep_their_rules() {
    let w = World::new();
    // A terminal app at the root, even one `open` started: the pane still decides.
    let ghostty_exe = format!("{GHOSTTY}/Contents/MacOS/ghostty");
    let mut closed_term = w.proc(990101, 1, "ghostty", &ghostty_exe, Some("%gone"));
    let mut live_term = w.proc(990102, 1, "ghostty", &ghostty_exe, Some("%live"));
    for t in [&mut closed_term, &mut live_term] {
        t.launched_app = Some("com.mitchellh.ghostty".into());
    }
    // A runtime (wrangler's node) from a closed pane: still an orphan.
    let node = w.proc(990201, 1, "node", "/opt/homebrew/bin/node", Some("%gone"));
    let node_child = w.proc(990202, 990201, "node", "/opt/homebrew/bin/node", None);
    // An agent under an opened app: the agent still owns it.
    let host = w.reverser(990301, "%gone");
    let agent = w.proc(
        990302,
        990301,
        "opencode",
        "/opt/homebrew/bin/opencode",
        None,
    );
    let mut agents = BTreeMap::new();
    agents.insert(990302, Agent::unknown("opencode"));
    let (_, owners) = w.run(
        vec![closed_term, live_term, node, node_child, host, agent],
        "%live",
        &agents,
    );

    let o = owner_of(&owners, 990101);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("orphan:990101", OwnerKind::Orphan)
    );
    assert_eq!(kinds(o), vec![("tmux_environment", Confidence::Certain)]);
    assert_eq!(o.origin.as_ref().unwrap().pane_alive, Some(false));
    let o = owner_of(&owners, 990102);
    assert_eq!((o.id.as_str(), o.kind), ("pane:%live", OwnerKind::TmuxPane));
    assert_eq!(kinds(o), vec![("tmux_environment", Confidence::Certain)]);

    let o = owner_of(&owners, 990201);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("orphan:990201", OwnerKind::Orphan)
    );
    assert_eq!(o.pids, vec![990201, 990202]);
    assert_eq!(kinds(o), vec![("tmux_environment", Confidence::Certain)]);
    assert_eq!(o.origin.as_ref().unwrap().pane_alive, Some(false));

    let o = owner_of(&owners, 990302);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("agent:opencode:pid:990302", OwnerKind::AgentSession)
    );
    assert_opened_app(owner_of(&owners, 990301), "%gone");
}

/// Review B1: a bundle executable that LaunchServices did not start as that app.
#[test]
fn qa65_runtimes_and_clis_inside_bundles_keep_the_pane_rules() {
    let w = World::new();
    let closed_py = w.python(990401, "%gone");
    let live_py = w.python(990402, "%live");
    // The CLI plugin of an app (the frozen snapshot's docker-agent).
    let docker_exe = format!("{DOCKER}/Contents/Resources/cli-plugins/docker-agent");
    let docker = w.proc(990501, 1, "docker-agent", &docker_exe, Some("%gone"));
    // Launched as some other app: not its own bundle, so no app launch either.
    let mut other = w.reverser(990601, "%gone");
    other.launched_app = Some("com.example.other".into());
    let (_, owners) = w.run(
        vec![closed_py, live_py, docker, other],
        "%live",
        &BTreeMap::new(),
    );

    for (pid, id) in [
        (990401, "orphan:990401"),
        (990501, "orphan:990501"),
        (990601, "orphan:990601"),
    ] {
        let o = owner_of(&owners, pid);
        assert_eq!((o.id.as_str(), o.kind), (id, OwnerKind::Orphan));
        assert_eq!(kinds(o), vec![("tmux_environment", Confidence::Certain)]);
    }
    let o = owner_of(&owners, 990402);
    assert_eq!((o.id.as_str(), o.kind), ("pane:%live", OwnerKind::TmuxPane));
    assert!(
        !owners
            .iter()
            .any(|o| o.id == "app:org.python.python" || o.id == "app:com.docker.docker")
    );
}

/// Review A / CONFORM G3: a live pane's descendant stays with the pane, and an
/// app whose Info.plist cannot be read keeps the old pane rule.
#[test]
fn qa65_pane_descendants_and_unreadable_apps_are_unchanged() {
    let w = World::new();
    let mut shell = w.proc(990901, 990900, "zsh", "/bin/zsh", Some("%live"));
    shell.responsible_pid = None;
    let mut child = w.reverser(990902, "%live");
    child.ppid = 990901;
    let mut pane_root = w.proc(990900, 1, "tmux", "/opt/homebrew/bin/tmux", None);
    pane_root.responsible_pid = None;
    let mut unreadable = w.proc(
        990903,
        1,
        "Hidden",
        "Applications/Hidden.app/Contents/MacOS/Hidden",
        Some("%gone"),
    );
    unreadable.launched_app = Some("com.example.hidden".into());
    let (_, owners) = w.run(
        vec![pane_root, shell, child, unreadable],
        "%live",
        &BTreeMap::new(),
    );
    let o = owner_of(&owners, 990902);
    assert_eq!((o.id.as_str(), o.kind), ("pane:%live", OwnerKind::TmuxPane));
    assert!(kinds(o).contains(&("pane_ancestor", Confidence::Certain)));
    let o = owner_of(&owners, 990903);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("orphan:990903", OwnerKind::Orphan)
    );
}

/// Capture keeps only the bundle id derived from XPC_SERVICE_NAME, and a
/// recorded fixture keeps nothing of it.
#[test]
fn qa65_capture_keeps_only_the_launched_bundle_id() {
    use whoate_core::sample::{fixture, sysctl::parse_procargs};
    let parse = |env: &[u8]| {
        let mut bytes = 1_i32.to_ne_bytes().to_vec();
        bytes.extend(b"/Applications/X.app/Contents/MacOS/X\0\0X\0");
        bytes.extend(env);
        parse_procargs(&bytes).unwrap()
    };
    let args = parse(b"XPC_SERVICE_NAME=application.com.pilotmoon.scroll-reverser.659599856.659599862\0SECRET=s\0");
    assert_eq!(
        args.launched_app.as_deref(),
        Some("com.pilotmoon.scroll-reverser")
    );
    assert!(!format!("{args:?}").contains("659599856"));
    for env in [
        &b"XPC_SERVICE_NAME=0\0"[..],
        b"XPC_SERVICE_NAME=com.apple.Safari.SafeBrowsing\0",
        b"XPC_SERVICE_NAME=application.\0",
        b"XPC_SERVICE_NAME=application.123.456\0",
        b"XPC_SERVICE_NAME=application.com.example.App.1\0",
        b"XPC_SERVICE_NAME=application.123.456.B23A8862-AC6A-40AA-8303-0B05C1D002A0\0",
        b"\0",
    ] {
        assert_eq!(
            parse(env).launched_app,
            None,
            "{}",
            String::from_utf8_lossy(env)
        );
    }
    assert_eq!(
        parse(b"XPC_SERVICE_NAME=application.com.example.App2.1.2\0")
            .launched_app
            .as_deref(),
        Some("com.example.App2")
    );

    let w = World::new();
    let snap = Snapshot::historical(
        "2026-10-26T11:53:00+08:00".into(),
        vec![w.reverser(990701, "%5")],
    );
    let redacted = fixture::redact(snap.clone());
    assert_eq!(redacted.procs[0].launched_app, None);
    assert!(
        !serde_json::to_string(&redacted)
            .unwrap()
            .contains("launched_app")
    );
    let mut unsafe_snap = redacted;
    unsafe_snap.procs[0].launched_app = Some("com.pilotmoon.scroll-reverser".into());
    assert!(fixture::validate_safe(&unsafe_snap).is_err());
}

/// Review B1 (re-review, round 9): a new instance (`open -n`, or an app that
/// relaunches itself) gets `application.<id>.<n>.<n>.<UUID>`; the reviewer's
/// probe app from a pane gave the value below.
const NEW_INSTANCE: &[u8] =
    b"XPC_SERVICE_NAME=application.ai.whoate.reviewprobe.668137113.668139516.B23A8862-AC6A-40AA-8303-0B05C1D002A0\0";

fn launched(env: &[u8]) -> Option<String> {
    let mut bytes = 1_i32.to_ne_bytes().to_vec();
    bytes.extend(b"/Applications/Probe.app/Contents/MacOS/Probe\0\0Probe\0");
    bytes.extend(env);
    whoate_core::sample::sysctl::parse_procargs(&bytes)
        .unwrap()
        .launched_app
}

#[test]
fn qa65_new_instance_values_parse_to_the_bundle_id() {
    assert_eq!(
        launched(NEW_INSTANCE).as_deref(),
        Some("ai.whoate.reviewprobe")
    );
    assert_eq!(
        launched(b"XPC_SERVICE_NAME=application.ai.whoate.reviewprobe.668137113.668139516\0")
            .as_deref(),
        Some("ai.whoate.reviewprobe")
    );
    assert_eq!(
        launched(b"XPC_SERVICE_NAME=application.com.example.App.1.2.b23a8862-ac6a-40aa-8303-0b05c1d002a0\0")
            .as_deref(),
        Some("com.example.App"),
        "lower-case UUID"
    );
    // Formats seen on the owner's Mac (numbers changed): Lark and Chrome end in a
    // UUID; Sublime Text's own id ends in a digit (`com.sublimetext.4`).
    for (env, id) in [
        (&b"XPC_SERVICE_NAME=application.com.electron.lark.1.2.BAB4ED7B-263E-41DC-ABCA-E6C9FB127139\0"[..], "com.electron.lark"),
        (b"XPC_SERVICE_NAME=application.com.sublimetext.4.1.2\0", "com.sublimetext.4"),
    ] {
        assert_eq!(launched(env).as_deref(), Some(id));
    }
}

#[test]
fn qa65_new_instance_from_a_closed_pane_belongs_to_the_app() {
    let w = World::new();
    w.bundle("Applications/Probe.app", "ai.whoate.reviewprobe", "Probe");
    let mut probe = w.proc(
        990801,
        1,
        "Probe",
        "Applications/Probe.app/Contents/MacOS/Probe",
        Some("%99992"),
    );
    probe.launched_app = launched(NEW_INSTANCE);
    let (_, owners) = w.run(vec![probe], "%live", &BTreeMap::new());
    let o = owner_of(&owners, 990801);
    assert_eq!(
        (o.id.as_str(), o.kind),
        ("app:ai.whoate.reviewprobe", OwnerKind::App)
    );
    assert_eq!(
        kinds(o),
        vec![
            ("app_bundle", Confidence::Certain),
            ("tmux_launch", Confidence::Weak)
        ]
    );
}
