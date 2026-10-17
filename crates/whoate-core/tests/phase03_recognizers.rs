mod common;
use common::Temp;
use std::{collections::VecDeque, fs, path::Path, time::Duration};
use whoate_core::{
    engine::{Clock, Context, Engine},
    model::Report,
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};

struct Replay(VecDeque<Snapshot>);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().unwrap())
    }
}
struct Time;
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        1
    }
    fn wait(&mut self, _: Duration) {}
}
fn write(root: &Path, relative: &str, text: &str) {
    let path = root.join(relative);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}
fn proc(pid: i32, parent: i32, comm: &str, exe: &str, args: &[&str], memory: Option<u64>) -> Proc {
    let mut p = Proc::identity(pid, parent, 501, Some(pid as u64), comm.into());
    p.exe_path = Some(exe.into());
    p.argv = std::iter::once(exe.into())
        .chain(args.iter().map(|s| s.to_string()))
        .collect();
    p.footprint_bytes = memory;
    p
}
fn app(home: &Path, name: &str, id: &str) -> String {
    write(
        home,
        &format!("{name}.app/Contents/Info.plist"),
        &format!(
            "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>{id}</string><key>CFBundleName</key><string>{name}</string></dict></plist>"
        ),
    );
    home.join(format!("{name}.app/Contents/MacOS/{name}"))
        .to_string_lossy()
        .into_owned()
}
fn report(home: &Path, ps: Vec<Proc>, language: Language) -> Report {
    let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), ps);
    report_snapshot(home, snap, language)
}
fn report_snapshot(home: &Path, snap: Snapshot, language: Language) -> Report {
    Engine::with_sampler(
        Replay(VecDeque::from([snap.clone(), snap])),
        Time,
        Context {
            cfg: ConfigReader::isolated(home.to_owned()),
            uid: 501,
            language,
            platform_probe: |_| Ok(false),
        },
    )
    .sample_once()
    .unwrap()
}

#[test]
fn phase03_chromium_default_engine_chrome_brave_parts_are_disjoint_and_conserve_resources() {
    for (name, id) in [
        ("Google Chrome", "com.google.Chrome"),
        ("Brave Browser", "com.brave.Browser"),
    ] {
        let home = Temp::new();
        let exe = app(&home.0, name, id);
        let ps = vec![
            proc(930001, 1, name, &exe, &[], Some(100)),
            proc(
                930002,
                930001,
                "Helper",
                &exe,
                &["--type=renderer", "https://URL_SENTINEL.invalid/private"],
                Some(20),
            ),
            proc(
                930003,
                930001,
                "Helper",
                &exe,
                &["--type=renderer", "--extension-process"],
                Some(30),
            ),
            proc(
                930004,
                930001,
                "Helper",
                &exe,
                &["--type=gpu-process"],
                None,
            ),
            proc(
                930005,
                930001,
                "Helper",
                &exe,
                &[
                    "--type=utility",
                    "--utility-sub-type=network.mojom.NetworkService",
                ],
                Some(50),
            ),
            proc(
                930006,
                930001,
                "Helper",
                &exe,
                &[
                    "--type=utility",
                    "--utility-sub-type=storage.mojom.StorageService",
                ],
                Some(60),
            ),
        ];
        let r = report(&home.0, ps, Language::En);
        assert_eq!(r.owners.len(), 1);
        let owner = &r.owners[0];
        assert_eq!(owner.id, format!("app:{id}"));
        assert_eq!(owner.mem_bytes, 260);
        assert_eq!(owner.unknown_mem_count, 1);
        for (part, pids, memory, unknown) in [
            ("Browser", vec![930001], 100, 0),
            ("Tab processes", vec![930002], 20, 0),
            ("Extensions", vec![930003], 30, 0),
            ("GPU", vec![930004], 0, 1),
            ("Network service", vec![930005], 50, 0),
            ("Storage service", vec![930006], 60, 0),
        ] {
            let p = owner.parts.iter().find(|p| p.name == part).expect(part);
            assert_eq!(p.pids, pids);
            assert_eq!(p.mem_bytes, memory);
            assert_eq!(p.unknown_mem_count, unknown);
            assert_eq!(p.process_count, 1);
        }
        assert!(!serde_json::to_string(&r).unwrap().contains("URL_SENTINEL"));
    }
}

#[test]
fn phase03_chromium_impostor_without_browser_app_path_is_not_a_browser() {
    let home = Temp::new();
    let r = report(
        &home.0,
        vec![proc(
            930001,
            1,
            "Google Chrome Helper",
            "/untrusted/demo",
            &["--type=renderer"],
            Some(20),
        )],
        Language::En,
    );
    assert!(r.owners[0].parts.is_empty());
}

#[test]
fn phase03_browser_config_preferences_are_known_only_from_valid_product_file() {
    use whoate_core::model::FactValue;
    for (contents, expected) in [
        ("{}", Some(0)),
        (
            r#"{"performance_tuning":{"high_efficiency_mode":{"state":0}}}"#,
            Some(0),
        ),
        (
            r#"{"performance_tuning.high_efficiency_mode.state":1}"#,
            Some(1),
        ),
        (
            r#"{"performance_tuning.high_efficiency_mode.state":2}"#,
            Some(2),
        ),
        ("broken", None),
        ("[]", None),
        (
            r#"{"performance_tuning.high_efficiency_mode.state":99}"#,
            None,
        ),
    ] {
        let home = Temp::new();
        let exe = app(&home.0, "Google Chrome", "com.google.Chrome");
        write(
            &home.0,
            "Library/Application Support/Google/Chrome/Local State",
            contents,
        );
        write(
            &home.0,
            "Library/Application Support/Google/Chrome/Default/Preferences",
            "URL_HISTORY_SECRET_SENTINEL",
        );
        let r = report(
            &home.0,
            vec![proc(930001, 1, "Chrome", &exe, &[], Some(20))],
            Language::En,
        );
        let state = r.owners[0]
            .facts
            .iter()
            .find(|f| f.key == "chrome.memory_saver_state")
            .map(|f| f.value.clone());
        assert_eq!(state, expected.map(FactValue::Count));
        assert!(
            r.owners[0]
                .facts
                .iter()
                .any(|f| f.key == "browser_policy_unknown" && f.value == FactValue::Bool(true))
        );
        assert!(
            !serde_json::to_string(&r)
                .unwrap()
                .contains("SECRET_SENTINEL")
        );
    }
}

