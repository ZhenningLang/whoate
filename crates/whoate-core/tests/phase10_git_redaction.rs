//! Phase 10 round 7, R18 (owner 2026-10-25): copied argv keeps git's command
//! name and plain-number switch values; everything private stays redacted.
use whoate_core::{
    sample::fixture,
    snapshot::{Proc, Snapshot},
};

fn redacted(argv: &[&str]) -> Vec<String> {
    let mut proc = Proc::identity(7, 1, 501, Some(1), "git".into());
    proc.argv = argv.iter().map(|a| a.to_string()).collect();
    let output = fixture::redact(Snapshot::historical(
        "2026-10-25T20:30:00+08:00".into(),
        vec![proc],
    ));
    fixture::validate_safe(&output).expect("the redactor's own output passes the privacy check");
    output.procs[0].argv.clone()
}

#[test]
fn qa58_fsmonitor_daemon_reads_as_itself() {
    let argv = redacted(&[
        "/opt/homebrew/opt/git/libexec/git-core/git",
        "fsmonitor--daemon",
        "run",
        "--detach",
        "--ipc-threads=8",
    ]);
    assert_eq!(
        argv[1..],
        ["fsmonitor--daemon", "run", "--detach", "--ipc-threads=8"]
    );
    assert_eq!(
        redacted(&["git", "fsmonitor--daemon", "stop"]),
        ["git", "fsmonitor--daemon", "stop"]
    );
    // Only right after fsmonitor--daemon, and only those four words.
    assert_eq!(
        redacted(&["git", "fsmonitor--daemon", "private"]),
        ["git", "fsmonitor--daemon", "<redacted>"]
    );
    assert_eq!(
        redacted(&["git", "status", "run"]),
        ["git", "status", "<redacted>"]
    );
}

#[test]
fn qa58_private_values_stay_redacted() {
    assert_eq!(
        redacted(&["git", "commit", "-m", "secret message"]),
        ["git", "commit", "-m", "<redacted>"]
    );
    let push = redacted(&["git", "push", "origin", "feature/x"]);
    assert_eq!(push[..3], ["git", "push", "<redacted>"], "{push:?}");
    assert!(
        !push[3].contains("feature"),
        "a branch with a slash is mapped like a path: {push:?}"
    );
    assert_eq!(
        redacted(&["git", "checkout", "my-branch"]),
        ["git", "checkout", "<redacted>"]
    );
    // Not a git command (an alias, a typo): private.
    assert_eq!(redacted(&["git", "my-alias"]), ["git", "<redacted>"]);
    // Global options with a value: the value is no command; the command after them is.
    let c = redacted(&["git", "-C", "/Users/someone/repo", "status"]);
    assert_eq!(c[3], "status", "{c:?}");
    assert!(
        !c.join(" ").contains("someone") && !c.join(" ").contains("repo"),
        "{c:?}"
    );
    assert_eq!(
        redacted(&["git", "-c", "user.name=Someone", "log"]),
        ["git", "-c", "<redacted>", "log"]
    );
    // Numbers only: a word or a path as a switch value stays private.
    assert_eq!(
        redacted(&["git", "log", "--author=someone"]),
        ["git", "log", "--author=<redacted>"]
    );
    assert_eq!(
        redacted(&["git", "log", "--max-count", "20"]),
        ["git", "log", "--max-count", "20"]
    );
}

#[test]
fn qa58_other_programs_are_unchanged() {
    assert_eq!(
        redacted(&["node", "status", "--ipc-threads=8", "--port", "3000"]),
        [
            "node",
            "<redacted>",
            "--ipc-threads=<redacted>",
            "--port",
            "<redacted>"
        ]
    );
    // A program that only ends in "git" is not git.
    assert_eq!(
        redacted(&["/usr/local/bin/legit", "status"])[1],
        "<redacted>"
    );
}

#[test]
fn qa58_privacy_check_still_rejects_raw_private_values() {
    for argv in [
        vec!["git", "push", "origin"],
        vec!["git", "commit", "-m", "secret"],
        vec!["git", "log", "--author=someone"],
        vec!["node", "--ipc-threads=8"],
    ] {
        let mut proc = Proc::identity(7, 1, 501, Some(1), "git".into());
        proc.argv = argv.iter().map(|a| a.to_string()).collect();
        let raw = Snapshot::historical("2026-10-25T20:30:00+08:00".into(), vec![proc]);
        assert!(
            fixture::validate_safe(&raw).is_err(),
            "{argv:?} must not pass unredacted"
        );
    }
}

/// Review round 7 (A:B1, CONFORM:B1): the git dir a fsmonitor daemon serves (R17) is
/// a private path in a redacted snapshot, and the privacy check reads it.
#[test]
fn qa58_git_dir_is_redacted_and_checked() {
    let repo = "/Users/someone/Projects/SecretRepo";
    let procs: Vec<Proc> = [
        (7101, format!("{repo}/.git")),
        (7102, format!("{repo}/.git/worktrees/secret-task")),
    ]
    .into_iter()
    .map(|(pid, dir)| {
        let mut p = Proc::identity(pid, 1, 501, Some(1), "git".into());
        p.argv = ["git", "fsmonitor--daemon", "run", "--detach"]
            .map(String::from)
            .to_vec();
        p.git_dir = Some(dir);
        p
    })
    .collect();
    let raw = Snapshot::historical("2026-10-25T20:30:00+08:00".into(), procs);
    assert!(
        fixture::validate_safe(&raw).is_err(),
        "a raw git dir must not pass"
    );
    let output = fixture::redact(raw);
    fixture::validate_safe(&output).expect("the redacted git dir passes");
    let json = serde_json::to_string(&output).unwrap();
    for leak in ["/Users/", "someone", "SecretRepo", "secret-task"] {
        assert!(!json.contains(leak), "{leak} survived redaction: {json}");
    }
    // The repository relation survives: both map under one redacted repository.
    let dirs: Vec<_> = output
        .procs
        .iter()
        .map(|p| p.git_dir.clone().unwrap())
        .collect();
    let repo_of = |d: &str| whoate_core::attribute::git_repository(d).map(str::to_owned);
    assert!(
        repo_of(&dirs[0]).is_some() && repo_of(&dirs[0]) == repo_of(&dirs[1]),
        "{dirs:?}"
    );
    assert!(dirs[1].contains("/.git/worktrees/path-"), "{dirs:?}");
}
