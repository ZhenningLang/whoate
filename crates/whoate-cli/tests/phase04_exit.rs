use std::process::Command;
#[test]
fn phase04_exit_binary_distinguishes_all_five_codes() {
    let root = std::env::temp_dir().join(format!("whoate-cli-exit-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let result = std::panic::catch_unwind(|| {
        for (args, expected, home) in [
            (vec!["agent-guide"], 0, true),
            (vec!["top", "--json"], 1, false),
            (vec!["why", "0"], 2, true),
            (vec!["why", "2147483647", "--json"], 3, true),
            (vec!["kill", "1", "--yes"], 4, true),
        ] {
            let mut cmd = Command::new(env!("CARGO_BIN_EXE_whoate"));
            cmd.args(&args)
                .env("WHOATE_CACHE_DIR", root.join("cache"))
                .env("WHOATE_RULES", root.join("rules.toml"))
                .env("LC_ALL", "en_US.UTF-8");
            if home {
                cmd.env("WHOATE_HOME_OVERRIDE", &root);
            } else {
                cmd.env_remove("HOME").env_remove("WHOATE_HOME_OVERRIDE");
            }
            let output = cmd.output().unwrap();
            assert_eq!(output.status.code(), Some(expected), "{args:?}");
            if expected == 4 {
                assert!(String::from_utf8_lossy(&output.stdout).contains("refused"));
            }
        }
    });
    std::fs::remove_dir_all(root).unwrap();
    if let Err(error) = result {
        std::panic::resume_unwind(error);
    }
}
