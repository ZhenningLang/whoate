use std::{collections::BTreeSet, fs};
use whoate_core::{insights, recognizers};
mod common;

#[test]
fn phase03_catalog_embedded_library_covers_all_twelve_software_families() {
    let entries = insights::all();
    assert_eq!(entries.len(), 28);
    let ids: BTreeSet<_> = entries.iter().map(|e| e.id.as_str()).collect();
    assert_eq!(ids.len(), 28);
    for (family, count) in [
        ("claude", 2),
        ("opencode", 2),
        ("vite", 2),
        ("next", 2),
        ("webpack", 2),
        ("wrangler", 2),
        ("tsserver", 2),
        ("python", 2),
        ("chrome", 3),
        ("brave", 3),
        ("vscode", 2),
        ("jetbrains", 2),
        ("docker", 2),
    ] {
        assert_eq!(
            entries
                .iter()
                .filter(|e| e.id.starts_with(&format!("{family}.")))
                .count(),
            count,
            "{family}"
        );
    }
    let recognizers: BTreeSet<_> = recognizers::builtins().iter().map(|r| r.id()).collect();
    for entry in &entries {
        assert!(recognizers.contains(entry.recognizer.as_str()));
        assert!(entry.source.starts_with("https://"));
        assert!(!entry.cause.zh.trim().is_empty());
        assert!(!entry.cause.en.trim().is_empty());
        assert!(!entry.advice.zh.trim().is_empty());
        assert!(!entry.advice.en.trim().is_empty());
        assert!(
            time::Date::parse(
                &entry.checked,
                &time::format_description::well_known::Iso8601::DATE
            )
            .is_ok()
        );
    }
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/insights");
    let disk: BTreeSet<_> = fs::read_dir(dir)
        .unwrap()
        .map(|p| p.unwrap().path())
        .filter(|p| p.extension().is_some_and(|s| s == "toml"))
        .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        disk,
        BTreeSet::from([
            "agents.toml".into(),
            "browsers.toml".into(),
            "desktop.toml".into(),
            "runtimes.toml".into()
        ])
    );
    assert_eq!(
        disk,
        insights::registry::SOURCES
            .iter()
            .map(|(name, _)| name.to_string())
            .collect()
    );
}

#[test]
fn phase03_catalog_rejects_invalid_files_metadata_and_all_trigger_leaves() {
    let (_, original) = insights::registry::SOURCES[0];
    for (from, to, code) in [
        (
            "https://github.com/anthropics/claude-code/issues/18859",
            "http://bad.invalid",
            "invalid_insight_metadata",
        ),
        (
            "checked = \"2026-10-16\"",
            "checked = \"2026-02-30\"",
            "invalid_insight_metadata",
        ),
        ("software_claude", "not_registered", "unknown_count_signal"),
        (
            "recognizer = \"coding-agents\"",
            "recognizer = \"unknown\"",
            "unknown_insight_recognizer",
        ),
        (
            "id = \"claude.mcp\"",
            "id = \"claude.large-session\"",
            "invalid_insight_id",
        ),
        (
            "en = \"The session includes a local MCP subtree; stdio servers consume separate process resources.\"",
            "en = \"   \"",
            "invalid_insight_metadata",
        ),
    ] {
        let bad = original.replacen(from, to, 1);
        assert_ne!(bad, original);
        assert_eq!(
            insights::registry::load_sources(&[("bad.toml", &bad)]),
            Err(code)
        );
    }
    let bad_key =
        insights::registry::SOURCES[1]
            .1
            .replacen("chrome.memory_saver_state", "unknown.key", 1);
    assert_eq!(
        insights::registry::load_sources(&[("bad.toml", &bad_key)]),
        Err("unknown_config_signal")
    );
    assert_eq!(
        insights::registry::load_sources(&[("bad.toml", "entries = [")]),
        Err("invalid_insight_toml")
    );
    assert_eq!(
        insights::registry::load_sources(&[("empty.toml", "entries = []")]),
        Err("empty_insight_catalog")
    );
    assert_eq!(
        insights::registry::load_sources(&[]),
        Err("empty_insight_catalog")
    );
}

#[test]
fn phase03_catalog_embedded_data_is_available_from_an_unrelated_cwd() {
    const MARKER: &str = "WHOATE_P03_CATALOG_CHILD";
    if std::env::var_os(MARKER).is_some() {
        assert_eq!(insights::registry::load().unwrap().len(), 28);
        return;
    }
    let temp = common::Temp::new();
    let result = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "phase03_catalog_embedded_data_is_available_from_an_unrelated_cwd",
            "--nocapture",
        ])
        .env(MARKER, "1")
        .current_dir(&temp.0)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout).contains("1 passed; 0 failed"));
}

