use std::process::Command;
#[test]
fn phase04_pipeline_fatal_json_is_explicit_and_safe() {
    let output = Command::new(env!("CARGO_BIN_EXE_whoate"))
        .args(["top", "--json"])
        .env_remove("HOME")
        .env_remove("WHOATE_HOME_OVERRIDE")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["schema"], "whoate/v1");
    assert_eq!(value["error"]["code"], 1);
    assert!(String::from_utf8_lossy(&output.stderr).contains("config"));
}
