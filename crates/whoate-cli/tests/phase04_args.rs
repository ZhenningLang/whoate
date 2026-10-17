use std::process::Command;

fn cli(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_whoate"))
        .args(args)
        .env_remove("HOME")
        .env_remove("WHOATE_HOME_OVERRIDE")
        .output()
        .unwrap()
}

#[test]
fn phase04_args_help_version_and_invalid_arguments() {
    let help = cli(&["--help"]);
    assert!(help.status.success());
    let text = String::from_utf8(help.stdout).unwrap();
    for name in [
        "top",
        "why",
        "orphans",
        "swarms",
        "pane",
        "agents",
        "kill",
        "watch",
        "agent-guide",
        "insights",
    ] {
        assert!(text.contains(name), "missing command: {name}");
    }
    let version = cli(&["--version"]);
    assert_eq!(String::from_utf8(version.stdout).unwrap(), "whoate 0.1.0\n");
    for args in [
        vec!["why"],
        vec!["why", "0"],
        vec!["why", "-1"],
        vec!["kill"],
        vec!["kill", "0", "--yes"],
        vec!["watch", "--interval", "0.5s"],
        vec!["top", "--by", "other"],
        vec!["top", "--prompts", "--redact"],
        vec!["insights"],
        vec!["unknown"],
    ] {
        assert_eq!(cli(&args).status.code(), Some(2), "{args:?}");
    }
}

#[test]
fn phase04_args_reject_flags_without_a_contract_on_that_command() {
    // Only query commands have Report JSON. Catalog/termination envelopes were
    // never approved; top-only controls must not silently become no-ops.
    for args in [
        vec!["insights", "--list", "--json"],
        vec!["agent-guide", "--fresh"],
        vec!["kill", "123", "--prompts"],
        vec!["why", "123", "--by", "mem"],
        vec!["agents", "-n", "10"],
        vec!["watch", "-n", "10"],
    ] {
        let result = cli(&args);
        assert_eq!(result.status.code(), Some(2), "{args:?}");
        assert!(
            result.stdout.is_empty(),
            "invalid invocation produced output"
        );
    }
}
