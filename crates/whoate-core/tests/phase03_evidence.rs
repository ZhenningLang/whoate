mod common;
use common::Temp;
use serde_json::{Value, json};
use std::{
    fs,
    io::{BufRead, BufReader},
    os::unix::fs::symlink,
    path::Path,
    process::{Command, Stdio},
};
use whoate_core::sample::{MacSampler, Sampler};

fn validate_venv_evidence(evidence: &Value) -> Result<(), &'static str> {
    if evidence["kind"] != "native_venv_spike"
        || evidence["production_sampler"] != "MacSampler"
        || evidence["new_environment_fields"] != false
    {
        return Err("invalid_venv_producer");
    }
    time::OffsetDateTime::parse(
        evidence["recorded_at"]
            .as_str()
            .ok_or("invalid_venv_timestamp")?,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| "invalid_venv_timestamp")?;
    let rows = evidence["rows"]
        .as_array()
        .filter(|rows| rows.len() == 5)
        .ok_or("invalid_venv_modes")?;
    for (mode, is_venv, argv0, executable, prefix) in [
        (
            "absolute",
            true,
            "$FIXTURE/project with spaces/.venv/bin/python3",
            "$FIXTURE/project with spaces/.venv/bin/python3",
            "$FIXTURE/project with spaces/.venv",
        ),
        (
            "relative",
            true,
            ".venv/bin/python3",
            "$FIXTURE/project with spaces/.venv/bin/python3",
            "$FIXTURE/project with spaces/.venv",
        ),
        (
            "activated",
            true,
            "python",
            "$FIXTURE/project with spaces/.venv/bin/python",
            "$FIXTURE/project with spaces/.venv",
        ),
        (
            "base",
            false,
            "python",
            "~/.local/share/uv/python/cpython-3.13-macos-aarch64-none/bin/python3.13",
            "~/.local/share/uv/python/cpython-3.13.12-macos-aarch64-none",
        ),
        (
            "symlink",
            true,
            "$FIXTURE/project with spaces/venv-link/bin/python3",
            "$FIXTURE/project with spaces/venv-link/bin/python3",
            "$FIXTURE/project with spaces/venv-link",
        ),
    ] {
        let matching: Vec<_> = rows.iter().filter(|row| row["mode"] == mode).collect();
        if matching.len() != 1 {
            return Err("invalid_venv_modes");
        }
        let row = matching[0];
        if row["truth"]["is_venv"] != is_venv {
            return Err("invalid_venv_truth");
        }
        if row["sample"]["argv0"] != argv0 || row["truth"]["executable"] != executable {
            return Err("invalid_venv_entry");
        }
        if row["truth"]["prefix"] != prefix {
            return Err("invalid_venv_prefix");
        }
        if row["sample"]["comm"] != "python3.13"
            || row["sample"]["cwd"] != "$FIXTURE/project with spaces"
            || row["sample"]["exe_path"]
                != "~/.local/share/uv/python/cpython-3.13.12-macos-aarch64-none/bin/python3.13"
        {
            return Err("invalid_venv_sample");
        }
    }
    Ok(())
}

