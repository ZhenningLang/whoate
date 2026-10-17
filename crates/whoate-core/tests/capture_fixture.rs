use whoate_core::{
    sample::fixture,
    snapshot::{Proc, Snapshot, TmuxPane},
};

#[test]
fn docker_public_identifiers_survive_positional_and_switch_arguments() {
    let mut proc = Proc::identity(7, 1, 501, Some(1), "node".into());
    proc.argv = vec![
        "node".into(),
        "com.docker.backend".into(),
        "--service=com.docker.vmnetd".into(),
        "--token=PRIVATE_SENTINEL".into(),
    ];
    let output = fixture::redact(Snapshot::historical(
        "2026-10-14T19:40:08+08:00".into(),
        vec![proc],
    ));
    fixture::validate_safe(&output).unwrap();
    assert_eq!(
        output.procs[0].argv,
        [
            "node",
            "com.docker.backend",
            "--service=com.docker.vmnetd",
            "--token=<redacted>"
        ]
    );
}

#[test]
fn historical_application_executable_does_not_absorb_positional_private_arguments() {
    let raw = serde_json::json!({
        "taken_at": "2026-10-14T19:40:08+08:00", "tmux_panes": [],
        "procs": [{"pid":42,"ppid":1,"uid":501,"start":"Wed Oct 14 19:00:00 2026",
            "command":"/Applications/Unlisted Editor.app/Contents/MacOS/plugin_host 123 /Users/person/PRIVATE_SENTINEL true /Applications/Unlisted Editor.app/Contents/MacOS/editor",
            "footprint_bytes":100,"cpu_time_ns":200,"tmux_pane":null}]
    });
    let imported = fixture::import_archive(&serde_json::to_vec(&raw).unwrap()).unwrap();
    assert_eq!(
        imported.procs[0].exe_path.as_deref(),
        Some("/Applications/Unlisted Editor.app/Contents/MacOS/plugin_host")
    );
    assert_eq!(imported.procs[0].argv[1], "123");
    let output = fixture::redact(imported);
    fixture::validate_safe(&output).unwrap();
    assert!(
        !serde_json::to_string(&output)
            .unwrap()
            .contains("PRIVATE_SENTINEL")
    );
}

#[test]
fn public_paths_application_markers_and_runtime_signals_survive_redaction() {
    let mut proc = Proc::identity(7, 1, 0, Some(1), "com.docker.backend".into());
    proc.exe_path = Some("/Applications/Unlisted Editor.app/Contents/MacOS/Unlisted Editor".into());
    proc.cwd = Some("/".into());
    proc.argv = vec![
        "/usr/libexec/UnlistedDaemon".into(),
        "/System/Library/Frameworks/WebKit.framework/WebKit".into(),
        "/Library/Application Support/PublicTool/tool".into(),
        "/opt/homebrew/bin/node".into(),
        "/Users/other/private-project/node_modules/vite/bin/vite.js".into(),
        "next".into(),
        "wrangler".into(),
        "tsserver".into(),
        "jupyter".into(),
        "kernel".into(),
    ];
    let output = fixture::redact(Snapshot::historical(
        "2026-10-14T19:40:08+08:00".into(),
        vec![proc],
    ));
    fixture::validate_safe(&output).unwrap();
    let proc = &output.procs[0];
    assert_eq!(proc.comm, "com.docker.backend");
    assert_eq!(proc.cwd.as_deref(), Some("/"));
    assert_eq!(
        proc.exe_path.as_deref(),
        Some("/Applications/Unlisted Editor.app/Contents/MacOS/Unlisted Editor")
    );
    assert_eq!(proc.argv[0], "/usr/libexec/UnlistedDaemon");
    assert_eq!(
        proc.argv[1],
        "/System/Library/Frameworks/WebKit.framework/WebKit"
    );
    assert_eq!(proc.argv[2], "/Library/Application Support/PublicTool/tool");
    assert_eq!(proc.argv[3], "/opt/homebrew/bin/node");
    assert!(proc.argv[4].starts_with("~/"));
    assert!(proc.argv[4].ends_with("/node_modules/vite/bin/vite.js"));
    assert!(!proc.argv[4].contains("private-project"));
    assert_eq!(
        &proc.argv[5..],
        ["next", "wrangler", "tsserver", "jupyter", "kernel"]
    );
}