#[test]
fn phase03_insights_every_entry_has_positive_negative_boundary_and_software_controls() {
    use whoate_core::{
        model::{Fact, FactValue, Owner, OwnerKind, Part},
        recognizers::ConfigReader,
    };
    // Independent acceptance matrix, not derived from the production triggers.
    let cases = [
        ("claude.large-session", "claude", 4294967296, None, None),
        ("claude.mcp", "claude", 0, Some(("mcp_count", 1)), None),
        ("opencode.large-session", "opencode", 2147483648, None, None),
        ("opencode.mcp", "opencode", 0, Some(("mcp_count", 1)), None),
        ("vite.profile", "vite", 536870912, None, None),
        ("vite.imports", "vite", 1073741824, None, None),
        ("next.inspect", "next", 1073741824, None, None),
        ("next.webpack-memory", "next", 2147483648, None, None),
        ("webpack.pathinfo", "webpack", 536870912, None, None),
        ("webpack.source-maps", "webpack", 1073741824, None, None),
        ("wrangler.snapshot", "wrangler", 536870912, None, None),
        ("wrangler.compare", "wrangler", 1073741824, None, None),
        ("tsserver.files", "tsserver", 536870912, None, None),
        ("tsserver.references", "tsserver", 1073741824, None, None),
        (
            "python.output-cache",
            "python",
            536870912,
            Some(("python_kernel", 1)),
            None,
        ),
        (
            "python.cache-limit",
            "python",
            1073741824,
            Some(("python_kernel", 1)),
            None,
        ),
        (
            "chrome.memory-saver-off",
            "chrome",
            0,
            None,
            Some(("chrome.memory_saver_state", serde_json::json!(0))),
        ),
        (
            "chrome.memory-saver-legacy",
            "chrome",
            0,
            None,
            Some(("chrome.memory_saver_state", serde_json::json!(1))),
        ),
        (
            "chrome.extensions",
            "chrome",
            0,
            Some(("chrome_extensions", 5)),
            None,
        ),
        (
            "brave.memory-saver-off",
            "brave",
            0,
            None,
            Some(("brave.memory_saver_state", serde_json::json!(0))),
        ),
        (
            "brave.memory-saver-legacy",
            "brave",
            0,
            None,
            Some(("brave.memory_saver_state", serde_json::json!(1))),
        ),
        (
            "brave.extensions",
            "brave",
            0,
            Some(("brave_extensions", 5)),
            None,
        ),
        (
            "vscode.extension-bisect",
            "vscode",
            1073741824,
            Some(("vscode_extension_hosts", 1)),
            None,
        ),
        (
            "vscode.watchers",
            "vscode",
            0,
            Some(("vscode_watchers", 1)),
            None,
        ),
        (
            "jetbrains.heap-settings",
            "jetbrains",
            2147483648,
            None,
            None,
        ),
        ("jetbrains.indexing", "jetbrains", 1073741824, None, None),
        (
            "docker.default-memory",
            "docker",
            0,
            None,
            Some(("docker.memory_default", serde_json::json!(true))),
        ),
        (
            "docker.resource-saver",
            "docker",
            0,
            None,
            Some(("docker.resource_saver_disabled", serde_json::json!(true))),
        ),
    ];
    let entries = insights::all();
    assert_eq!(cases.len(), entries.len());
    let temp = common::Temp::new();
    for (id, family, memory, count, config) in cases {
        let entry = entries.iter().find(|e| e.id == id).unwrap();
        let mut owner = Owner::new("test".into(), OwnerKind::Service, "Test".into());
        owner.mem_bytes = memory;
        for signal in insights::registry::COUNT_SIGNALS
            .iter()
            .filter(|s| !matches!(**s, "process_count" | "mcp_count"))
        {
            let n = if *signal == format!("software_{family}") {
                1
            } else if count.is_some_and(|(key, _)| key == *signal) {
                count.unwrap().1
            } else {
                0
            };
            owner.facts.push(Fact {
                key: format!("count.{signal}"),
                label: "Signal".into(),
                value: FactValue::Count(n),
            });
        }
        if count == Some(("mcp_count", 1)) {
            owner.parts.push(Part {
                name: "MCP: test".into(),
                pids: vec![999001],
                process_count: 1,
                mem_bytes: 0,
                unknown_mem_count: 0,
            });
        }
        let mut cfg = ConfigReader::isolated(temp.0.clone());
        for key in insights::registry::CONFIG_SIGNALS {
            cfg.values.insert((*key).into(), serde_json::Value::Null);
        }
        if let Some((key, value)) = &config {
            cfg.values.insert((*key).into(), value.clone());
        }
        assert_eq!(
            insights::evaluate(&entry.trigger, &owner, &cfg),
            Ok(true),
            "positive {id}"
        );
        let mut other = owner.clone();
        other
            .facts
            .iter_mut()
            .find(|f| f.key == format!("count.software_{family}"))
            .unwrap()
            .value = FactValue::Count(0);
        assert_eq!(
            insights::evaluate(&entry.trigger, &other, &cfg),
            Ok(false),
            "other software {id}"
        );
        if memory > 0 {
            let mut below = owner.clone();
            below.mem_bytes = memory - 1;
            assert_eq!(
                insights::evaluate(&entry.trigger, &below, &cfg),
                Ok(false),
                "threshold {id}"
            );
        }
        if let Some((key, _)) = config {
            cfg.values.insert(key.into(), serde_json::Value::Null);
            assert_eq!(
                insights::evaluate(&entry.trigger, &owner, &cfg),
                Ok(false),
                "unknown config {id}"
            );
            cfg.values.insert(key.into(), serde_json::json!(2));
            assert_eq!(
                insights::evaluate(&entry.trigger, &owner, &cfg),
                Ok(false),
                "nonmatching config {id}"
            );
        }
        if let Some((key, n)) = count {
            let mut below = owner.clone();
            if key == "mcp_count" {
                below.parts.clear();
            } else {
                below
                    .facts
                    .iter_mut()
                    .find(|f| f.key == format!("count.{key}"))
                    .unwrap()
                    .value = FactValue::Count(n - 1);
            }
            assert_eq!(
                insights::evaluate(&entry.trigger, &below, &cfg),
                Ok(false),
                "count threshold {id}"
            );
        }
        let mut filtered = owner.clone();
        insights::Catalog::new(vec![entry.clone()])
            .attach("not-matched", &mut filtered, &cfg)
            .unwrap();
        assert!(filtered.insights.is_empty(), "recognizer {id}");
    }
}
