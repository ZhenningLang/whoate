use crate::{
    engine::{
        Engine,
        tests::{Replay, Time, context},
    },
    model::*,
    snapshot::*,
    sources::testing::Temp,
};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

fn process(pid: i32, ppid: i32, uid: u32, name: &str, mem: u64) -> Proc {
    let mut p = Proc::identity(pid, ppid, uid, Some(1_000_000), name.into());
    p.footprint_bytes = Some(mem);
    p
}
fn scenario(temp: &Temp) -> Snapshot {
    temp.write(".claude/sessions/990021.json",r#"{"pid":990021,"sessionId":"fixture-session","name":"Fixture Claude","status":"working","cwd":"/demo"}"#);
    temp.write(".claude/projects/-demo/fixture-session.jsonl",concat!(
        "{\"type\":\"user\",\"message\":{\"content\":\"Fixture question\"}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"Fixture answer\"}]}}\n",
        "{\"type\":\"ai-title\",\"aiTitle\":\"Fixture title\",\"gitBranch\":\"fixture-branch\"}\n"));
    let db = temp.write(".local/share/opencode/opencode.db", "");
    let c = rusqlite::Connection::open(db).unwrap();
    c.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT); INSERT INTO session VALUES('open-fixture','/open-demo',NULL,'Fixture OpenCode',2000,3000);").unwrap();
    let plist=temp.write("Chrome.app/Contents/Info.plist",r#"<?xml version="1.0"?><plist version="1.0"><dict><key>CFBundleIdentifier</key><string>com.google.Chrome</string><key>CFBundleName</key><string>Fixture Chrome</string></dict></plist>"#);
    let app = plist.parent().unwrap().parent().unwrap();
    let mut orphan = process(990010, 1, 501, "node", 100);
    orphan.tmux_pane = Some("%closed".into());
    orphan.listen_ports = Some(vec![8799]);
    orphan.established_count = Some(0);
    orphan.argv = vec!["node".into(), "wrangler".into()];
    let shell = process(990020, 1, 501, "zsh", 10);
    let mut claude = process(990021, 990020, 501, "claude", 200);
    claude.cwd = Some("/demo".into());
    let mut mcp = process(990022, 990021, 501, "node", 20);
    mcp.argv = vec![
        "node".into(),
        "/demo/node_modules/example-mcp/index.js".into(),
    ];
    let mut open = process(990030, 1, 501, "opencode", 300);
    open.cwd = Some("/open-demo".into());
    let mut chrome = process(990040, 1, 501, "Google Chrome", 400);
    chrome.exe_path = Some(
        app.join("Contents/MacOS/Google Chrome")
            .to_string_lossy()
            .into(),
    );
    let mut helper = process(990041, 990040, 501, "Google Chrome Helper", 40);
    helper.exe_path = Some(
        app.join("Contents/Frameworks/Helper.app/Contents/MacOS/Helper")
            .to_string_lossy()
            .into(),
    );
    let root = process(990050, 1, 0, "root-service", 50);
    let mut snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![orphan, shell, claude, mcp, open, chrome, helper, root],
    );
    snap.tmux_panes = vec![TmuxPane {
        pane_title: None,
        pane_id: "%live".into(),
        target: "fixture:1.0".into(),
        window_name: None,
        pane_pid: 990020,
        cwd: Some("/demo".into()),
    }];
    snap
}
fn run(temp: &Temp, snapshot: Snapshot) -> Report {
    Engine::with_sampler(
        Replay(VecDeque::from([snapshot.clone(), snapshot])),
        Time::default(),
        context(temp.0.clone()),
    )
    .sample_once()
    .unwrap()
}
#[test]
fn phase02_five_scenarios_public_pipeline_exact_evidence_and_parts() {
    let t = Temp::new();
    let report = run(&t, scenario(&t));
    let owners: BTreeMap<_, _> = report.owners.iter().map(|o| (o.id.as_str(), o)).collect();
    let orphan = owners["orphan:990010"];
    assert_eq!(orphan.kind, OwnerKind::Orphan);
    assert_eq!(orphan.pids, vec![990010]);
    assert_eq!(
        orphan.origin.as_ref().unwrap().tmux_pane.as_deref(),
        Some("%closed")
    );
    assert_eq!(orphan.origin.as_ref().unwrap().pane_alive, Some(false));
    assert_eq!(orphan.evidence[0].confidence, Confidence::Certain);
    assert_eq!(orphan.verdict.rule, "orphan-dev-server");
    let claude = owners["agent:claude:fixture-session"];
    assert_eq!(claude.pids, vec![990021, 990022]);
    assert_eq!(claude.evidence[0].kind, "pane_ancestor");
    assert_eq!(claude.evidence[1].confidence, Confidence::Certain);
    let a = claude.agent.as_ref().unwrap();
    assert_eq!(a.name.as_deref(), Some("Fixture Claude"));
    assert_eq!(a.title.as_deref(), Some("Fixture title"));
    assert_eq!(a.first_prompt.as_deref(), Some("Fixture question"));
    assert_eq!(a.first_reply.as_deref(), Some("Fixture answer"));
    assert_eq!(a.branch.as_deref(), Some("fixture-branch"));
    let mut part_pids = BTreeSet::new();
    for part in &claude.parts {
        for pid in &part.pids {
            assert!(
                part_pids.insert(*pid),
                "parts must not double count pid {pid}"
            );
        }
    }
    let open = owners["agent:opencode:open-fixture"];
    assert_eq!(open.pids, vec![990030]);
    assert_eq!(
        open.agent.as_ref().unwrap().confidence,
        Confidence::Inferred
    );
    assert_eq!(
        open.agent.as_ref().unwrap().title.as_deref(),
        Some("Fixture OpenCode")
    );
    let app = owners["app:com.google.Chrome"];
    assert_eq!(app.kind, OwnerKind::App);
    assert_eq!(app.pids, vec![990040, 990041]);
    assert_eq!(app.evidence[0].confidence, Confidence::Certain);
    assert_eq!(owners["system:990050"].verdict.rule, "system");
    assert!(owners["system:990050"].verdict.action.is_none());
    assert_eq!(report.owners.iter().map(|o| o.mem_bytes).sum::<u64>(), 1120);
    let evidence = |owner: &Owner| {
        owner
            .evidence
            .iter()
            .map(|e| (e.kind.clone(), e.confidence))
            .collect::<Vec<_>>()
    };
    assert_eq!(
        evidence(orphan),
        vec![("tmux_environment".into(), Confidence::Certain)]
    );
    assert_eq!(claude.kind, OwnerKind::AgentSession);
    assert_eq!(
        evidence(claude),
        vec![
            ("pane_ancestor".into(), Confidence::Certain),
            ("agent_source".into(), Confidence::Certain)
        ]
    );
    assert_eq!(open.kind, OwnerKind::AgentSession);
    assert_eq!(
        evidence(open),
        vec![("agent_source".into(), Confidence::Inferred)]
    );
    assert_eq!(
        evidence(app),
        vec![("app_bundle".into(), Confidence::Certain)]
    );
    let root = owners["system:990050"];
    assert_eq!(root.kind, OwnerKind::System);
    assert_eq!(root.pids, vec![990050]);
    assert_eq!(
        evidence(root),
        vec![("other_uid".into(), Confidence::Certain)]
    );
}

