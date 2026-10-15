mod common;
use common::Temp;
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
};
use whoate_core::{sample::fixture, snapshot::Snapshot};

fn stats(snap: &Snapshot) -> (usize, u64, [usize; 4]) {
    let members: Vec<_> = snap
        .procs
        .iter()
        .filter(|proc| {
            proc.argv
                .iter()
                .any(|arg| arg.contains("chrome-reading-list-mcp"))
        })
        .collect();
    let mut markers = [0; 4];
    for proc in &snap.procs {
        for arg in &proc.argv {
            for (i, marker) in [
                "--type=renderer",
                "--type=gpu-process",
                "--utility-sub-type=",
                "--extension-process",
            ]
            .iter()
            .enumerate()
            {
                if arg.contains(marker) {
                    markers[i] += 1;
                }
            }
        }
    }
    (
        members.len(),
        members.iter().filter_map(|proc| proc.footprint_bytes).sum(),
        markers,
    )
}

fn digest(path: &Path) -> String {
    let output = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .arg(path)
        .output()
        .unwrap();
    assert!(output.status.success());
    let digest = String::from_utf8(output.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(digest.len(), 64);
    assert!(digest.bytes().all(|c| c.is_ascii_hexdigit()));
    digest
}

#[test]
fn archive_conversion_and_live_recorder_preserve_source_and_never_copy_raw() {
    let raw = PathBuf::from(
        std::env::var("WHOATE_CAPTURE_RAW")
            .expect("QA11 requires the approved read-only raw source"),
    );
    let before = std::fs::read(&raw).unwrap();
    let before_digest = digest(&raw);
    let permissions = std::fs::metadata(&raw).unwrap().permissions().mode();
    assert_eq!(permissions & 0o777, 0o600);
    let imported = fixture::import_archive(&before).unwrap();
    assert_eq!(imported.procs.len(), 834);
    let source: serde_json::Value = serde_json::from_slice(&before).unwrap();
    for (source, proc) in source["procs"]
        .as_array()
        .unwrap()
        .iter()
        .zip(&imported.procs)
    {
        let recorded = serde_json::to_value(proc).unwrap();
        for field in [
            "pid",
            "ppid",
            "uid",
            "footprint_bytes",
            "cpu_time_ns",
            "tmux_pane",
        ] {
            assert_eq!(
                recorded[field], source[field],
                "source field {field} changed for pid={}",
                proc.pid
            );
        }
    }
    // Independent Python datetime conversion of raw first start + recorded +08:00.
    assert_eq!(imported.procs[0].pid, 1);
    assert_eq!(imported.procs[0].start_time, Some(1_788_308_925_000_000));
    assert_eq!(stats(&imported), (69, 2_901_288_016, [56, 9, 25, 12]));
    let output = fixture::redact(imported.clone());
    assert_eq!(stats(&output), stats(&imported));
    for (a, b) in imported.procs.iter().zip(&output.procs) {
        if let Some(path) = &a.exe_path {
            if [
                "/usr/",
                "/System/",
                "/Applications/",
                "/Library/",
                "/bin/",
                "/sbin/",
                "/opt/homebrew/",
            ]
            .iter()
            .any(|root| path.starts_with(root))
            {
                assert!(
                    b.exe_path.as_ref() == Some(path),
                    "public executable path changed for pid={}",
                    a.pid
                );
            }
            if path.contains(".app/Contents/") {
                assert!(
                    b.exe_path
                        .as_deref()
                        .is_some_and(|path| path.contains(".app/Contents/")),
                    "application marker lost for pid={}",
                    a.pid
                );
            }
        }
        assert_eq!(
            (
                a.pid,
                a.ppid,
                a.uid,
                a.start_time,
                a.footprint_bytes,
                a.cpu_time_ns,
                &a.tmux_pane
            ),
            (
                b.pid,
                b.ppid,
                b.uid,
                b.start_time,
                b.footprint_bytes,
                b.cpu_time_ns,
                &b.tmux_pane
            )
        );
    }
    for signal in ["com.docker", "vite"] {
        // A private project name containing "vite" is not a public script name.
        // Keep the independent source oracle at exact script basenames instead
        // of requiring a private directory substring to survive redaction.
        let matches = |text: &str| {
            if signal == "vite" {
                text.split_whitespace()
                    .any(|token| matches!(token.rsplit('/').next(), Some("vite" | "vite.js")))
            } else {
                text.contains(signal)
            }
        };
        let before = source["procs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|proc| matches(proc["command"].as_str().unwrap()))
            .count();
        let after = output
            .procs
            .iter()
            .filter(|proc| matches(&proc.comm) || proc.argv.iter().any(|arg| matches(arg)))
            .count();
        assert_eq!(after, before, "recognizer signal {signal} changed");
        assert_eq!(before, if signal == "vite" { 3 } else { 2 });
    }
    let dir = Temp::new();
    let archive_dest = dir.0.join("archive.json");
    let live_dest = dir.0.join("live.json");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let committed: Snapshot = serde_json::from_slice(
        &std::fs::read(root.join("tests/fixtures/capture/snapshot-20261014-1940.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(
        committed, output,
        "committed fixture must match current safe conversion"
    );
    let record = |source: Option<&Path>, destination: &Path| {
        let mut command = Command::new("bash");
        command
            .arg(root.join("scripts/record-fixture.sh"))
            .env("WHOATE_CACHE_DIR", dir.0.join("cache"))
            .current_dir(&root);
        if let Some(source) = source {
            command.arg("--source").arg(source);
        }
        command.arg("--output").arg(destination);
        let result = command.output().unwrap();
        assert!(
            result.status.success(),
            "safe recorder status: {}",
            String::from_utf8_lossy(&result.stderr)
        );
        let bytes = std::fs::read(destination).unwrap();
        let snap: Snapshot = serde_json::from_slice(&bytes).unwrap();
        fixture::validate_safe(&snap).unwrap();
        assert!(!String::from_utf8_lossy(&result.stderr).contains("/Users/"));
        snap
    };
    let archive = record(Some(&raw), &archive_dest);
    assert_eq!(stats(&archive), (69, 2_901_288_016, [56, 9, 25, 12]));
    let live = record(None, &live_dest);
    assert!(!live.procs.is_empty());
    assert!(live.host.is_some());
    let taken = time::OffsetDateTime::parse(
        &live.taken_at,
        &time::format_description::well_known::Rfc3339,
    )
    .unwrap();
    assert!(
        (time::OffsetDateTime::now_utc() - taken)
            .whole_seconds()
            .abs()
            < 30
    );
    assert_eq!(std::fs::read(&raw).unwrap(), before);
    assert_eq!(digest(&raw), before_digest);
    assert_eq!(
        std::fs::metadata(&raw).unwrap().permissions().mode(),
        permissions
    );
    let tracked = Command::new("git")
        .args(["ls-files"])
        .current_dir(&root)
        .output()
        .unwrap();
    assert!(tracked.status.success());
    assert!(!String::from_utf8_lossy(&tracked.stdout).contains("raw-snapshot"));
    println!(
        "QA11: source unchanged; rows=834 reading-list=69 bytes=2901288016 markers=56/9/25/12; live redaction checked"
    );
}
