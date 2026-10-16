//! Independent boundary fixtures; expected values are literals from the spec.
use crate::{
    attribute,
    engine::{
        Engine,
        tests::{Replay, Time, context},
    },
    history::History,
    model::*,
    recognizers::{ConfigReader, Recognizer},
    snapshot::*,
    sources::testing::Temp,
    swarm,
    verdict::{Language, rules},
};
use std::collections::{BTreeMap, VecDeque};
fn proc(pid: i32, ppid: i32, name: &str) -> Proc {
    let mut p = Proc::identity(pid, ppid, 501, Some(1), name.into());
    p.footprint_bytes = Some(1);
    p
}
fn snap(procs: Vec<Proc>) -> Snapshot {
    Snapshot::historical("2026-10-14T19:40:00+08:00".into(), procs)
}
#[test]
fn phase02_attribution_edges_home_runtime_and_broken_chain_table() {
    let t = Temp::new();
    let cfg = ConfigReader::isolated(t.0.clone());
    for (name, inside, want) in [
        ("node", true, OwnerKind::Orphan),
        ("bun", true, OwnerKind::Orphan),
        ("deno", true, OwnerKind::Orphan),
        ("python3.12", true, OwnerKind::Orphan),
        ("node", false, OwnerKind::Service),
        ("java", true, OwnerKind::Service),
        ("go", true, OwnerKind::Service),
        ("ruby", true, OwnerKind::Service),
        ("nodejs", true, OwnerKind::Service),
    ] {
        let mut p = proc(999001, 1, name);
        p.cwd = Some(if inside {
            t.0.join("project").to_string_lossy().into()
        } else {
            "/outside-home/project".into()
        });
        let owners = attribute::attribute(&snap(vec![p]), 501, &cfg, &BTreeMap::new());
        assert_eq!(owners[0].kind, want, "{name} inside={inside}");
        assert_eq!(owners[0].evidence[0].confidence, Confidence::Weak);
    }
    for ps in [
        vec![proc(999001, 999002, "demo"), proc(999002, 999001, "demo")],
        vec![proc(999001, 999001, "demo")],
        vec![proc(999001, 999999, "demo")],
    ] {
        let n = ps.len();
        let owners = attribute::attribute(&snap(ps), 501, &cfg, &BTreeMap::new());
        assert_eq!(owners.iter().map(|o| o.pids.len()).sum::<usize>(), n);
        assert!(owners.iter().all(|o| o.kind == OwnerKind::Service));
    }
}
#[test]
fn phase02_attribution_edges_live_environment_and_responsible_binary_plist() {
    let t = Temp::new();
    let cfg = ConfigReader::isolated(t.0.clone());
    let mut p = proc(999001, 1, "node");
    p.tmux_pane = Some("%live".into());
    let mut s = snap(vec![p]);
    s.tmux_panes.push(TmuxPane {
        pane_id: "%live".into(),
        target: "demo:0.0".into(),
        window_name: None,
        pane_pid: 999099,
        cwd: None,
    });
    let owners = attribute::attribute(&s, 501, &cfg, &BTreeMap::new());
    assert_eq!(owners[0].id, "pane:%live");
    assert_eq!(owners[0].kind, OwnerKind::TmuxPane);
    assert_eq!(owners[0].origin.as_ref().unwrap().pane_alive, Some(true));
    let path = t.0.join("Mail.app/Contents/Info.plist");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut dictionary = plist::Dictionary::new();
    dictionary.insert(
        "CFBundleIdentifier".into(),
        plist::Value::String("com.example.Mail".into()),
    );
    dictionary.insert(
        "CFBundleName".into(),
        plist::Value::String("Fixture Mail".into()),
    );
    plist::Value::Dictionary(dictionary)
        .to_file_binary(&path)
        .unwrap();
    let mut app = proc(999010, 1, "Mail");
    app.exe_path = Some(
        t.0.join("Mail.app/Contents/MacOS/Mail")
            .to_string_lossy()
            .into(),
    );
    let mut web = proc(999011, 1, "WebKit");
    web.responsible_pid = Some(999010);
    let owners = attribute::attribute(
        &snap(vec![app.clone(), web.clone()]),
        501,
        &cfg,
        &BTreeMap::new(),
    );
    assert_eq!(owners.len(), 1);
    assert_eq!(owners[0].id, "app:com.example.Mail");
    assert_eq!(owners[0].pids, vec![999010, 999011]);
    assert!(
        owners[0]
            .evidence
            .iter()
            .any(|e| e.kind == "responsible_app" && e.confidence == Confidence::Certain)
    );
    web.responsible_pid = None;
    let owners = attribute::attribute(&snap(vec![web]), 501, &cfg, &BTreeMap::new());
    assert_eq!(owners[0].kind, OwnerKind::Service);
    assert!(
        owners[0]
            .evidence
            .iter()
            .all(|e| e.confidence == Confidence::Weak)
    );
}
#[test]
fn phase02_runtimes_venv_entry_survives_resolved_base_executable() {
    let t = Temp::new();
    t.write("project/.venv/pyvenv.cfg", "home = /fixture/base\n");
    let mut p = proc(999001, 1, "python3.12");
    p.cwd = Some(t.0.join("project").to_string_lossy().into());
    p.exe_path = Some(t.0.join("base/bin/python3.12").to_string_lossy().into());
    p.argv = vec![
        t.0.join("project/.venv/bin/python3.12")
            .to_string_lossy()
            .into(),
        "worker.py".into(),
    ];
    let s = snap(vec![p]);
    let report = Engine::with_sampler(
        Replay(VecDeque::from([s.clone(), s])),
        Time::default(),
        context(t.0.clone()),
    )
    .sample_once()
    .unwrap();
    assert!(report.owners[0].facts.iter().any(|f| f.key == "venv"
        && f.value == FactValue::Text(t.0.join("project/.venv").to_string_lossy().into())));
}
#[test]
fn phase02_runtimes_python_venv_and_executable_near_misses() {
    let t = Temp::new();
    t.write(
        "project/pyproject.toml",
        "[project]\nname = 'python-project'\n",
    );
    let mut p = proc(999001, 1, "python3.12");
    p.cwd = Some(t.0.join("project").to_string_lossy().into());
    p.exe_path = Some(
        t.0.join("project/.venv/bin/python3.12")
            .to_string_lossy()
            .into(),
    );
    p.argv = vec![
        "python3.12".into(),
        "-m".into(),
        "ipykernel_launcher".into(),
    ];
    let mut o = Owner::new("orphan:999001".into(), OwnerKind::Orphan, "python".into());
    o.pids = vec![999001];
    crate::recognizers::dev_runtimes::DevRuntimes.explain(
        &mut o,
        &snap(vec![p]),
        &ConfigReader::isolated(t.0.clone()),
    );
    assert!(
        o.facts
            .iter()
            .any(|f| f.key == "project" && f.value == FactValue::Text("python-project".into()))
    );
    assert!(o.facts.iter().any(|f| f.key == "venv"
        && f.value == FactValue::Text(t.0.join("project/.venv").to_string_lossy().into())));
    assert_eq!(o.parts[0].name, "jupyter kernel");
    for (script, want) in [
        ("vite", Some("vite")),
        ("next", Some("next")),
        ("wrangler", Some("wrangler")),
        ("tsserver.js", Some("tsserver")),
        ("jupyter", Some("jupyter kernel")),
        ("not-vite", None),
        ("my-next-project", None),
    ] {
        let mut p = proc(999001, 1, "node");
        p.argv = vec!["node".into(), script.into()];
        assert_eq!(crate::recognizers::dev_runtimes::script(&p), want);
    }
    assert_eq!(
        crate::sources::tool(&proc(999001, 1, "2.1.299")),
        Some("claude")
    );
    assert_eq!(crate::sources::tool(&proc(999001, 1, "2.1.bad")), None);
}
#[test]
fn phase02_swarm_signature_semantics_first_three_and_logical_service() {
    let mut a = proc(999001, 1, "demo");
    a.argv = vec![
        "demo".into(),
        "--mode=worker".into(),
        "123".into(),
        "123e4567-e89b-12d3-a456-426614174000".into(),
        "ignored".into(),
    ];
    assert_eq!(swarm::signature(&a), r#"["demo","--mode=worker"]"#);
    let mut b = a.clone();
    b.argv[1] = "--mode=reader".into();
    assert_ne!(swarm::signature(&a), swarm::signature(&b));
    b = a.clone();
    b.argv[2] = "/private/tmp/random-file".into();
    assert_eq!(swarm::signature(&a), swarm::signature(&b));
    let mut npm = proc(999001, 1, "npm");
    npm.argv = vec!["npm".into(), "exec".into(), "example-mcp".into()];
    let mut node = proc(999002, 1, "node");
    node.argv = vec![
        "node".into(),
        "/demo/node_modules/example-mcp/index.js".into(),
    ];
    assert_eq!(swarm::signature(&npm), "service:example-mcp");
    assert_eq!(swarm::signature(&npm), swarm::signature(&node));
    npm.argv[1] = "uninstall".into();
    assert_ne!(swarm::signature(&npm), swarm::signature(&node));
    let mut shell = proc(999003, 1, "zsh");
    shell.argv = vec!["zsh".into(), "-c".into(), "npm exec example-mcp".into()];
    assert_eq!(swarm::logical_service(&shell), None);
}

#[test]
fn phase02_swarm_ancestor_pane_signal_groups_children_without_environment() {
    let t = Temp::new();
    for split in [false, true] {
        let mut s = snap(vec![proc(990001, 1, "zsh"), proc(990002, 1, "zsh")]);
        for (pid, id) in [(990001, "%one"), (990002, "%two")] {
            s.tmux_panes.push(TmuxPane {
                pane_id: id.into(),
                target: id.into(),
                window_name: None,
                pane_pid: pid,
                cwd: None,
            });
        }
        for i in 0..20 {
            let root = if split && i >= 10 { 990002 } else { 990001 };
            s.procs
                .push(proc(991000 + i, root, &format!("wrapper-{i}")));
            let mut child = proc(992000 + i, 991000 + i, "sleep");
            child.start_time = Some(1791978000_u64 * 1_000_000);
            s.procs.push(child);
        }
        let report = Engine::with_sampler(
            Replay(VecDeque::from([s.clone(), s])),
            Time::default(),
            context(t.0.clone()),
        )
        .sample_once()
        .unwrap();
        assert_eq!(report.swarms.len(), if split { 0 } else { 1 });
        if !split {
            assert_eq!(report.swarms[0].pids, (992000..992020).collect::<Vec<_>>());
        }
    }
}
#[test]
fn phase02_swarm_node_hosted_npm_and_extensionless_package_entry() {
    let mut wrapper = proc(999001, 1, "npm");
    wrapper.exe_path = Some("/opt/homebrew/bin/node".into());
    wrapper.argv = vec![
        "npm".into(),
        "exec".into(),
        "@scope/example-mcp@1.2.3".into(),
    ];
    assert_eq!(swarm::signature(&wrapper), "service:@scope/example-mcp");
    wrapper.comm = "node".into();
    wrapper.argv = vec![
        "node".into(),
        "/opt/homebrew/lib/node_modules/npm/bin/npm-cli.js".into(),
        "exec".into(),
        "@scope/example-mcp@1.2.3".into(),
    ];
    assert_eq!(swarm::signature(&wrapper), "service:@scope/example-mcp");
    wrapper.argv = vec![
        "node".into(),
        "/demo/node_modules/@scope/example-mcp/bin/serve".into(),
    ];
    assert_eq!(swarm::signature(&wrapper), "service:@scope/example-mcp");
    wrapper.argv = vec!["node".into(), "/demo/node_modules/.bin/example-mcp".into()];
    assert_eq!(swarm::signature(&wrapper), "service:example-mcp");
}
#[test]
fn phase02_swarm_variable_arguments_have_separate_branch_oracles() {
    let signature = |arg: &str| {
        let mut p = proc(999001, 1, "demo");
        p.argv = vec!["demo".into(), arg.into()];
        swarm::signature(&p)
    };
    assert_eq!(signature("123"), r#"["demo"]"#);
    assert_eq!(
        signature("123e4567-e89b-12d3-a456-426614174000"),
        r#"["demo"]"#
    );
    assert_eq!(signature("/private/tmp/random-file"), r#"["demo"]"#);
    assert_eq!(signature("127.0.0.1:8799"), r#"["demo"]"#);
    assert_eq!(signature("--pid=123"), r#"["demo","--pid"]"#);
}
#[test]
fn phase02_swarm_time_pane_count_and_exact_binary_gib_boundaries() {
    let now = 1791978000_u64 * 1_000_000;
    let mut s = snap(
        (0..20)
            .map(|i| {
                let mut p = proc(990000 + i, 900001, "sleep");
                p.start_time = Some(now - 600_000_000);
                p
            })
            .collect(),
    );
    let mut o = Owner::new("pane:demo".into(), OwnerKind::TmuxPane, "demo".into());
    o.pids = s.procs.iter().map(|p| p.pid).collect();
    let owners = vec![o];
    assert_eq!(swarm::detect(&s, &owners).len(), 1);
    s.procs[0].start_time = Some(now - 600_000_001);
    assert!(swarm::detect(&s, &owners).is_empty());
    for (i, p) in s.procs.iter_mut().enumerate() {
        p.start_time = Some(now);
        p.ppid = 910000 + i as i32;
    }
    assert!(swarm::detect(&s, &owners).is_empty());
    for p in &mut s.procs {
        p.tmux_pane = Some("%same".into());
    }
    assert_eq!(swarm::detect(&s, &owners)[0].shape, Shape::Burst);
    s.procs.truncate(10);
    let mut owners: Vec<_> = (0..5)
        .map(|i| Owner::new(format!("pane:{i}"), OwnerKind::TmuxPane, "demo".into()))
        .collect();
    for (i, p) in s.procs.iter_mut().enumerate() {
        p.tmux_pane = None;
        p.footprint_bytes = Some(0);
        owners[i % 5].pids.push(p.pid);
    }
    s.procs[0].footprint_bytes = Some(1073741824);
    assert_eq!(swarm::detect(&s, &owners)[0].mem_bytes, 1073741824);
    s.procs[0].footprint_bytes = Some(1073741823);
    assert!(swarm::detect(&s, &owners).is_empty());
    s.procs[0].footprint_bytes = Some(1073741824);
    s.procs.pop();
    assert!(swarm::detect(&s, &owners).is_empty());
}
#[test]
fn phase02_verdict_curve_and_idle_exact_boundaries_both_languages() {
    let s = snap(vec![proc(999001, 1, "demo")]);
    let now = (swarm::timestamp_us(&s).unwrap() / 1_000_000) as i64;
    let mut o = Owner::new("service:demo".into(), OwnerKind::Service, "demo".into());
    o.pids = vec![999001];
    for growth in [1073741823_u64, 1073741824] {
        let mut h = History::default();
        for t in 0..=1800 {
            o.mem_bytes = if t == 0 { 0 } else { growth };
            h.record_at(&o, &s.procs, now - 1800 + t);
        }
        for lang in [Language::Zh, Language::En] {
            let v = rules::owner(&o, &s, &h, true, false, lang);
            assert_eq!(
                v.rule,
                if growth == 1073741824 {
                    "leak-suspect"
                } else {
                    "default"
                }
            );
            assert!(v.action.is_none());
            assert_eq!(rules::owner(&o, &s, &h, false, false, lang).rule, "default");
        }
    }
    let mut h = History::default();
    for t in 0..=600 {
        o.mem_bytes = if t == 0 {
            0
        } else if t < 600 {
            499 * 1024 * 1024
        } else {
            500 * 1024 * 1024
        };
        h.record_at(&o, &s.procs, now - 600 + t);
    }
    for lang in [Language::Zh, Language::En] {
        let v = rules::owner(&o, &s, &h, true, false, lang);
        assert_eq!(v.rule, "fast-growth");
        assert_eq!(v.level, Level::Watch);
        assert!(v.action.is_none());
        assert!(v.text.contains("500"));
    }
    let mut flat = History::default();
    for t in 0..=600 {
        o.mem_bytes = if t == 0 { 0 } else { 500 * 1024 * 1024 };
        flat.record_at(&o, &s.procs, now - 600 + t);
    }
    assert_eq!(
        rules::owner(&o, &s, &flat, true, false, Language::En).rule,
        "default"
    );
    let mut agent = Agent::unknown("claude");
    agent.status = Some("idle".into());
    agent.updated_at_ms = Some(1791978000_i64 * 1000 - 7_200_000);
    o.agent = Some(agent);
    o.kind = OwnerKind::AgentSession;
    o.mem_bytes = 500 * 1024 * 1024;
    for lang in [Language::Zh, Language::En] {
        assert_eq!(
            rules::owner(&o, &s, &History::default(), false, false, lang).rule,
            "idle-agent"
        );
    }
    o.mem_bytes -= 1;
    assert_eq!(
        rules::owner(&o, &s, &History::default(), false, false, Language::En).rule,
        "default"
    );
    o.mem_bytes += 1;
    o.agent.as_mut().unwrap().updated_at_ms = Some(1791978000_i64 * 1000 - 7_200_000 + 1);
    assert_eq!(
        rules::owner(&o, &s, &History::default(), false, false, Language::En).rule,
        "default"
    );
}
#[test]
fn phase02_engine_cpu_bad_identity_regression_and_system_interval() {
    let t = Temp::new();
    for case in 0..5 {
        let mut p = proc(999001, 1, "demo");
        p.cpu_time_ns = Some(1000);
        let first = snap(vec![p.clone()]);
        p.cpu_time_ns = Some(2000);
        p.cpu_pct_sampled = Some(77.0);
        match case {
            0 => p.start_time = Some(2),
            1 => p.cpu_time_ns = Some(999),
            2 => p.start_time = None,
            3 => p.pid = 999002,
            4 => p.uid = 0,
            _ => {}
        }
        let mut e = Engine::with_sampler(
            Replay(VecDeque::from([first, snap(vec![p])])),
            Time::default(),
            context(t.0.clone()),
        );
        let r = e.sample_once().unwrap();
        assert_eq!(
            r.owners[0].cpu_pct,
            if case == 4 { Some(77.0) } else { None }
        );
    }
}
#[test]
fn phase02_engine_continuous_history_and_one_shot_do_not_forge_curves() {
    let t = Temp::new();
    let mut frames = VecDeque::new();
    for second in 0..=1801_u64 {
        let mut p = proc(999001, 1, "demo");
        p.footprint_bytes = Some(second * 1024 * 1024);
        let mut s = snap(vec![p]);
        if second == 900 {
            s.procs.push(proc(999002, 999001, "git"));
        }
        s.taken_at = time::OffsetDateTime::from_unix_timestamp(1791978000 + second as i64)
            .unwrap()
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        frames.push_back(s);
    }
    let mut engine = Engine::with_sampler(Replay(frames), Time::default(), context(t.0.clone()));
    let mut last = engine.tick().unwrap();
    for _ in 0..1800 {
        last = engine.tick().unwrap();
    }
    assert_eq!(last.owners[0].history.len(), 1800);
    assert_eq!(last.owners[0].verdict.rule, "leak-suspect");
}
#[test]
fn phase02_insights_count_config_memory_and_unknown_registry_negative_controls() {
    use crate::insights::{Trigger, evaluate};
    let t = Temp::new();
    let mut cfg = ConfigReader::isolated(t.0.clone());
    cfg.values.insert("enabled".into(), serde_json::json!(true));
    let mut o = Owner::new("service:demo".into(), OwnerKind::Service, "demo".into());
    o.pids = vec![999001];
    o.mem_bytes = 1024;
    assert_eq!(
        evaluate(
            &Trigger::Count {
                signal: "process_count".into(),
                min: 1
            },
            &o,
            &cfg
        ),
        Ok(true)
    );
    assert_eq!(
        evaluate(
            &Trigger::Count {
                signal: "process_count".into(),
                min: 2
            },
            &o,
            &cfg
        ),
        Ok(false)
    );
    assert_eq!(
        evaluate(&Trigger::Memory { min_bytes: 1025 }, &o, &cfg),
        Ok(false)
    );
    assert_eq!(
        evaluate(
            &Trigger::Config {
                key: "enabled".into(),
                equals: serde_json::json!(true)
            },
            &o,
            &cfg
        ),
        Ok(true)
    );
    assert_eq!(
        evaluate(
            &Trigger::Config {
                key: "enabled".into(),
                equals: serde_json::json!(false)
            },
            &o,
            &cfg
        ),
        Ok(false)
    );
    assert_eq!(
        evaluate(
            &Trigger::Config {
                key: "unknown".into(),
                equals: serde_json::Value::Null
            },
            &o,
            &cfg
        ),
        Err("unknown_config_signal")
    );
}

#[test]
fn phase02_verdict_swarm_bilingual_action_and_rule_metadata() {
    let mut group = Swarm {
        signature: "demo".into(),
        label: "demo".into(),
        count: 20,
        mem_bytes: 104857600,
        unknown_mem_count: 0,
        cpu_pct: None,
        shape: Shape::Burst,
        owners: vec![SwarmOwner {
            id: "pane:demo".into(),
            count: 20,
        }],
        parents: vec![999001],
        started_span_s: Some(60.0),
        pids: vec![999002],
        evidence: vec![],
        verdict: Verdict::normal(),
    };
    for lang in [Language::Zh, Language::En] {
        let v = rules::swarm(&group, lang);
        assert_eq!(v.rule, "swarm-burst");
        assert_eq!(v.level, Level::Act);
        assert_eq!(v.action.as_deref(), Some("whoate kill 999002"));
        assert!(v.text.contains("20"));
    }
    group.shape = Shape::Spread;
    for lang in [Language::Zh, Language::En] {
        let v = rules::swarm(&group, lang);
        assert_eq!(v.rule, "swarm-spread");
        assert_eq!(v.level, Level::Watch);
        assert!(v.action.is_none());
        assert!(v.text.contains("20"));
    }
}

#[test]
fn phase02_verdict_negative_growth_threshold_and_nonmonotonic_curve() {
    let s = snap(vec![proc(999001, 1, "demo")]);
    let now = (swarm::timestamp_us(&s).unwrap() / 1_000_000) as i64;
    let mut o = Owner::new("service:demo".into(), OwnerKind::Service, "demo".into());
    o.pids = vec![999001];
    let mut h = History::default();
    for t in 0..=600 {
        o.mem_bytes = if t == 0 {
            0
        } else if t < 600 {
            499 * 1024 * 1024
        } else {
            500 * 1024 * 1024 - 1
        };
        h.record_at(&o, &s.procs, now - 600 + t);
    }
    assert_eq!(
        rules::owner(&o, &s, &h, true, false, Language::En).rule,
        "default"
    );
    let mut h = History::default();
    for t in 0..=1800 {
        o.mem_bytes = if t == 0 {
            0
        } else if t == 900 {
            2 * 1073741824 - 1
        } else {
            2 * 1073741824
        };
        h.record_at(&o, &s.procs, now - 1800 + t);
    }
    assert!(!h.window(&o.id, 1800).unwrap().monotonic);
    assert_eq!(
        rules::owner(&o, &s, &h, true, false, Language::En).rule,
        "default"
    );
}