#[test]
fn phase03_venv_relative_absolute_symlink_and_base_use_public_engine() {
    use whoate_core::model::FactValue;
    let home = Temp::new();
    write(
        &home.0,
        "project with spaces/.venv/pyvenv.cfg",
        "home = /public/base\n",
    );
    std::os::unix::fs::symlink(
        home.0.join("project with spaces/.venv"),
        home.0.join("project with spaces/venv-link"),
    )
    .unwrap();
    let expected = FactValue::Text(
        home.0
            .join("project with spaces/.venv")
            .to_string_lossy()
            .into(),
    );
    for entry in [
        home.0
            .join("project with spaces/.venv/bin/python3.13")
            .to_string_lossy()
            .into_owned(),
        ".venv/bin/python3.13".into(),
        "venv-link/bin/python3.13".into(),
    ] {
        let mut p = proc(
            930001,
            1,
            "python3.13",
            "/public/base/bin/python3.13",
            &[],
            Some(10),
        );
        p.argv = vec![entry];
        p.cwd = Some(home.0.join("project with spaces").to_string_lossy().into());
        let r = report(&home.0, vec![p], Language::En);
        assert_eq!(
            r.owners[0]
                .facts
                .iter()
                .find(|f| f.key == "venv")
                .map(|f| &f.value),
            Some(&expected)
        );
    }
    for entry in [
        "python",
        ".venv/bin/python-not-python",
        "/public/base/bin/python3.13",
    ] {
        let mut p = proc(
            930001,
            1,
            "python3.13",
            "/public/base/bin/python3.13",
            &[],
            Some(10),
        );
        p.argv = vec![entry.into()];
        p.cwd = Some(home.0.join("project with spaces").to_string_lossy().into());
        let r = report(&home.0, vec![p], Language::En);
        assert!(!r.owners[0].facts.iter().any(|f| f.key == "venv"));
    }
}

#[test]
fn phase03_webkit_responsible_owners_keep_distinct_web_gpu_network_parts() {
    let home = Temp::new();
    let mail = app(&home.0, "Mail", "com.example.Mail");
    let other = app(&home.0, "Other", "com.example.Other");
    let mut ps = vec![
        proc(930001, 1, "Mail", &mail, &[], Some(100)),
        proc(930002, 1, "Other", &other, &[], Some(200)),
    ];
    for (pid, name, responsible, memory) in [
        (930003, "WebContent", Some(930001), Some(30)),
        (930004, "GPU", Some(930001), Some(40)),
        (930005, "Networking", Some(930001), None),
        (930006, "WebContent", Some(930002), Some(60)),
        (930007, "WebContent", None, Some(70)),
        (930008, "WebContent", Some(999999), Some(80)),
        (930009, "WebContent", Some(930010), Some(90)),
        (930010, "GPU", Some(930009), Some(100)),
    ] {
        let mut p = proc(
            pid,
            1,
            &format!("com.apple.WebKit.{name}"),
            &format!(
                "/System/Library/Frameworks/WebKit.framework/XPCServices/com.apple.WebKit.{name}.xpc/Contents/MacOS/com.apple.WebKit.{name}"
            ),
            &[],
            memory,
        );
        p.responsible_pid = responsible;
        ps.push(p);
    }
    let r = report(&home.0, ps, Language::En);
    let mail = r
        .owners
        .iter()
        .find(|o| o.id == "app:com.example.Mail")
        .unwrap();
    assert_eq!(mail.pids, vec![930001, 930003, 930004, 930005]);
    assert_eq!(mail.mem_bytes, 170);
    assert_eq!(mail.unknown_mem_count, 1);
    for (name, pids) in [
        ("Web content", vec![930003]),
        ("WebKit GPU", vec![930004]),
        ("WebKit network", vec![930005]),
    ] {
        assert_eq!(
            mail.parts
                .iter()
                .find(|p| p.name == name)
                .map(|p| p.pids.clone()),
            Some(pids)
        );
    }
    let other = r
        .owners
        .iter()
        .find(|o| o.id == "app:com.example.Other")
        .unwrap();
    assert_eq!(
        other
            .parts
            .iter()
            .find(|p| p.name == "Web content")
            .map(|p| p.pids.clone()),
        Some(vec![930006])
    );
    for pid in [930007, 930008, 930009, 930010] {
        let owner = r.owners.iter().find(|o| o.pids.contains(&pid)).unwrap();
        assert_eq!(owner.kind, whoate_core::model::OwnerKind::Service);
        assert!(owner.parts.is_empty());
    }
}

#[test]
fn phase03_vscode_extensions_watchers_and_languages_precede_runtime_fallback() {
    let home = Temp::new();
    let exe = app(&home.0, "Visual Studio Code", "com.microsoft.VSCode");
    let plugin = home.0.join("Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)").to_string_lossy().into_owned();
    let ps = vec![
        proc(930001, 1, "Code", &exe, &[], Some(100)),
        proc(
            930002,
            930001,
            "Helper",
            &exe,
            &["--type=extensionHost"],
            Some(20),
        ),
        proc(
            930003,
            930001,
            "Code Helper (Plugin)",
            &plugin,
            &[
                "--type=utility",
                "--utility-sub-type=node.mojom.NodeService",
                "--skipWorkspaceStorageLock",
            ],
            Some(30),
        ),
        proc(
            930004,
            930001,
            "Code Helper (Plugin)",
            &plugin,
            &[
                "--type=utility",
                "--utility-sub-type=node.mojom.NodeService",
            ],
            Some(40),
        ),
        proc(
            930005,
            930001,
            "Helper",
            &exe,
            &["--type=fileWatcher"],
            Some(50),
        ),
        proc(
            930006,
            930002,
            "node",
            "/public/node",
            &["/public/typescript/lib/tsserver.js"],
            Some(60),
        ),
        proc(
            930007,
            930003,
            "node",
            "/public/node",
            &["/public/.vscode/extensions/ms-python.vscode-pylance-2026.2.1/dist/server.bundle.js"],
            None,
        ),
    ];
    let r = report(&home.0, ps, Language::En);
    let owner = r.owners.iter().find(|o| o.pids.contains(&930001)).unwrap();
    for (name, pids, memory, unknown) in [
        ("Extension hosts", vec![930002, 930003], 50, 0),
        ("File watchers", vec![930005], 50, 0),
        ("TypeScript language server", vec![930006], 60, 0),
        ("Pylance", vec![930007], 0, 1),
    ] {
        let part = owner.parts.iter().find(|p| p.name == name).expect(name);
        assert_eq!(part.pids, pids);
        assert_eq!(part.mem_bytes, memory);
        assert_eq!(part.unknown_mem_count, unknown);
    }
    let members: Vec<_> = owner.parts.iter().flat_map(|p| p.pids.clone()).collect();
    assert_eq!(
        members.len(),
        members
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
    );
    assert_eq!(owner.mem_bytes, 300);
}

