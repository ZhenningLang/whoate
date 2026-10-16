use std::process::Command;
#[test]
fn phase04_guide_is_exact_and_has_safe_agent_contract() {
    let output = Command::new(env!("CARGO_BIN_EXE_whoate"))
        .arg("agent-guide")
        .env_remove("HOME")
        .env_remove("WHOATE_HOME_OVERRIDE")
        .output()
        .unwrap();
    assert!(output.status.success());
    let expected = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../skills/whoate/SKILL.md"
    ));
    assert!(
        !output.stdout.is_empty(),
        "guide must not be an empty successful response"
    );
    assert_eq!(output.stdout, expected.unwrap());
    assert!(output.stderr.is_empty());
}