#[test]
fn phase02_attribution_edges_pane_exec_agent_keeps_root_subtree() {
    let t = Temp::new();
    let mut s = scenario(&t);
    s.tmux_panes[0].pane_pid = 990021;
    let report = run(&t, s);
    let owner = report
        .owners
        .iter()
        .find(|o| o.id == "agent:claude:fixture-session")
        .unwrap();
    assert_eq!(owner.pids, vec![990021, 990022]);
    assert_eq!(
        owner.origin.as_ref().unwrap().tmux_pane.as_deref(),
        Some("%live")
    );
}
#[test]
fn phase02_attribution_edges_weak_names_follow_environment_before_session() {
    let t = Temp::new();
    let mut p = process(990001, 1, 501, "opencode", 10);
    p.tmux_pane = Some("%closed".into());
    let report = run(
        &t,
        Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]),
    );
    assert_eq!(report.owners[0].id, "orphan:990001");
    assert_eq!(report.owners[0].agent, None);
}
#[test]
fn phase02_attribution_edges_pane_weak_wrapper_owns_native_child() {
    let t = Temp::new();
    let root = process(990001, 1, 501, "opencode", 10);
    let child = process(990002, 990001, 501, "opencode", 20);
    let mut s = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![root, child]);
    s.tmux_panes.push(TmuxPane {
        pane_title: None,
        pane_id: "%live".into(),
        target: "demo:0.0".into(),
        window_name: None,
        pane_pid: 990001,
        cwd: None,
    });
    let report = run(&t, s);
    assert_eq!(report.owners.len(), 1);
    assert_eq!(report.owners[0].id, "agent:opencode:pid:990001");
    assert_eq!(report.owners[0].pids, vec![990001, 990002]);
    assert_eq!(
        report.owners[0].agent.as_ref().unwrap().confidence,
        Confidence::Weak
    );
}
#[test]
fn phase02_attribution_edges_tmux_failure_never_proves_pane_closed() {
    let t = Temp::new();
    for stage in ["tmux", "command", "command_spawn"] {
        let mut p = process(990001, 1, 501, "node", 10);
        p.tmux_pane = Some("%unknown".into());
        let mut s = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        s.issues.push(SampleIssue {
            stage: stage.into(),
            pid: None,
            code: "failed".into(),
        });
        let report = run(&t, s);
        assert_eq!(report.owners[0].kind, OwnerKind::Service);
        assert_eq!(report.owners[0].origin.as_ref().unwrap().pane_alive, None);
        assert!(
            report.owners[0]
                .evidence
                .iter()
                .all(|e| e.confidence == Confidence::Weak)
        );
    }
}
#[test]
fn phase02_attribution_edges_session_keeps_environment_origin() {
    let t = Temp::new();
    let mut s = scenario(&t);
    s.tmux_panes.clear();
    s.procs
        .iter_mut()
        .find(|p| p.pid == 990021)
        .unwrap()
        .tmux_pane = Some("%closed".into());
    let report = run(&t, s);
    let owner = report
        .owners
        .iter()
        .find(|o| o.id == "agent:claude:fixture-session")
        .unwrap();
    assert_eq!(
        owner.origin.as_ref().unwrap().tmux_pane.as_deref(),
        Some("%closed")
    );
    assert_eq!(owner.origin.as_ref().unwrap().pane_alive, Some(false));
    assert_eq!(owner.origin.as_ref().unwrap().cwd.as_deref(), Some("/demo"));
}
#[test]
fn phase02_attribution_edges_unconfirmed_other_uid_agents_are_system() {
    let t = Temp::new();
    let snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![
            process(990001, 1, 0, "claude", 10),
            process(990002, 1, 502, "opencode", 20),
        ],
    );
    let result = run(&t, snap);
    assert_eq!(result.owners.len(), 2);
    assert!(result.owners.iter().all(|o| o.kind == OwnerKind::System));
}