#[test]
fn phase03_jetbrains_runtime_xmx_and_running_version_config_remain_distinct() {
    use whoate_core::model::FactValue;
    let home = Temp::new();
    let exe = app(&home.0, "PyCharm", "com.jetbrains.pycharm.ce");
    write(
        &home.0,
        "PyCharm.app/Contents/Info.plist",
        "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.jetbrains.pycharm.ce</string><key>CFBundleShortVersionString</key><string>2024.3.7</string></dict></plist>",
    );
    write(
        &home.0,
        "Library/Application Support/JetBrains/PyCharmCE2024.3/pycharm.vmoptions",
        "-Xms128m\n-Xmx2048m\n",
    );
    write(
        &home.0,
        "Library/Application Support/JetBrains/PyCharmCE2025.2/pycharm.vmoptions",
        "-Xmx8g\n",
    );
    for (arg, expected) in [
        ("-Xmx3g", Some(3221225472)),
        ("-Xmx256m", Some(268435456)),
        ("-Xmx1024k", Some(1048576)),
        ("-Xmx1024", Some(1024)),
        ("-Xmx0", None),
        ("-Xmx3oops", None),
        ("-Xmx999999999999999999999g", None),
    ] {
        let r = report(
            &home.0,
            vec![proc(930001, 1, "pycharm", &exe, &[arg], Some(100))],
            Language::En,
        );
        let facts = &r.owners[0].facts;
        assert_eq!(
            facts
                .iter()
                .find(|f| f.key == "jvm_xmx_bytes")
                .map(|f| f.value.clone()),
            expected.map(FactValue::Count)
        );
        assert_eq!(
            facts
                .iter()
                .find(|f| f.key == "jvm_config_xmx_bytes")
                .map(|f| f.value.clone()),
            Some(FactValue::Count(2147483648))
        );
        assert_eq!(
            facts
                .iter()
                .find(|f| f.key == "jetbrains_selector")
                .map(|f| f.value.clone()),
            Some(FactValue::Text("PyCharmCE2024.3".into()))
        );
        assert_eq!(
            r.owners[0]
                .parts
                .iter()
                .find(|p| p.name == "JVM")
                .map(|p| p.pids.clone()),
            Some(vec![930001])
        );
    }
}

#[test]
fn phase03_user_rules_first_match_same_member_and_preserve_owner_identity() {
    use whoate_core::model::FactValue;
    let home = Temp::new();
    let cwd = home.0.join("project").to_string_lossy().into_owned();
    let mut p = proc(
        930001,
        1,
        "mytool",
        "/public/mytool",
        &["--mode=dev"],
        Some(100),
    );
    p.cwd = Some(cwd.clone());
    let before = report(&home.0, vec![p.clone()], Language::En);
    let rules = format!(
        "[[rules]]\nexecutable='mytool'\nargv_regex='--mode=dev'\ncwd_prefix='{cwd}'\nlabel='Friendly'\nadvice='Do not execute: harmless text'\n\n[[rules]]\nexecutable='mytool'\nlabel='Wrong second match'\nadvice='second'\n"
    );
    write(&home.0, "rules.toml", &rules);
    let changed = report(&home.0, vec![p.clone()], Language::En);
    let owner = &changed.owners[0];
    assert_eq!(owner.label, "Friendly");
    assert!(owner.facts.iter().any(|f| f.key == "user_advice"
        && f.value == FactValue::Text("Do not execute: harmless text".into())));
    let mut normalized = owner.clone();
    normalized.label = before.owners[0].label.clone();
    normalized
        .facts
        .retain(|f| f.key != "user_rule" && f.key != "user_advice");
    assert_eq!(normalized, before.owners[0]);
    p.cwd = Some(format!("{cwd}-neighbor"));
    let r = report(&home.0, vec![p], Language::En);
    assert_eq!(r.owners[0].label, "Wrong second match");
    write(
        &home.0,
        "rules.toml",
        "[[rules]]\nexecutable='mytool'\nlabel='Bad'\nadvice='secret-invalid-pattern'\nargv_regex='['\n",
    );
    let r = report(
        &home.0,
        vec![proc(930001, 1, "mytool", "/public/mytool", &[], Some(100))],
        Language::En,
    );
    assert_eq!(r.owners[0].label, "mytool");
    assert!(
        r.issues
            .iter()
            .any(|i| i.stage == "user_rules" && i.code == "invalid_regex")
    );
    assert!(
        !serde_json::to_string(&r)
            .unwrap()
            .contains("secret-invalid-pattern")
    );
}

#[test]
fn phase03_insights_default_engine_hits_browser_and_agent_without_catalog_injection() {
    let home = Temp::new();
    let exe = app(&home.0, "Google Chrome", "com.google.Chrome");
    write(
        &home.0,
        "Library/Application Support/Google/Chrome/Local State",
        "{}",
    );
    let r = report(
        &home.0,
        vec![
            proc(930001, 1, "Chrome", &exe, &[], Some(100)),
            proc(930002, 1, "claude", "/public/claude", &[], Some(4294967296)),
            proc(
                930003,
                1,
                "node",
                "/public/node",
                &["--message", "vite"],
                Some(4294967296),
            ),
        ],
        Language::En,
    );
    assert_eq!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&930001))
            .unwrap()
            .insights,
        vec!["chrome.memory-saver-off"]
    );
    assert_eq!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&930002))
            .unwrap()
            .insights,
        vec!["claude.large-session"]
    );
    assert!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&930003))
            .unwrap()
            .insights
            .is_empty()
    );
    assert!(!r.issues.iter().any(|i| i.stage == "insights"));
}

