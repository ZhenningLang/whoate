use std::{collections::BTreeSet, path::Path, process::Command};
use whoate_core::{model::Report, recognizers, snapshot::Snapshot};

#[test]
fn phase03_contract_six_recognizers_and_old_wire_fixtures_remain_compatible() {
    let ids: Vec<_> = recognizers::builtins().iter().map(|r| r.id()).collect();
    assert_eq!(
        &ids[..6],
        vec![
            "chromium",
            "webkit-host",
            "ide",
            "docker",
            "coding-agents",
            "dev-runtimes"
        ]
    );
    assert_eq!(ids.iter().collect::<BTreeSet<_>>().len(), ids.len());
    let snapshot: Snapshot = serde_json::from_str(include_str!(
        "../../../tests/fixtures/capture/snapshot-20261014-1940.json"
    ))
    .unwrap();
    assert_eq!(snapshot.procs.len(), 834);
    let report: Report = serde_json::from_str(include_str!(
        "../../../tests/fixtures/attribution/sample-v1.json"
    ))
    .unwrap();
    assert_eq!(report.schema, "whoate/v1");
    let value = serde_json::to_value(&report).unwrap();
    let roundtrip: Report = serde_json::from_value(value).unwrap();
    assert_eq!(roundtrip, report);
}

#[test]
fn phase03_contract_fixed_phase_ref_preserves_frozen_dependencies_collectors_and_fixtures() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    // Never use base..HEAD here: integrated HEAD may contain later phases.
    let base = "7f63627a2eb2c10e2f6870a74b183b626e183d0b";
    let tip = "8eef7e06ce3c0d0a4161d6b84b2755009084c512";
    let frozen = [
        "Cargo.toml",
        "Cargo.lock",
        "crates/whoate-core/Cargo.toml",
        "crates/whoate-cli/Cargo.toml",
        "crates/whoate-ffi/Cargo.toml",
        "crates/whoate-core/src/lib.rs",
        "crates/whoate-core/src/snapshot.rs",
        "crates/whoate-core/src/sample",
        "crates/whoate-core/src/attribute/mod.rs",
        "crates/whoate-core/src/attribute/acceptance.rs",
        "crates/whoate-core/src/swarm.rs",
        "crates/whoate-core/src/kill.rs",
        "tests/fixtures/capture",
        "tests/fixtures/attribution",
    ];
    let committed = Command::new("git")
        .args(["diff", "--exit-code", base, tip, "--"])
        .args(frozen)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        committed.status.success(),
        "frozen phase diff: {}",
        String::from_utf8_lossy(&committed.stdout)
    );
    let unstaged = Command::new("git")
        .args(["diff", "--exit-code", "--"])
        // Historical P3 collector freeze is checked above at its completion
        // ref. Current dependencies/fixtures stay frozen; P7 checks every
        // other untouched file against its own exact allowed surface.
        .args(frozen.into_iter().filter(|file| {
            file.ends_with("Cargo.toml")
                || *file == "Cargo.lock"
                || file.ends_with("src/lib.rs")
                || file.starts_with("tests/fixtures/")
        }))
        .current_dir(root)
        .output()
        .unwrap();
    assert!(
        unstaged.status.success(),
        "frozen working diff: {}",
        String::from_utf8_lossy(&unstaged.stdout)
    );
}
