mod common;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
fn validator() -> jsonschema::Validator {
    let schema: Value =
        serde_json::from_slice(&fs::read(root().join("schema/whoate-v1.schema.json")).unwrap())
            .unwrap();
    jsonschema::options()
        .should_validate_formats(true)
        .build(&schema)
        .unwrap()
}

fn report_failure_paths(value: &Value) -> Vec<(String, String)> {
    let schema: Value =
        serde_json::from_slice(&fs::read(root().join("schema/whoate-v1.schema.json")).unwrap())
            .unwrap();
    let mut report = schema["$defs"]["report"].clone();
    report["$defs"] = schema["$defs"].clone();
    let validator = jsonschema::options()
        .should_validate_formats(true)
        .build(&report)
        .unwrap();
    // Never format ValidationError or instance: they can contain prompt text.
    validator
        .iter_errors(value)
        .map(|e| (e.instance_path.to_string(), e.schema_path.to_string()))
        .collect()
}

#[test]
fn phase04_schema_memory_only_input() {
    let bytes =
        if std::env::var_os("WHOATE_SCHEMA_STDIN").as_deref() == Some(std::ffi::OsStr::new("1")) {
            use std::io::Read;
            let mut bytes = Vec::new();
            std::io::stdin().read_to_end(&mut bytes).expect("stdin_io");
            bytes
        } else {
            include_bytes!("../../../tests/fixtures/attribution/sample-v1.json").to_vec()
        };
    // Never include serde errors, values, or ValidationError's Display/Debug.
    let value: Value = serde_json::from_slice(&bytes).unwrap_or_else(|_| panic!("invalid_json"));
    assert!(validator().is_valid(&value), "memory_schema_invalid");
    println!("QA7_MEMORY_SCHEMA_VALID");
}

#[test]
fn phase04_schema_kernel_pid_zero_is_valid_but_negative_pid_is_not() {
    use whoate_core::{
        model::{Owner, OwnerKind, Report},
        snapshot::Snapshot,
    };
    let mut report = Report::from_snapshot(&Snapshot::historical(
        "2026-10-14T19:40:00+08:00".into(),
        vec![],
    ));
    let mut system = Owner::new("system:0".into(), OwnerKind::System, "kernel_task".into());
    system.pids = vec![0, 1];
    report.owners.push(system);
    let mut value = serde_json::to_value(report).unwrap();
    let schema = validator();
    assert!(
        schema.is_valid(&value),
        "macOS kernel PID 0 is a report identity"
    );
    value["owners"][0]["pids"][0] = json!(-1);
    assert!(!schema.is_valid(&value), "negative PIDs remain invalid");
}
#[test]
fn phase04_schema_existing_report_and_independent_invalid_fields() {
    let schema = validator();
    let original: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/attribution/sample-v1.json"
    ))
    .unwrap();
    assert!(schema.is_valid(&original));
    for (pointer, bad) in [
        ("/schema", json!("whoate/v2")),
        ("/owners/0/cpu_pct", json!("0")),
        ("/owners/0/kind", json!("unknown")),
        ("/host/wired_bytes", json!(-1)),
        ("/swarms/0/shape", json!("cluster")),
        ("/owners/0/agent/stale", json!(null)),
    ] {
        let mut value = original.clone();
        *value.pointer_mut(pointer).unwrap() = bad;
        assert!(
            !schema.is_valid(&value),
            "negative control accepted: {pointer}"
        );
    }
    let mut missing = original.clone();
    missing["owners"][0].as_object_mut().unwrap().remove("pids");
    assert!(!schema.is_valid(&missing));
    let mut additive = original.clone();
    additive["future_field"] = json!({"value":true});
    additive["owners"][0]["cpu_pct"] = json!(250.25);
    additive["owners"][0]["future_field"] = json!(42);
    assert!(schema.is_valid(&additive));
    let empty = whoate_core::model::Report::from_snapshot(
        &whoate_core::snapshot::Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![]),
    );
    assert!(schema.is_valid(&serde_json::to_value(empty).unwrap()));
    assert!(
        schema
            .is_valid(&json!({"schema":"whoate/v1","error":{"code":3,"message":"target_missing"}}))
    );
}
#[test]
fn phase04_schema_current_binary_and_exact_captured_outputs() {
    let binary = std::env::var_os("WHOATE_CLI_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root().join("target/debug/whoate"));
    assert!(
        binary.is_file(),
        "build current CLI before cross-crate tests"
    );
    let home = common::Temp::new();
    let output = Command::new(binary)
        .args(["top", "--json", "-n", "0"])
        .env("WHOATE_HOME_OVERRIDE", &home.0)
        .env("WHOATE_CACHE_DIR", home.0.join("cache"))
        .env("WHOATE_RULES", home.0.join("rules.toml"))
        .env("LC_ALL", "en_US.UTF-8")
        .output()
        .unwrap();
    assert!(matches!(output.status.code(), Some(0 | 1)));
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    let schema = validator();
    assert!(
        schema.is_valid(&value),
        "{:?}",
        schema
            .iter_errors(&value)
            .map(|e| e.to_string())
            .collect::<Vec<_>>()
    );
    assert_eq!(value["owners"], json!([]));
    if let Some(dir) = std::env::var_os("WHOATE_CLI_CAPTURE_DIR") {
        let mut count = 0;
        for file in fs::read_dir(dir).unwrap() {
            let path = file.unwrap().path();
            if path
                .extension()
                .is_some_and(|ext| ext == "json" || ext == "jsonl")
            {
                let text = fs::read_to_string(&path).unwrap();
                for line in text.lines().filter(|s| !s.trim().is_empty()) {
                    let value: Value = serde_json::from_str(line).unwrap();
                    assert!(
                        schema.is_valid(&value),
                        "captured output failed Schema: {} paths={:?}",
                        path.file_name().unwrap().to_string_lossy(),
                        report_failure_paths(&value)
                    );
                    count += 1;
                }
            }
        }
        assert!(count > 0, "capture input cannot silently skip validation");
        println!("QA7_CAPTURED_SCHEMA_OK={count}");
    }
}