#[test]
fn phase03_mcp_trusted_entries_do_not_scan_arbitrary_arguments() {
    use whoate_core::recognizers::coding_agents::mcp_name;
    for (comm, args, expected) in [
        ("uvx", vec!["mcp-server-fetch"], Some("mcp-server-fetch")),
        (
            "python3.13",
            vec!["-m", "mcp_server_fetch"],
            Some("mcp-server-fetch"),
        ),
        (
            "docker",
            vec!["run", "-i", "--rm", "mcp/fetch"],
            Some("mcp/fetch"),
        ),
        (
            "docker",
            vec!["run", "-i", "--rm", "mcp/filesystem", "/projects"],
            Some("mcp/filesystem"),
        ),
        (
            "node",
            vec!["/project/node_modules/@modelcontextprotocol/server-filesystem/dist/index.js"],
            Some("@modelcontextprotocol/server-filesystem"),
        ),
        ("node", vec!["/project/index.js", "example-mcp"], None),
        ("uvx", vec!["ordinary-package", "mcp-server-fetch"], None),
        (
            "python3",
            vec!["ordinary.py", "-m", "mcp_server_fetch"],
            None,
        ),
        ("sh", vec!["-c", "uvx mcp-server-fetch"], None),
        ("docker", vec!["inspect", "mcp/fetch"], None),
        (
            "docker",
            vec!["run", "--name", "mcp/fetch", "ordinary"],
            None,
        ),
        (
            "node",
            vec![
                "/project/node_modules/ordinary/index.js",
                "mcp-server-fetch",
            ],
            None,
        ),
    ] {
        let p = proc(930001, 1, comm, &format!("/public/{comm}"), &args, Some(1));
        assert_eq!(mcp_name(&p).as_deref(), expected, "{comm} {args:?}");
    }
}

#[test]
fn phase03_holdout_mixed_agent_owner_reserves_mcp_and_specialized_parts_in_both_languages() {
    use whoate_core::snapshot::TmuxPane;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/recognizers/mixed-holdout.json"
    ))
    .unwrap();
    for language in [Language::En, Language::Zh] {
        let home = Temp::new();
        let chrome = app(&home.0, "Google Chrome", "com.google.Chrome");
        let code = app(&home.0, "Visual Studio Code", "com.microsoft.VSCode");
        let ps: Vec<_> = fixture["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let exe = match row[3].as_str().unwrap() {
                    "chrome" => chrome.as_str(),
                    "code" => code.as_str(),
                    other => other,
                };
                let args: Vec<_> = row[4]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                proc(
                    row[0].as_i64().unwrap() as i32,
                    row[1].as_i64().unwrap() as i32,
                    row[2].as_str().unwrap(),
                    exe,
                    &args,
                    row[5].as_u64(),
                )
            })
            .collect();
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), ps);
        snap.tmux_panes.push(TmuxPane {
            pane_title: None,
            pane_id: "%holdout".into(),
            target: "test:1.0".into(),
            window_name: None,
            pane_pid: 940001,
            cwd: None,
        });
        let original = snap.clone();
        let r = report_snapshot(&home.0, snap.clone(), language);
        assert_eq!(r.owners.len(), 1);
        let owner = &r.owners[0];
        assert_eq!(
            owner.pids,
            vec![
                940001, 940002, 940003, 940004, 940005, 940006, 940007, 940008
            ]
        );
        assert_eq!(owner.mem_bytes, 280);
        assert_eq!(owner.unknown_mem_count, 1);
        for (key, labels) in [
            ("mcp", ("MCP: mcp-server-fetch", "MCP: mcp-server-fetch")),
            ("tabs", ("Tab processes", "标签页进程")),
            ("extension_hosts", ("Extension hosts", "扩展宿主")),
            (
                "languages",
                ("TypeScript language server", "TypeScript 语言服务器"),
            ),
        ] {
            let expected: Vec<_> = fixture["parts"][key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let name = language.choose(labels.1, labels.0);
            assert_eq!(
                owner
                    .parts
                    .iter()
                    .find(|p| p.name == name)
                    .map(|p| p.pids.clone()),
                Some(expected)
            );
        }
        let members: Vec<_> = owner.parts.iter().flat_map(|p| p.pids.clone()).collect();
        assert_eq!(members.len(), 8);
        assert_eq!(
            members
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
        assert_eq!(owner.parts.iter().map(|p| p.mem_bytes).sum::<u64>(), 280);
        assert!(owner.insights.contains(&"claude.mcp".into()));
        snap.procs.reverse();
        let shuffled = report_snapshot(&home.0, snap, language);
        assert_eq!(shuffled.owners, r.owners);
        assert_eq!(original.procs[2].argv.last().unwrap(), "--type=renderer");
    }
}

#[test]
fn phase03_vscode_jsonc_settings_are_scoped_and_only_expose_whitelisted_values() {
    use whoate_core::model::FactValue;
    let home = Temp::new();
    let exe = app(&home.0, "Visual Studio Code", "com.microsoft.VSCode");
    write(
        &home.0,
        "Library/Application Support/Code/User/settings.json",
        "{ // user\n\"python.analysis.languageServerMode\":\"light\",\"files.watcherExclude\":{\"**/dist/**\":true,},\"private.url\":\"SECRET_SENTINEL\",}",
    );
    write(
        &home.0,
        "workspace/.vscode/settings.json",
        r#"{"python.analysis.languageServerMode":"full","files.watcherExclude":{"**/dist/**":false,"**/build/**":true},}"#,
    );
    let mut p = proc(930001, 1, "Code", &exe, &[], Some(100));
    p.cwd = Some(home.0.join("workspace").to_string_lossy().into());
    let r = report(&home.0, vec![p.clone()], Language::En);
    for (key, expected) in [
        (
            "vscode.user.language_server_mode",
            FactValue::Text("light".into()),
        ),
        (
            "vscode.workspace.language_server_mode",
            FactValue::Text("full".into()),
        ),
        ("vscode.user.watcher_exclusion_count", FactValue::Count(1)),
        (
            "vscode.workspace.watcher_exclusion_count",
            FactValue::Count(1),
        ),
    ] {
        assert_eq!(
            r.owners[0]
                .facts
                .iter()
                .find(|f| f.key == key)
                .map(|f| f.value.clone()),
            Some(expected)
        );
    }
    assert!(
        !serde_json::to_string(&r)
            .unwrap()
            .contains("SECRET_SENTINEL")
    );
    write(
        &home.0,
        "workspace/.vscode/settings.json",
        "broken-SECRET_SENTINEL",
    );
    let r = report(&home.0, vec![p], Language::En);
    assert!(
        r.owners[0]
            .facts
            .iter()
            .any(|f| f.key == "config_issue.vscode.workspace")
    );
    assert!(
        !r.owners[0]
            .facts
            .iter()
            .any(|f| f.key == "vscode.workspace.language_server_mode")
    );
    assert!(
        !serde_json::to_string(&r)
            .unwrap()
            .contains("SECRET_SENTINEL")
    );
}