#[test]
fn home_application_and_virtualenv_structure_keep_private_directory_relations() {
    let mut proc = Proc::identity(7, 1, 501, Some(1), "node".into());
    proc.exe_path =
        Some("/Users/person/PRIVATE_PROJECT/Unlisted Editor.app/Contents/MacOS/node".into());
    proc.cwd = Some("/Users/person/PRIVATE_PROJECT".into());
    proc.argv = vec![
        proc.exe_path.clone().unwrap(),
        "/Users/person/PRIVATE_PROJECT/.venv/bin/python3".into(),
    ];
    let output = fixture::redact(Snapshot::historical(
        "2026-10-14T19:40:08+08:00".into(),
        vec![proc],
    ));
    fixture::validate_safe(&output).unwrap();
    let proc = &output.procs[0];
    let cwd = proc.cwd.as_ref().unwrap();
    assert!(cwd.starts_with("~/"));
    assert_eq!(
        proc.exe_path.as_ref().unwrap(),
        &format!("{cwd}/Unlisted Editor.app/Contents/MacOS/node")
    );
    assert_eq!(proc.argv[1], format!("{cwd}/.venv/bin/python3"));
    assert!(
        !serde_json::to_string(&output)
            .unwrap()
            .contains("PRIVATE_PROJECT")
    );
}

#[test]
fn historical_multiword_app_name_keeps_only_the_executable_in_argv_zero() {
    for (exe, tail) in [
        (
            "/Applications/Unlisted Editor.app/Contents/MacOS/Unlisted Editor",
            " 123 PRIVATE_SENTINEL",
        ),
        (
            "/Applications/Google Chrome.app/Contents/Frameworks/Helper.app/Contents/MacOS/Google Chrome Helper (GPU)",
            " --type=gpu-process",
        ),
    ] {
        let raw = serde_json::json!({
            "taken_at":"2026-10-14T19:40:08+08:00", "tmux_panes":[],
            "procs":[{"pid":42,"ppid":1,"uid":501,"start":"Wed Oct 14 19:00:00 2026",
                "command":format!("{exe}{tail}"),"footprint_bytes":100,"cpu_time_ns":200,"tmux_pane":null}]
        });
        let imported = fixture::import_archive(&serde_json::to_vec(&raw).unwrap()).unwrap();
        assert_eq!(imported.procs[0].exe_path.as_deref(), Some(exe));
        assert_eq!(imported.procs[0].argv[0], exe);
        assert!(imported.procs[0].argv.len() > 1);
        let output = fixture::redact(imported);
        fixture::validate_safe(&output).unwrap();
        assert!(
            !serde_json::to_string(&output)
                .unwrap()
                .contains("PRIVATE_SENTINEL")
        );
    }
}

#[test]
fn all_strings_are_scrubbed_but_switch_positions_and_public_signals_survive() {
    let mut proc = Proc::identity(9, 8, 501, Some(1), "PRIVATE_SENTINEL".into());
    proc.argv = vec![
        "/Users/other/private-project/node".into(),
        "--type=renderer".into(),
        "--utility-sub-type=network.mojom.NetworkService".into(),
        "--unknown=PRIVATE_SENTINEL".into(),
        "--extension-process".into(),
        "--token".into(),
        "PRIVATE_SENTINEL".into(),
        "https://private.example/PRIVATE_SENTINEL".into(),
        "/home/other/PRIVATE_SENTINEL".into(),
        "--type".into(),
        "gpu-process".into(),
    ];
    proc.exe_path = Some("/Users/other/private-project/node".into());
    proc.cwd = Some("/Users/other/PRIVATE_SENTINEL".into());
    proc.tmux_pane = Some("%1".into());
    proc.tmux = Some("/private/PRIVATE_SENTINEL".into());
    proc.footprint_bytes = Some(456);
    proc.cpu_time_ns = Some(789);
    let mut snap = Snapshot::historical("2026-10-14T19:40:08+08:00".into(), vec![proc]);
    snap.tmux_panes.push(TmuxPane {
        pane_title: Some(
            "PRIVATE_SENTINEL /Users/other/private-project token=PRIVATE_SENTINEL".into(),
        ),
        pane_id: "%1".into(),
        target: "PRIVATE_SENTINEL:1.0".into(),
        window_name: Some("PRIVATE_SENTINEL".into()),
        pane_pid: 8,
        cwd: Some("/Users/other/PRIVATE_SENTINEL".into()),
    });
    let snap = fixture::redact(snap);
    let output = serde_json::to_string(&snap).unwrap();
    for secret in [
        "PRIVATE_SENTINEL",
        "private.example",
        "/Users/",
        "/home/",
        "private-project",
        "top_raw",
        "note",
    ] {
        assert!(!output.contains(secret));
    }
    assert_eq!(snap.procs[0].argv[1], "--type=renderer");
    assert_eq!(
        snap.procs[0].argv[2],
        "--utility-sub-type=network.mojom.NetworkService"
    );
    assert_eq!(snap.procs[0].argv[3], "--unknown=<redacted>");
    assert_eq!(snap.procs[0].argv[4], "--extension-process");
    assert_eq!(snap.procs[0].argv[5], "--token");
    assert_eq!(snap.procs[0].argv[9], "--type");
    assert_eq!(snap.procs[0].argv[10], "gpu-process");
    assert_eq!(
        snap.procs[0].argv[0],
        snap.procs[0].exe_path.as_deref().unwrap()
    );
    assert_eq!(snap.procs[0].cwd, snap.tmux_panes[0].cwd);
    assert_eq!(snap.procs[0].footprint_bytes, Some(456));
    assert_eq!(snap.procs[0].cpu_time_ns, Some(789));
    assert_eq!((snap.procs[0].pid, snap.procs[0].ppid), (9, 8));
    // Check each retained signal before the mandatory whole-export privacy gate.
    fixture::validate_safe(&snap).unwrap();
}

