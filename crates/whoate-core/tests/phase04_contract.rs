use std::{fs, path::PathBuf, process::Command};
#[test]
fn phase04_contract_frozen_inputs_match_both_phase_baselines() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tree = Command::new("git")
        .current_dir(&root)
        .args([
            "ls-tree",
            "-r",
            "--name-only",
            "8eef7e06ce3c0d0a4161d6b84b2755009084c512",
        ])
        .output()
        .unwrap();
    assert!(tree.status.success(), "P4 baseline is required");
    let mut count = 0;
    for file in String::from_utf8(tree.stdout).unwrap().lines().filter(|p| {
        (p.starts_with("crates/whoate-core/src/")
            // Supplemental rulings 5/6 authorize indexed paths and a narrowing
            // session-read capability; all other frozen inputs stay byte-equal.
            && !matches!(*p, "crates/whoate-core/src/recognizers/registry.rs"
                | "crates/whoate-core/src/recognizers/mod.rs"
                | "crates/whoate-core/src/engine.rs"
                | "crates/whoate-core/src/config_reader.rs"
                | "crates/whoate-core/src/sources/mod.rs"
                | "crates/whoate-core/src/sources/claude_code.rs"))
            || p.starts_with("tests/fixtures/")
            || p.ends_with("Cargo.toml")
            || *p == "Cargo.lock"
            || *p == "crates/whoate-core/build.rs"
    }) {
        let baseline = if file.ends_with("Cargo.toml")
            || file == "Cargo.lock"
            || file == "crates/whoate-core/src/lib.rs"
        {
            "7f9c20146457ceec50c30a1dc7a35edb742bb85b"
        } else {
            "8eef7e06ce3c0d0a4161d6b84b2755009084c512"
        };
        let old = Command::new("git")
            .current_dir(&root)
            .args(["show", &format!("{baseline}:{file}")])
            .output()
            .unwrap();
        assert!(old.status.success(), "missing frozen history: {file}");
        let actual = if file.ends_with("Cargo.toml")
            || file == "Cargo.lock"
            || file == "crates/whoate-core/src/lib.rs"
            || file.starts_with("tests/fixtures/")
            || file == "crates/whoate-core/build.rs"
        {
            fs::read(root.join(file)).unwrap()
        } else {
            let p4 = Command::new("git")
                .current_dir(&root)
                .args([
                    "show",
                    &format!("e0d5440a39b9dc7a6932f45452a6d008fca36e17:{file}"),
                ])
                .output()
                .unwrap();
            assert!(p4.status.success(), "missing P4 completion: {file}");
            p4.stdout
        };
        assert_eq!(actual, old.stdout, "frozen input drift: {file}");
        count += 1;
    }
    assert!(count > 30);
    println!("QA2_FROZEN_INPUTS_OK={count}");
}
#[test]
fn phase04_contract_schema_covers_every_existing_wire_field() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join("schema/whoate-v1.schema.json")).unwrap())
            .unwrap();
    let report: serde_json::Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/attribution/sample-v1.json"
    ))
    .unwrap();
    for (definition, value) in [
        ("report", &report),
        ("owner", &report["owners"][0]),
        ("part", &report["owners"][0]["parts"][0]),
        ("fact", &report["owners"][0]["facts"][0]),
        ("swarm", &report["swarms"][0]),
    ] {
        let props = &schema["$defs"][definition]["properties"];
        for key in value.as_object().unwrap().keys() {
            if key != "schema" {
                assert!(props.get(key).is_some(), "undocumented {definition}.{key}");
            }
        }
    }
    for key in [
        "name_source",
        "updated_at_ms",
        "stale",
        "first_prompt",
        "first_reply",
    ] {
        assert!(
            schema["$defs"]["agent"]["anyOf"][1]["properties"]
                .get(key)
                .is_some()
        );
    }
    assert!(
        schema["$defs"]["host"]["anyOf"][1]["properties"]
            .get("wired_bytes")
            .is_some()
    );
}