#[test]
fn phase03_user_rules_hot_reload_rejects_cross_member_and_invalid_whole_files() {
    use whoate_core::snapshot::TmuxPane;
    let home = Temp::new();
    let mut snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![
            proc(930001, 1, "mytool", "/public/mytool", &[], Some(100)),
            proc(
                930002,
                930001,
                "child",
                "/public/child",
                &["--child-only"],
                Some(50),
            ),
        ],
    );
    snap.tmux_panes.push(TmuxPane {
        pane_title: None,
        pane_id: "%rules".into(),
        target: "test:1.0".into(),
        window_name: None,
        pane_pid: 930001,
        cwd: None,
    });
    let mut engine = Engine::with_sampler(
        Replay(std::iter::repeat_n(snap, 30).collect()),
        Time,
        Context {
            cfg: ConfigReader::isolated(home.0.clone()),
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let baseline = engine.tick().unwrap().owners[0].clone();
    write(
        &home.0,
        "rules.toml",
        "[[rules]]\nexecutable='mytool'\nargv_regex='--child-only'\nlabel='Wrong AND'\nadvice='Do not run'\n",
    );
    assert_eq!(engine.tick().unwrap().owners[0].label, baseline.label);
    for name in ["Friendly one", "Friendly two longer"] {
        write(
            &home.0,
            "rules.toml",
            &format!("[[rules]]\nexecutable='mytool'\nlabel='{name}'\nadvice='Display only'\n"),
        );
        let report = engine.tick().unwrap();
        let owner = &report.owners[0];
        assert_eq!(owner.label, name);
        assert_eq!(owner.id, baseline.id);
        assert_eq!(owner.kind, baseline.kind);
        assert_eq!(owner.pids, baseline.pids);
        assert_eq!(owner.mem_bytes, 150);
        assert_eq!(owner.evidence, baseline.evidence);
        assert_eq!(owner.verdict, baseline.verdict);
    }
    for bad in [
        "[[rules]]\nexecutable='mytool'\nlabel='Good first'\nadvice='display'\n[[rules]]\nexecutable='child'\nlabel='Bad second'\nadvice='SECRET_RULE_SENTINEL'\nargv_regex='['\n",
        "[[rules]]\nexecutable='mytool'\nlabel='Unknown'\nadvice='SECRET_RULE_SENTINEL'\nunknown_key=true\n",
        "rules = [ SECRET_RULE_SENTINEL",
    ] {
        write(&home.0, "rules.toml", bad);
        let report = engine.tick().unwrap();
        assert_eq!(report.owners[0].label, baseline.label);
        assert!(
            !report.owners[0]
                .facts
                .iter()
                .any(|f| f.key == "user_advice")
        );
        assert!(report.issues.iter().any(|i| i.stage == "user_rules"));
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("SECRET_RULE_SENTINEL")
        );
    }
    fs::remove_file(home.0.join("rules.toml")).unwrap();
    let report = engine.tick().unwrap();
    assert_eq!(report.owners[0].label, baseline.label);
    assert!(!report.issues.iter().any(|i| i.stage == "user_rules"));
}

#[test]
fn phase03_browser_config_two_products_never_share_preferences_or_stale_values() {
    use std::os::unix::fs::PermissionsExt;
    use whoate_core::model::FactValue;
    let home = Temp::new();
    let chrome = app(&home.0, "Google Chrome", "com.google.Chrome");
    let brave = app(&home.0, "Brave Browser", "com.brave.Browser");
    write(
        &home.0,
        "Library/Application Support/Google/Chrome/Local State",
        "{}",
    );
    write(
        &home.0,
        "Library/Application Support/BraveSoftware/Brave-Browser/Local State",
        r#"{"performance_tuning.high_efficiency_mode.state":2}"#,
    );
    let snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![
            proc(930001, 1, "Chrome", &chrome, &[], Some(100)),
            proc(930002, 1, "Brave", &brave, &[], Some(100)),
        ],
    );
    let cfg = ConfigReader::isolated(home.0.clone());
    let reader = cfg.clone();
    let mut engine = Engine::with_sampler(
        Replay(std::iter::repeat_n(snap, 15).collect()),
        Time,
        Context {
            cfg,
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let r = engine.tick().unwrap();
    let chrome = r.owners.iter().find(|o| o.pids.contains(&930001)).unwrap();
    let brave = r.owners.iter().find(|o| o.pids.contains(&930002)).unwrap();
    assert_eq!(chrome.insights, vec!["chrome.memory-saver-off"]);
    assert!(brave.insights.is_empty());
    assert!(
        brave
            .facts
            .iter()
            .any(|f| f.key == "brave.memory_saver_state" && f.value == FactValue::Count(2))
    );
    let reads = reader.disk_reads();
    engine.tick().unwrap();
    assert_eq!(reader.disk_reads(), reads);
    let path = home
        .0
        .join("Library/Application Support/Google/Chrome/Local State");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o000)).unwrap();
    let r = engine.tick().unwrap();
    let chrome = r.owners.iter().find(|o| o.pids.contains(&930001)).unwrap();
    assert!(chrome.insights.is_empty());
    assert!(
        !chrome
            .facts
            .iter()
            .any(|f| f.key == "chrome.memory_saver_state")
    );
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    fs::remove_file(&path).unwrap();
    let r = engine.tick().unwrap();
    assert!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&930001))
            .unwrap()
            .insights
            .is_empty()
    );
    fs::write(
        &path,
        r#"{"performance_tuning.high_efficiency_mode.state":1}"#,
    )
    .unwrap();
    let r = engine.tick().unwrap();
    assert_eq!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&930001))
            .unwrap()
            .insights,
        vec!["chrome.memory-saver-legacy"]
    );
    assert!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&930002))
            .unwrap()
            .insights
            .is_empty()
    );
}

#[test]
fn phase03_venv_relative_entry_needs_resolvable_cwd_and_cannot_escape_isolation() {
    let home = Temp::new();
    let outside = Temp::new();
    write(&home.0, "project/.venv/pyvenv.cfg", "home=/public/base\n");
    write(&outside.0, ".venv/pyvenv.cfg", "SECRET_VENV_SENTINEL");
    fs::create_dir_all(home.0.join("different-cwd")).unwrap();
    std::os::unix::fs::symlink(outside.0.join(".venv"), home.0.join("escape")).unwrap();
    for (entry, cwd) in [
        (".venv/bin/python3", home.0.join("different-cwd")),
        ("escape/bin/python3", home.0.clone()),
        ("../../outside/.venv/bin/python3", home.0.clone()),
    ] {
        let mut p = proc(930001, 1, "python3", "/public/python3", &[], Some(10));
        p.argv = vec![entry.into()];
        p.cwd = Some(cwd.to_string_lossy().into());
        let report = report(&home.0, vec![p], Language::En);
        assert!(!report.owners[0].facts.iter().any(|f| f.key == "venv"));
        assert!(
            !serde_json::to_string(&report)
                .unwrap()
                .contains("SECRET_VENV_SENTINEL")
        );
    }
}