#[test]
fn directory_prefix_relations_survive_nested_private_paths() {
    let mut redactor = fixture::Redactor::default();
    let directory = redactor.path("/Users/other/private-project");
    let script = redactor.path("/Users/other/private-project/server.js");
    assert!(script.starts_with(&(directory.clone() + "/")));
    assert_eq!(directory, redactor.path("/Users/other/private-project"));
    assert_ne!(directory, redactor.path("/Users/another/private-project"));
    assert!(!script.contains("server.js"));
}

#[test]
fn privacy_scanner_rejects_unmasked_text_and_fake_redacted_paths() {
    let mut proc = Proc::identity(1, 0, 501, Some(1), "node".into());
    proc.argv = vec!["node".into(), "--token=UNSEEN_SECRET".into()];
    let mut snap = Snapshot::historical("2026-10-14T19:40:08+08:00".into(), vec![proc]);
    assert!(fixture::validate_safe(&snap).is_err());
    snap.procs[0].argv = vec!["node".into(), "UNSEEN_SECRET".into()];
    assert!(fixture::validate_safe(&snap).is_err());
    snap.procs[0].argv = vec!["node".into()];
    snap.procs[0].cwd = Some("~/redacted/path-0/UNSEEN_SECRET".into());
    assert!(fixture::validate_safe(&snap).is_err());
}

#[test]
fn phase07_fixture_validator_rejects_unredacted_pane_titles() {
    let mut snap = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![]);
    snap.tmux_panes.push(TmuxPane {
        pane_id: "%1".into(),
        target: "session:%1".into(),
        window_name: Some("window".into()),
        pane_title: Some("PRIVATE_SENTINEL /Users/other/private-project token=secret".into()),
        pane_pid: 8,
        cwd: None,
    });
    assert!(fixture::validate_safe(&snap).is_err());
    let safe = fixture::redact(snap);
    fixture::validate_safe(&safe).unwrap();
    assert_eq!(safe.tmux_panes[0].pane_title.as_deref(), Some("<redacted>"));
}

#[test]
fn committed_capture_has_real_required_signals_and_no_raw_payload() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let bytes =
        std::fs::read(root.join("tests/fixtures/capture/snapshot-20261014-1940.json")).unwrap();
    let snap: Snapshot = serde_json::from_slice(&bytes).unwrap();
    fixture::validate_safe(&snap).unwrap();
    assert_eq!(snap.procs.len(), 834);
    let json = serde_json::to_string(&snap).unwrap();
    for signal in [
        "opencode",
        "Google Chrome Helper",
        "com.apple.WebKit",
        "chrome-reading-list-mcp",
    ] {
        assert!(json.contains(signal), "missing public signal {signal}");
    }
    assert!(snap.procs.iter().any(|proc| proc.tmux_pane.is_some()
        && (proc.comm == "claude"
            || proc.comm.bytes().all(|c| c.is_ascii_digit() || c == b'.')
                && proc.comm.contains('.'))));
    assert!(snap.procs.iter().any(|proc| proc.uid == 0));
    assert!(
        snap.procs
            .iter()
            .filter(|proc| proc
                .argv
                .iter()
                .any(|arg| arg.contains("chrome-reading-list-mcp")))
            .count()
            > 1
    );
    assert!(snap.host.is_none());
    assert!(snap.procs.iter().all(|proc| proc.argv_lossy
        && proc.cwd.is_none()
        && proc.responsible_pid.is_none()
        && proc.cpu_pct_sampled.is_none()));
    assert!(!json.contains("top_raw"));
    assert!(!json.contains("/Users/"));
    assert!(snap.procs.iter().any(|proc| {
        proc.exe_path
            .as_deref()
            .is_some_and(|path| path.starts_with("/System/"))
    }));
    assert!(snap.procs.iter().any(|proc| {
        proc.exe_path.as_deref().is_some_and(|path| {
            path.starts_with("/Applications/") && path.contains(".app/Contents/")
        })
    }));
}
