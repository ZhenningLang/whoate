use serde_json::{Value, json};
use std::{path::PathBuf, process::Command};
mod common;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_slice(
        &std::fs::read(root().join("schema/whoate-v1.schema.json")).unwrap(),
    )
    .unwrap();
    jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap()
}
#[test]
fn schema_cli_kill_dry_run_is_one_termination_envelope_without_after_measurement() {
    let home = common::Temp::new();
    let bin = std::env::var_os("WHOATE_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target/debug/whoate"));
    let output = Command::new(bin)
        .args([
            "kill",
            &std::process::id().to_string(),
            "--json",
            "--dry-run",
        ])
        .env("WHOATE_HOME_OVERRIDE", &home.0)
        .env("WHOATE_CACHE_DIR", home.0.join("cache"))
        .env("WHOATE_RULES", home.0.join("rules.toml"))
        .env("TMUX_TMPDIR", &home.0)
        .env_remove("TMUX")
        .env_remove("WHOATE_SESSION_PID")
        .env_remove("WHOATE_TEST_CLAUDE_PID")
        .env_remove("WHOATE_TEST_CLAUDE_BODY_AUTHORIZED")
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "isolated dry-run must succeed"
    );
    let value: Value =
        serde_json::from_slice(&output.stdout).expect("stdout must contain exactly one JSON value");
    assert!(
        validator().is_valid(&value),
        "termination Schema validation failed"
    );
    assert_eq!(value["executed"], false);
    assert_eq!(value["results"][0]["pid"], std::process::id());
    assert_eq!(value["results"][0]["status"], "still_alive");
    assert!(value["results"][0].get("reason").is_some());
    assert_eq!(value["memory"]["after"], Value::Null);
    assert_eq!(value["memory"]["physical_memory_change_bytes"], Value::Null);
    assert!(value["memory"]["before"]["target_resident_bytes"].is_u64());
    assert!(value["memory"]["before"]["measured_at"].is_string());
}

#[test]
fn schema_existing_required_units_and_fixture_bytes_stay_frozen() {
    let baseline = "e0d5440a39b9dc7a6932f45452a6d008fca36e17";
    let previous = Command::new("git")
        .current_dir(root())
        .args(["show", &format!("{baseline}:schema/whoate-v1.schema.json")])
        .output()
        .unwrap();
    assert!(previous.status.success());
    let old: Value = serde_json::from_slice(&previous.stdout).unwrap();
    let new: Value = serde_json::from_slice(
        &std::fs::read(root().join("schema/whoate-v1.schema.json")).unwrap(),
    )
    .unwrap();
    for def in ["report", "owner", "part", "swarm", "fact", "verdict"] {
        assert_eq!(
            old["$defs"][def]["required"], new["$defs"][def]["required"],
            "required changed: {def}"
        );
    }
    for def in ["host", "origin", "agent"] {
        assert_eq!(
            old["$defs"][def]["anyOf"][1]["required"], new["$defs"][def]["anyOf"][1]["required"],
            "required changed: {def}"
        );
    }
    for def in ["uint", "nullableUint", "cpu", "pids", "confidence"] {
        assert_eq!(
            old["$defs"][def], new["$defs"][def],
            "unit/enum changed: {def}"
        );
    }
    let files = Command::new("git")
        .current_dir(root())
        .args(["ls-tree", "-r", "--name-only", baseline, "tests/fixtures"])
        .output()
        .unwrap();
    assert!(files.status.success());
    for file in String::from_utf8(files.stdout).unwrap().lines() {
        let old = Command::new("git")
            .current_dir(root())
            .args(["show", &format!("{baseline}:{file}")])
            .output()
            .unwrap();
        assert!(old.status.success());
        assert_eq!(
            std::fs::read(root().join(file)).unwrap(),
            old.stdout,
            "old fixture modified: {file}"
        );
    }
}