#[test]
fn phase03_jetbrains_unknown_version_never_selects_latest_config_and_is_observable() {
    let home = Temp::new();
    let exe = app(&home.0, "PyCharm", "com.jetbrains.pycharm.ce");
    write(
        &home.0,
        "Library/Application Support/JetBrains/PyCharmCE2099.1/pycharm.vmoptions",
        "-Xmx64g\n",
    );
    let r = report(
        &home.0,
        vec![proc(930001, 1, "pycharm", &exe, &[], Some(100))],
        Language::En,
    );
    assert!(
        !r.owners[0]
            .facts
            .iter()
            .any(|f| f.key == "jvm_config_xmx_bytes")
    );
    assert!(
        r.owners[0]
            .facts
            .iter()
            .any(|f| f.key == "config_issue.jetbrains")
    );
}

#[test]
fn phase03_mcp_ancestry_stops_at_membership_boundary_and_cycles_terminate() {
    use whoate_core::{
        model::{Agent, Owner, OwnerKind},
        recognizers::{Recognizer, coding_agents::CodingAgents},
    };
    let home = Temp::new();
    let snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![
            proc(
                930001,
                1,
                "uvx",
                "/public/uvx",
                &["mcp-server-fetch"],
                Some(10),
            ),
            proc(930002, 930001, "worker", "/public/worker", &[], Some(20)),
            proc(
                930003,
                930004,
                "node",
                "/public/node",
                &["ordinary.js", "mcp-server-fetch"],
                Some(30),
            ),
            proc(930004, 930003, "worker", "/public/worker", &[], None),
            proc(930005, 999999, "worker", "/public/worker", &[], Some(50)),
        ],
    );
    let mut owner = Owner::new("agent:test".into(), OwnerKind::AgentSession, "Test".into());
    owner.agent = Some(Agent::unknown("claude"));
    owner.pids = vec![930002, 930003, 930004, 930005];
    CodingAgents.explain(&mut owner, &snap, &ConfigReader::isolated(home.0.clone()));
    assert!(!owner.parts.iter().any(|p| p.name.starts_with("MCP:")));
    assert_eq!(
        owner
            .parts
            .iter()
            .flat_map(|p| p.pids.clone())
            .collect::<Vec<_>>(),
        vec![930002, 930003, 930004, 930005]
    );
    owner.parts.clear();
    owner.pids = vec![930001, 930002];
    CodingAgents.explain(&mut owner, &snap, &ConfigReader::isolated(home.0.clone()));
    assert_eq!(owner.parts[0].name, "MCP: mcp-server-fetch");
    assert_eq!(owner.parts[0].pids, vec![930001, 930002]);
    assert_eq!(owner.parts[0].mem_bytes, 30);
}

#[test]
fn phase03_reader_unmatched_software_does_not_read_browser_ide_or_docker_configs() {
    let home = Temp::new();
    for path in [
        "Library/Application Support/Google/Chrome/Local State",
        "Library/Application Support/Code/User/settings.json",
        "Library/Group Containers/group.com.docker/settings-store.json",
    ] {
        write(&home.0, path, "UNMATCHED_CONFIG_SECRET");
    }
    let cfg = ConfigReader::isolated(home.0.clone());
    let reader = cfg.clone();
    let snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![proc(930001, 1, "demo", "/public/demo", &[], Some(10))],
    );
    let r = Engine::with_sampler(
        Replay(VecDeque::from([snap.clone(), snap])),
        Time,
        Context {
            cfg,
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    )
    .sample_once()
    .unwrap();
    assert_eq!(reader.disk_reads(), 0);
    assert!(r.owners[0].insights.is_empty());
    assert!(
        !serde_json::to_string(&r)
            .unwrap()
            .contains("UNMATCHED_CONFIG_SECRET")
    );
}

#[test]
fn phase03_insights_runtime_signals_come_from_entries_not_argument_mentions() {
    let home = Temp::new();
    for (comm, args, expected) in [
        (
            "node",
            vec!["/p/node_modules/.bin/vite"],
            vec!["vite.profile", "vite.imports"],
        ),
        (
            "node",
            vec!["/p/node_modules/next/dist/bin/next", "dev"],
            vec!["next.inspect", "next.webpack-memory"],
        ),
        (
            "node",
            vec!["/p/node_modules/.bin/webpack"],
            vec!["webpack.pathinfo", "webpack.source-maps"],
        ),
        (
            "node",
            vec!["/p/node_modules/.bin/wrangler", "dev"],
            vec!["wrangler.snapshot", "wrangler.compare"],
        ),
        (
            "node",
            vec!["/p/typescript/lib/tsserver.js"],
            vec!["tsserver.files", "tsserver.references"],
        ),
        (
            "python3.13",
            vec!["-m", "ipykernel_launcher", "-f", "/public/kernel.json"],
            vec!["python.output-cache", "python.cache-limit"],
        ),
        (
            "python3.13",
            vec!["ordinary.py", "-m", "ipykernel_launcher"],
            vec![],
        ),
        (
            "node",
            vec![
                "ordinary.js",
                "vite",
                "next",
                "webpack",
                "wrangler",
                "tsserver.js",
            ],
            vec![],
        ),
        (
            "node",
            vec!["/p/node_modules/.bin/wrangler", "--help"],
            vec![],
        ),
        ("sh", vec!["-c", "node /p/node_modules/.bin/vite"], vec![]),
    ] {
        let p = proc(
            930001,
            1,
            comm,
            &format!("/public/{comm}"),
            &args,
            Some(8589934592),
        );
        assert_eq!(
            report(&home.0, vec![p], Language::En).owners[0].insights,
            expected,
            "{comm} {args:?}"
        );
    }
    let mut next = proc(930001, 1, "node", "/public/node", &[], Some(8589934592));
    next.argv = vec!["next-server (v15.2.0)".into()];
    assert_eq!(
        report(&home.0, vec![next], Language::En).owners[0].insights,
        vec!["next.inspect", "next.webpack-memory"]
    );
}

