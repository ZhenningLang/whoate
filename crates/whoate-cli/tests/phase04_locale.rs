use std::{fs, process::Command};

#[test]
fn phase04_locale_kill_preview_uses_the_selected_language() {
    let root = std::env::temp_dir().join(format!("whoate-p04-kill-locale-{}", std::process::id()));
    fs::create_dir(&root).unwrap();
    let outputs: Vec<_> = ["zh_CN.UTF-8", "en_US.UTF-8"]
        .into_iter()
        .map(|locale| {
            Command::new(env!("CARGO_BIN_EXE_whoate"))
                .args(["kill", "2147483647"])
                .env("LC_ALL", locale)
                .env("WHOATE_HOME_OVERRIDE", &root)
                .env("WHOATE_CACHE_DIR", root.join("cache"))
                .env("WHOATE_RULES", root.join("rules.toml"))
                .output()
                .unwrap()
        })
        .collect();
    fs::remove_dir_all(root).unwrap();
    for (output, expected) in outputs.into_iter().zip(["仅预览", "Preview only"]) {
        assert_eq!(output.status.code(), Some(3));
        assert!(String::from_utf8(output.stdout).unwrap().contains(expected));
    }
}