#[test]
fn phase02_recognizers_subagent_without_separate_session_stays_in_parent() {
    let t = Temp::new();
    let mut s = scenario(&t);
    s.procs.push(process(990023, 990021, 501, "claude", 30));
    let report = run(&t, s);
    let owner = report
        .owners
        .iter()
        .find(|o| o.id == "agent:claude:fixture-session")
        .unwrap();
    assert_eq!(owner.pids, vec![990021, 990022, 990023]);
    let part = owner
        .parts
        .iter()
        .find(|p| p.name == "Sub-agent: claude")
        .unwrap();
    assert_eq!(part.pids, vec![990023]);
    assert_eq!(part.mem_bytes, 30);
}
#[test]
fn phase02_conservation_real_and_reordered_all_pids_once() {
    let t = Temp::new();
    let raw = include_bytes!("../../../../tests/fixtures/capture/snapshot-20261014-1940.json");
    let mut snap: Snapshot = serde_json::from_slice(raw).unwrap();
    let report = run(&t, snap.clone());
    let actual: Vec<_> = report
        .owners
        .iter()
        .flat_map(|o| o.pids.iter().copied())
        .collect();
    assert_eq!(actual.len(), 834);
    assert_eq!(actual.iter().copied().collect::<BTreeSet<_>>().len(), 834);
    assert_eq!(
        actual.iter().copied().collect::<BTreeSet<_>>(),
        snap.procs.iter().map(|p| p.pid).collect()
    );
    assert_eq!(
        report.owners.iter().map(|o| o.mem_bytes).sum::<u64>(),
        56094758552
    );
    assert_eq!(
        report
            .owners
            .iter()
            .map(|o| o.unknown_mem_count)
            .sum::<usize>(),
        182
    );
    snap.procs.reverse();
    let reversed = run(&t, snap);
    assert_eq!(
        report
            .owners
            .iter()
            .map(|o| (&o.id, &o.pids, o.mem_bytes))
            .collect::<Vec<_>>(),
        reversed
            .owners
            .iter()
            .map(|o| (&o.id, &o.pids, o.mem_bytes))
            .collect::<Vec<_>>()
    );
}
#[test]
fn phase02_swarm_real_frozen_68_members_and_31_process_backed_owners() {
    let t = Temp::new();
    let mut snap: Snapshot = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/capture/snapshot-20261014-1940.json"
    ))
    .unwrap();
    let oracle: serde_json::Value = serde_json::from_slice(include_bytes!(
        "../../../../tests/fixtures/attribution/swarm-1940-oracle.json"
    ))
    .unwrap();
    let restore: BTreeSet<_> = oracle["restore_npm_exec_pids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    let restore_bin: BTreeSet<_> = oracle["restore_node_bin_pids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_i64().unwrap() as i32)
        .collect();
    for p in &mut snap.procs {
        if restore.contains(&p.pid) {
            assert_eq!(p.argv[1], "<redacted>");
            p.argv[1] = "exec".into();
        }
        if restore_bin.contains(&p.pid) {
            let (prefix, tail) = p.argv[1].rsplit_once("/node_modules/").unwrap();
            assert_eq!(tail, "path-103/chrome-reading-list-mcp");
            p.argv[1] = format!("{prefix}/node_modules/.bin/chrome-reading-list-mcp");
        }
    }
    let report = run(&t, snap);
    let group = report
        .swarms
        .iter()
        .find(|g| g.signature == "service:chrome-reading-list-mcp")
        .unwrap();
    assert_eq!(group.count, 68);
    assert_eq!(group.mem_bytes, 2899272448);
    assert_eq!(group.shape, Shape::Spread);
    assert_eq!(group.verdict.rule, "swarm-spread");
    assert_eq!(serde_json::to_value(&group.pids).unwrap(), oracle["pids"]);
    let owners: BTreeMap<_, _> = group
        .owners
        .iter()
        .map(|o| (o.id.clone(), o.count))
        .collect();
    assert_eq!(serde_json::to_value(owners).unwrap(), oracle["owners"]);
    assert_eq!(
        report.owners.iter().map(|o| o.mem_bytes).sum::<u64>(),
        56094758552
    );
}