#[test]
fn phase03_holdout_frozen_unseen_brave_docker_mcp_and_ide_keep_independent_oracle() {
    use whoate_core::snapshot::TmuxPane;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/recognizers/frozen-unseen.json"
    ))
    .unwrap();
    for language in [Language::En, Language::Zh] {
        let home = Temp::new();
        let brave = app(&home.0, "Brave Browser", "com.brave.Browser");
        let code = app(&home.0, "Visual Studio Code", "com.microsoft.VSCode");
        let ps = fixture["rows"]
            .as_array()
            .unwrap()
            .iter()
            .map(|row| {
                let exe = match row[3].as_str().unwrap() {
                    "brave" => brave.as_str(),
                    "code" => code.as_str(),
                    other => other,
                };
                let args: Vec<_> = row[4]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_str().unwrap())
                    .collect();
                proc(
                    row[0].as_i64().unwrap() as i32,
                    row[1].as_i64().unwrap() as i32,
                    row[2].as_str().unwrap(),
                    exe,
                    &args,
                    row[5].as_u64(),
                )
            })
            .collect();
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), ps);
        snap.tmux_panes.push(TmuxPane {
            pane_title: None,
            pane_id: "%unseen".into(),
            target: "test:2.0".into(),
            window_name: None,
            pane_pid: 960100,
            cwd: None,
        });
        let report = report_snapshot(&home.0, snap.clone(), language);
        assert_eq!(report.owners.len(), 1);
        let owner = &report.owners[0];
        assert_eq!(
            owner.pids,
            vec![
                960100, 960110, 960111, 960120, 960130, 960131, 960140, 960150
            ]
        );
        assert_eq!(owner.mem_bytes, 308);
        assert_eq!(owner.unknown_mem_count, 1);
        for (key, en, zh, unknown) in [
            ("mcp", "MCP: mcp/filesystem", "MCP: mcp/filesystem", 0),
            ("tabs", "Tab processes", "标签页进程", 1),
            ("watchers", "File watchers", "文件监视", 0),
            (
                "languages",
                "TypeScript language server",
                "TypeScript 语言服务器",
                0,
            ),
        ] {
            let pids: Vec<_> = fixture["parts"][key]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_i64().unwrap() as i32)
                .collect();
            let part = owner
                .parts
                .iter()
                .find(|p| p.name == language.choose(zh, en));
            assert_eq!(part.map(|p| p.pids.clone()), Some(pids));
            assert_eq!(part.map(|p| p.unknown_mem_count), Some(unknown));
        }
        let members: Vec<_> = owner.parts.iter().flat_map(|p| p.pids.clone()).collect();
        assert_eq!(members.len(), 8);
        assert_eq!(
            members
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            8
        );
        assert_eq!(owner.parts.iter().map(|p| p.mem_bytes).sum::<u64>(), 308);
        assert_eq!(
            owner
                .insights
                .iter()
                .map(String::as_str)
                .collect::<std::collections::BTreeSet<_>>(),
            std::collections::BTreeSet::from(["opencode.mcp", "vscode.watchers"])
        );
        snap.procs.reverse();
        assert_eq!(
            report_snapshot(&home.0, snap, language).owners,
            report.owners
        );
    }
}

#[test]
fn phase03_insights_internal_counts_are_not_public_software_facts() {
    let home = Temp::new();
    let mut system = proc(
        970002,
        1,
        "system-demo",
        "/public/system-demo",
        &[],
        Some(10),
    );
    system.uid = 0;
    let r = report(
        &home.0,
        vec![
            proc(970001, 1, "demo", "/public/demo", &[], Some(10)),
            system,
        ],
        Language::En,
    );
    for pid in [970001, 970002] {
        let owner = r.owners.iter().find(|o| o.pids.contains(&pid)).unwrap();
        assert!(
            owner.facts.is_empty(),
            "unmatched pid {pid} has public facts"
        );
    }
    let chrome = app(&home.0, "Google Chrome", "com.google.Chrome");
    write(
        &home.0,
        "Library/Application Support/Google/Chrome/Local State",
        "{}",
    );
    let r = report(
        &home.0,
        vec![
            proc(970003, 1, "Chrome", &chrome, &[], Some(100)),
            proc(
                970004,
                1,
                "node",
                "/public/node",
                &["/p/node_modules/.bin/vite"],
                Some(1073741824),
            ),
        ],
        Language::En,
    );
    assert_eq!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&970003))
            .unwrap()
            .insights,
        vec!["chrome.memory-saver-off"]
    );
    assert_eq!(
        r.owners
            .iter()
            .find(|o| o.pids.contains(&970004))
            .unwrap()
            .insights,
        vec!["vite.profile", "vite.imports"]
    );
    assert!(
        r.owners
            .iter()
            .all(|o| o.facts.iter().all(|f| !f.key.starts_with("count.")))
    );
    assert!(!r.issues.iter().any(|i| i.stage == "insights"));
}

#[test]
fn phase03_webkit_enhanced_security_is_web_content_only_for_system_framework() {
    let home = Temp::new();
    let exe = app(&home.0, "Safari", "com.apple.Safari");
    let mut enhanced = proc(
        970102,
        1,
        "com.apple.WebKit.WebContent.EnhancedSecurity",
        "/System/Library/Frameworks/WebKit.framework/Versions/A/XPCServices/com.apple.WebKit.WebContent.EnhancedSecurity.xpc/Contents/MacOS/com.apple.WebKit.WebContent.EnhancedSecurity",
        &[],
        Some(20),
    );
    enhanced.responsible_pid = Some(970101);
    let mut fake = proc(
        970103,
        1,
        "com.apple.WebKit.WebContent.EnhancedSecurity",
        "/untrusted/com.apple.WebKit.WebContent.EnhancedSecurity",
        &[],
        Some(30),
    );
    fake.responsible_pid = Some(970101);
    let r = report(
        &home.0,
        vec![
            proc(970101, 1, "Safari", &exe, &[], Some(10)),
            enhanced,
            fake,
        ],
        Language::En,
    );
    let owner = r.owners.iter().find(|o| o.pids.contains(&970101)).unwrap();
    assert_eq!(
        owner
            .parts
            .iter()
            .find(|p| p.name == "Web content")
            .map(|p| p.pids.clone()),
        Some(vec![970102])
    );
    assert_eq!(owner.parts.iter().map(|p| p.mem_bytes).sum::<u64>(), 20);
    assert!(!owner.parts.iter().any(|p| p.pids.contains(&970103)));
    assert_eq!(owner.mem_bytes, 60);
}