#[test]
fn schema_native_captures_from_the_current_acceptance_batch() {
    let Some(directory) = std::env::var_os("WHOATE_P07_CAPTURES") else {
        assert!(
            validator().is_valid(
                &serde_json::from_str::<Value>(include_str!(
                    "../../../tests/fixtures/attribution/sample-v1.json"
                ))
                .unwrap()
            )
        );
        return;
    };
    let validator = validator();
    let mut count = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "json") {
            continue;
        }
        let data = std::fs::read(path).unwrap();
        let value: Value = serde_json::from_slice(&data).expect("invalid native JSON");
        assert!(validator.is_valid(&value), "native capture Schema invalid");
        count += 1;
    }
    assert!(count > 0, "native capture batch is empty");
    println!("P07_NATIVE_CAPTURE_SCHEMA_OK={count}");
}

#[test]
fn schema_old_fields_and_new_optional_resource_types_are_independent() {
    let legacy: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/attribution/sample-v1.json"
    ))
    .unwrap();
    let check = validator();
    assert!(check.is_valid(&legacy));
    let roundtrip: whoate_core::model::Report = serde_json::from_value(legacy.clone()).unwrap();
    assert_eq!(serde_json::to_value(roundtrip).unwrap(), legacy);
    let mut fresh = legacy.clone();
    fresh["owners"][0]["reclaimable_physical"] = json!({"bytes":4096,"unknown_count":0,"basis":"RSS estimate, not guaranteed","reason":null});
    fresh["owners"][0]["origin"]["pane_title"] = json!("pane");
    fresh["owners"][0]["origin"]["window_name"] = json!("window");
    fresh["owners"][0]["agent"]["last_activity_at"] = json!("2026-10-17T00:00:00Z");
    fresh["owners"][0]["agent"]["idle_seconds"] = json!(7200);
    fresh["host"] = json!({"mem_total_bytes":16384,"mem_used_bytes":8192,"compressed_bytes":1024,"wired_bytes":2048,"swap_used_bytes":0,"pressure":"normal","kernel_zones":{"status":"fresh","as_of":"2026-10-17T00:00:00Z","basis":"elem * inuse","top":[{"name":"zone","bytes":4294967296_u64,"collectible":false,"pageable":false}],"reason":null},"gpu_memory":{"status":"missing","as_of":null,"bytes":null,"source":null,"reason":"unverified units"},"memory_diagnostics":[]});
    assert!(check.is_valid(&fresh));
    for (pointer, bad) in [
        ("/owners/0/reclaimable_physical/bytes", json!(-1)),
        ("/owners/0/reclaimable_physical/unknown_count", json!(-1)),
        ("/owners/0/agent/idle_seconds", json!(-1)),
        ("/owners/0/agent/last_activity_at", json!("not-a-date")),
        ("/owners/0/origin/pane_title", json!(123)),
        ("/owners/0/origin/window_name", json!(false)),
        ("/host/kernel_zones/top/0/bytes", json!(-1)),
        ("/host/kernel_zones/top/0/collectible", json!("unknown")),
        ("/host/kernel_zones/as_of", json!("bad")),
        ("/host/gpu_memory/bytes", json!(-1)),
        ("/host/gpu_memory/status", json!("made_up")),
    ] {
        let mut broken = fresh.clone();
        *broken.pointer_mut(pointer).unwrap() = bad;
        assert!(!check.is_valid(&broken), "Schema must reject {pointer}");
    }
    let before = whoate_core::kill::MemoryMeasurement {
        measured_at: Some("2026-10-17T00:00:00Z".into()),
        host_used_bytes: Some(10),
        target_resident_bytes: None,
        unknown_count: 1,
        reason: Some("resident_missing".into()),
    };
    let mut after = before.clone();
    after.host_used_bytes = Some(20);
    let result = json!({"schema":"whoate/v1","executed":true,"results":[{"pid":123,"status":"refused","reason":"apple_platform_binary"}],"missing_targets":[],"memory":whoate_core::kill::MemoryChange::new(before,Some(after))});
    assert!(check.is_valid(&result));
    assert_eq!(result["memory"]["physical_memory_change_bytes"], -10);
    assert_eq!(
        result["memory"]["target_resident_change_bytes"],
        Value::Null
    );
    let mut invalid = result;
    invalid["results"][0]["status"] = json!("success");
    assert!(!check.is_valid(&invalid));
}
