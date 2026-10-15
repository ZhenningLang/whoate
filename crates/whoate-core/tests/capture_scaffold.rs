use std::{path::Path, process::Command};

fn placeholder_only(source: &str, declarations: &[&str]) -> bool {
    let lines: Vec<_> = source
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with("//"))
        .collect();
    lines.len() == declarations.len()
        && declarations
            .iter()
            .all(|declaration| lines.contains(declaration))
}

#[test]
fn phase_one_module_tree_rejects_business_code_and_missing_declarations() {
    // Negative controls exercise this verifier without mutating production files.
    assert!(!placeholder_only("pub fn kill() {}", &[]));
    assert!(!placeholder_only(
        "// pub mod docker;",
        &["pub mod docker;"]
    ));
    assert!(!placeholder_only("pub mod other;", &["pub mod docker;"]));
    assert!(!placeholder_only(
        "pub mod docker;\npub fn explain() {}",
        &["pub mod docker;"]
    ));
    assert!(placeholder_only(
        "//! Placeholder\n\npub mod docker;",
        &["pub mod docker;"]
    ));
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let tree: &[(&str, &[&str])] = &[
        ("model.rs", &[]),
        ("engine.rs", &[]),
        ("kill.rs", &[]),
        ("history.rs", &[]),
        ("swarm.rs", &[]),
        ("attribute/mod.rs", &[]),
        (
            "sources/mod.rs",
            &["pub mod claude_code;", "pub mod kilo;", "pub mod opencode;"],
        ),
        ("sources/claude_code.rs", &[]),
        ("sources/opencode.rs", &[]),
        ("sources/kilo.rs", &[]),
        (
            "recognizers/mod.rs",
            &[
                "pub mod chromium;",
                "pub mod coding_agents;",
                "pub mod dev_runtimes;",
                "pub mod docker;",
                "pub mod ide;",
                "pub mod webkit_host;",
            ],
        ),
        ("recognizers/coding_agents.rs", &[]),
        ("recognizers/dev_runtimes.rs", &[]),
        ("recognizers/chromium.rs", &[]),
        ("recognizers/webkit_host.rs", &[]),
        ("recognizers/docker.rs", &[]),
        ("recognizers/ide.rs", &[]),
        ("insights/mod.rs", &[]),
        ("verdict/mod.rs", &["pub mod rules;"]),
        ("verdict/rules.rs", &[]),
    ];
    for (path, declarations) in tree {
        assert!(root.join(path).is_file(), "missing phase module {path}");
        // P2 is authorized to implement these modules. Preserve the P1-only
        // no-early-business gate against its immutable delivered baseline.
        let output = Command::new("git")
            .current_dir(root.join("../../.."))
            .args([
                "show",
                &format!("7f9c20146457ceec50c30a1dc7a35edb742bb85b:crates/whoate-core/src/{path}"),
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "missing P1 baseline module {path}");
        let content = String::from_utf8(output.stdout).unwrap();
        assert!(
            placeholder_only(&content, declarations),
            "P1 placeholder contains business code or wrong declarations: {path}"
        );
    }
}

#[test]
fn scaffold_locks_all_future_modules_and_dependencies() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = Command::new("cargo")
        .args(["metadata", "--locked", "--no-deps", "--format-version", "1"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(out.status.success());
    let data: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(data["workspace_members"].as_array().unwrap().len(), 3);
    for name in ["whoate-core", "whoate-cli", "whoate-ffi"] {
        assert!(
            data["packages"]
                .as_array()
                .unwrap()
                .iter()
                .any(|p| p["name"] == name)
        );
    }
    let ffi = data["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "whoate-ffi")
        .unwrap();
    assert!(
        ffi["targets"][0]["crate_types"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("staticlib"))
    );
    let core = data["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "whoate-core")
        .unwrap();
    for name in [
        "serde",
        "serde_json",
        "libc",
        "rusqlite",
        "plist",
        "toml",
        "regex",
        "time",
        "jsonschema",
    ] {
        let dep = core["dependencies"]
            .as_array()
            .unwrap()
            .iter()
            .find(|d| d["name"] == name)
            .unwrap();
        assert_eq!(
            dep["kind"].as_str(),
            if name == "jsonschema" {
                Some("dev")
            } else {
                None
            }
        );
        let required: &[&str] = match name {
            "rusqlite" => &["bundled"],
            "serde" => &["derive"],
            "time" => &["local-offset", "formatting", "parsing", "serde"],
            _ => &[],
        };
        for feature in required {
            assert!(
                dep["features"]
                    .as_array()
                    .unwrap()
                    .contains(&serde_json::json!(feature))
            );
        }
    }
    let cli = data["packages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "whoate-cli")
        .unwrap();
    assert!(cli["dependencies"].as_array().unwrap().iter().any(|d| {
        d["name"] == "clap"
            && d["features"]
                .as_array()
                .unwrap()
                .contains(&serde_json::json!("derive"))
    }));
    let source = std::fs::read_to_string(root.join("crates/whoate-core/src/lib.rs")).unwrap();
    for module in [
        "model",
        "snapshot",
        "sample",
        "attribute",
        "engine",
        "kill",
        "history",
        "swarm",
        "sources",
        "recognizers",
        "insights",
        "verdict",
    ] {
        assert!(source.contains(&format!("pub mod {module};")));
    }
}