#[test]
fn phase03_user_rules_explicit_whoate_rules_outside_home_is_a_single_file_capability() {
    use whoate_core::model::FactValue;
    let home = Temp::new();
    let external = Temp::new();
    let path = external.0.join("explicit-rules.toml");
    write(
        &home.0,
        "rules.toml",
        "[[rules]]\nexecutable='mytool'\nlabel='Wrong home fallback'\nadvice='Wrong'\n",
    );
    fs::write(
        &path,
        "[[rules]]\nexecutable='mytool'\nlabel='External label'\nadvice='External display only'\n",
    )
    .unwrap();
    let original = fs::read(&path).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let cfg = ConfigReader::from_lookup(|key| match key {
        "WHOATE_HOME_OVERRIDE" => Some(home.0.clone().into_os_string()),
        "WHOATE_CACHE_DIR" => Some(home.0.join("cache").into_os_string()),
        "WHOATE_RULES" => Some(path.clone().into_os_string()),
        _ => None,
    })
    .unwrap();
    let reader = cfg.clone();
    let snap = Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![proc(970201, 1, "mytool", "/public/mytool", &[], Some(100))],
    );
    let mut engine = Engine::with_sampler(
        Replay(std::iter::repeat_n(snap, 12).collect()),
        Time,
        Context {
            cfg,
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let first = engine.tick().unwrap();
    assert_eq!(first.owners[0].label, "External label");
    assert!(
        first.owners[0].facts.iter().any(|f| f.key == "user_advice"
            && f.value == FactValue::Text("External display only".into()))
    );
    assert_eq!(reader.read_text(&path), Err("outside_fixture_root"));
    assert_eq!(fs::read(&path).unwrap(), original);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    fs::write(&path, "[[rules]]\nexecutable='mytool'\nlabel='Good first'\nadvice='Display'\n[[rules]]\nexecutable='child'\nlabel='Bad'\nadvice='RULES_SECRET_SENTINEL'\nargv_regex='['\n").unwrap();
    let bad = engine.tick().unwrap();
    assert_eq!(bad.owners[0].label, "mytool");
    assert!(
        bad.issues
            .iter()
            .any(|i| i.stage == "user_rules" && i.code == "invalid_regex")
    );
    assert!(
        !serde_json::to_string(&bad)
            .unwrap()
            .contains("RULES_SECRET_SENTINEL")
    );
    fs::write(
        &path,
        "[[rules]]\nexecutable='mytool'\nlabel='Updated external'\nadvice='Display'\n",
    )
    .unwrap();
    assert_eq!(engine.tick().unwrap().owners[0].label, "Updated external");
    fs::remove_file(&path).unwrap();
    let missing = engine.tick().unwrap();
    assert_eq!(missing.owners[0].label, "mytool");
    assert!(!missing.issues.iter().any(|i| i.stage == "user_rules"));
}

#[test]
fn phase03_vscode_node_heap_flags_and_electron_host_do_not_hide_language_servers() {
    let home = Temp::new();
    let exe = app(&home.0, "Visual Studio Code", "com.microsoft.VSCode");
    let helper = home.0.join("Visual Studio Code.app/Contents/Frameworks/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)").to_string_lossy().into_owned();
    let r = report(
        &home.0,
        vec![
            proc(970301, 1, "Code", &exe, &[], Some(10)),
            proc(
                970302,
                970301,
                "node",
                "/public/node",
                &[
                    "--max-old-space-size=3072",
                    "/public/typescript/lib/tsserver.js",
                ],
                Some(536870912),
            ),
            proc(
                970303,
                970301,
                "Code Helper (Plugin)",
                &helper,
                &[
                    "--max-old-space-size=3072",
                    "/public/typescript/lib/tsserver.js",
                ],
                Some(536870912),
            ),
            proc(
                970304,
                970301,
                "Code Helper (Plugin)",
                &helper,
                &[
                    "--max-old-space-size=8192",
                    "/public/.vscode/extensions/ms-python.vscode-pylance-2026.2.1/dist/server.bundle.js",
                ],
                Some(20),
            ),
            proc(
                970305,
                970301,
                "node",
                "/public/node",
                &["ordinary.js", "/public/typescript/lib/tsserver.js"],
                Some(0),
            ),
        ],
        Language::En,
    );
    let owner = r.owners.iter().find(|o| o.pids.contains(&970301)).unwrap();
    assert_eq!(
        owner
            .parts
            .iter()
            .find(|p| p.name == "TypeScript language server")
            .map(|p| p.pids.clone()),
        Some(vec![970302, 970303])
    );
    assert_eq!(
        owner
            .parts
            .iter()
            .find(|p| p.name == "Pylance")
            .map(|p| p.pids.clone()),
        Some(vec![970304])
    );
    assert_eq!(
        owner.insights,
        vec!["tsserver.files", "tsserver.references"]
    );
    assert!(
        !owner
            .parts
            .iter()
            .filter(|p| p.name == "TypeScript language server")
            .any(|p| p.pids.contains(&970305))
    );
    assert_eq!(owner.mem_bytes, 1073741854);
}

#[test]
fn phase03_chromium_archive_replay_keeps_real_chrome_and_brave_pid_sets() {
    let home = Temp::new();
    let snap: Snapshot = serde_json::from_str(include_str!(
        "../../../tests/fixtures/capture/snapshot-20261014-1940.json"
    ))
    .unwrap();
    let r = report_snapshot(&home.0, snap, Language::En);
    // Independent read-only enumeration of the frozen capture before running
    // this test: Chrome 15/6/1 and Brave 15/6/1, not production classification.
    for (name, chrome, brave) in [
        (
            "Tab processes",
            vec![
                6581, 6585, 6628, 13843, 13917, 24692, 33915, 44241, 51696, 56112, 62735, 63541,
                77287, 89986, 90027,
            ],
            vec![
                4009, 4017, 5306, 7218, 24305, 30098, 37887, 51615, 79257, 83829, 84480, 84506,
                84855, 86264, 88131,
            ],
        ),
        (
            "Extensions",
            vec![6574, 6587, 6588, 33175, 33179, 33180],
            vec![35457, 74476, 83796, 83806, 83807, 83815],
        ),
        ("GPU", vec![6564], vec![83789]),
    ] {
        let mut expected: Vec<_> = chrome.into_iter().chain(brave).collect();
        expected.sort_unstable();
        let mut actual: Vec<_> = r
            .owners
            .iter()
            .flat_map(|o| &o.parts)
            .filter(|p| p.name == name)
            .flat_map(|p| p.pids.clone())
            .collect();
        actual.sort_unstable();
        assert_eq!(actual, expected, "real snapshot {name}");
    }
}