#[test]
fn phase03_evidence_record_or_validate_venv_native_signals() {
    if let Ok(out) = std::env::var("WHOATE_P03_RECORD_VENV") {
        let home = Temp::new();
        let root = home.0.join("project with spaces");
        fs::create_dir(&root).unwrap();
        let venv = root.join(".venv");
        let base = Command::new("python3")
            .args(["-c", "import sys; print(sys._base_executable)"])
            .output()
            .unwrap();
        assert!(base.status.success());
        let base = String::from_utf8(base.stdout).unwrap().trim().to_owned();
        assert!(
            Command::new("python3")
                .args(["-m", "venv", "--without-pip"])
                .arg(&venv)
                .status()
                .unwrap()
                .success()
        );
        symlink(&venv, root.join("venv-link")).unwrap();
        let script = root.join("truth.py");
        fs::write(&script, "import sys,json,os\nprint(json.dumps(dict(prefix=sys.prefix,base_prefix=sys.base_prefix,executable=sys.executable,cwd=os.getcwd())),flush=True)\nsys.stdin.readline()\n").unwrap();
        let mut sampler = MacSampler::with_cache_path(home.0.join("cache/system.json")).unwrap();
        let mut rows = vec![];
        for mode in ["absolute", "relative", "activated", "base", "symlink"] {
            let mut command = if mode == "base" {
                // Match the activated argv0, without importing PATH or VIRTUAL_ENV.
                let mut c = Command::new("/bin/zsh");
                c.args(["-f", "-c", "exec -a python \"$1\" \"$2\"", "whoate-spike"])
                    .arg(&base)
                    .arg(&script);
                c
            } else if mode == "activated" {
                let mut c = Command::new("/bin/zsh");
                c.args([
                    "-f",
                    "-c",
                    "source \"$1/bin/activate\"; exec python \"$2\"",
                    "whoate-spike",
                ])
                .arg(&venv)
                .arg(&script);
                c
            } else {
                let program = match mode {
                    "absolute" => venv.join("bin/python3"),
                    "relative" => Path::new(".venv/bin/python3").to_owned(),
                    "symlink" => root.join("venv-link/bin/python3"),
                    _ => Path::new("python3").to_owned(),
                };
                let mut c = Command::new(program);
                c.arg(&script);
                c
            };
            let mut child = command
                .current_dir(&root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let mut truth = String::new();
            BufReader::new(child.stdout.take().unwrap())
                .read_line(&mut truth)
                .unwrap();
            let truth: Value = serde_json::from_str(&truth).unwrap();
            let snapshot = sampler.sample().unwrap();
            let p = snapshot
                .procs
                .iter()
                .find(|p| p.pid == child.id() as i32)
                .unwrap();
            assert_eq!(truth["prefix"] != truth["base_prefix"], mode != "base");
            let clean = |v: &str| {
                v.replace(
                    &home.0.canonicalize().unwrap().to_string_lossy().to_string(),
                    "$FIXTURE",
                )
                .replace(&home.0.to_string_lossy().to_string(), "$FIXTURE")
                .replace(&std::env::var("HOME").unwrap(), "~")
            };
            rows.push(json!({"mode":mode,"truth":{"is_venv":truth["prefix"] != truth["base_prefix"],"prefix":clean(truth["prefix"].as_str().unwrap()),"executable":clean(truth["executable"].as_str().unwrap())},"sample":{"comm":p.comm,"exe_path":p.exe_path.as_deref().map(clean),"argv0":p.argv.first().map(|s| clean(s)),"cwd":p.cwd.as_deref().map(clean)}}));
            // Only close the child's own pipe; it exits normally after readline.
            drop(child.stdin.take());
            assert!(child.wait().unwrap().success());
        }
        let evidence = json!({"kind":"native_venv_spike","recorded_at":time::OffsetDateTime::now_utc().format(&time::format_description::well_known::Rfc3339).unwrap(),"production_sampler":"MacSampler","new_environment_fields":false,"rows":rows});
        fs::write(out, serde_json::to_string_pretty(&evidence).unwrap() + "\n").unwrap();
        println!("VENV_NATIVE_EVIDENCE: {}", evidence);
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/recognizers");
    let evidence: Value =
        serde_json::from_slice(&fs::read(root.join("venv-spike.json")).unwrap()).unwrap();
    assert_eq!(evidence["production_sampler"], "MacSampler");
    assert_eq!(evidence["new_environment_fields"], false);
    let rows = evidence["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows.iter().find(|r| r["mode"] == "base").unwrap()["truth"]["is_venv"],
        false
    );
    assert_eq!(
        rows.iter().find(|r| r["mode"] == "activated").unwrap()["truth"]["is_venv"],
        true
    );
    assert_eq!(
        rows.iter().find(|r| r["mode"] == "activated").unwrap()["sample"],
        rows.iter().find(|r| r["mode"] == "base").unwrap()["sample"]
    );
    assert_eq!(validate_venv_evidence(&evidence), Ok(()));
}

#[test]
fn phase03_evidence_bad_mode_truth_entry_and_timestamp_are_rejected() {
    let original: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/recognizers/venv-spike.json"
    ))
    .unwrap();
    for mode in ["absolute", "relative", "activated", "base", "symlink"] {
        let mut bad = original.clone();
        let row = bad["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["mode"] == mode)
            .unwrap();
        row["truth"]["is_venv"] = json!(mode == "base");
        assert_eq!(validate_venv_evidence(&bad), Err("invalid_venv_truth"));
        let mut bad = original.clone();
        bad["rows"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|row| row["mode"] == mode)
            .unwrap()["sample"]["argv0"] = json!("not-a-python-entry");
        assert_eq!(validate_venv_evidence(&bad), Err("invalid_venv_entry"));
    }
    let mut bad = original.clone();
    bad["recorded_at"] = json!("invalid-time");
    assert_eq!(validate_venv_evidence(&bad), Err("invalid_venv_timestamp"));
    let mut bad = original;
    bad["rows"][1]["mode"] = json!("absolute");
    assert_eq!(validate_venv_evidence(&bad), Err("invalid_venv_modes"));
}

#[test]
fn phase03_evidence_docker_real_attempts_stats_footprint_and_cleanup_are_recorded() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/recognizers/docker-spike.json");
    let e: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(e["branch"], "container_stats");
    assert_eq!(e["container_count"], 1);
    assert_eq!(e["local_unix_only"], true);
    assert_eq!(e["own_container_removed"], true);
    let parse = |key: &str| {
        time::OffsetDateTime::parse(
            e[key].as_str().unwrap(),
            &time::format_description::well_known::Rfc3339,
        )
        .unwrap()
    };
    let elapsed = parse("finished_at") - parse("started_at");
    assert!(elapsed.whole_milliseconds() > 0 && elapsed.whole_seconds() < 1800);
    let attempts = e["attempts"].as_array().unwrap();
    for operation in ["version", "image", "run", "stats", "rm"] {
        assert!(
            attempts
                .iter()
                .any(|a| a["operation"] == operation && a["exit"] == 0)
        );
    }
    assert_eq!(e["memory_usage"][0], "3.859MiB / 7.748GiB");
    assert_eq!(e["vm_measurements"][0]["footprint_bytes"], 38486904_u64);
    assert_eq!(e["vm_measurements"][1]["footprint_bytes"], 1108232616_u64);
}
