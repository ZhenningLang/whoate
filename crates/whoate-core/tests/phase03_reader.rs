mod common;
use common::Temp;
use std::{fs, os::unix::fs::symlink};
use whoate_core::recognizers::ConfigReader;

#[test]
fn phase03_reader_mtime_cache_invalidates_changes_deletion_and_bad_data() {
    let home = Temp::new();
    let path = home.0.join("settings.json");
    fs::write(&path, r#"{"enabled":true}"#).unwrap();
    let cfg = ConfigReader::isolated(home.0.clone());
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    assert_eq!(cfg.read_json(&path).unwrap()["enabled"], true);
    assert_eq!(cfg.read_json(&path).unwrap()["enabled"], true);
    assert_eq!(cfg.disk_reads(), 1);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    fs::write(&path, r#"{"enabled":false}"#).unwrap();
    assert_eq!(cfg.read_json(&path).unwrap()["enabled"], false);
    assert_eq!(cfg.disk_reads(), 2);
    fs::write(&path, "broken").unwrap();
    assert_eq!(cfg.read_json(&path), Err("invalid_json"));
    fs::remove_file(&path).unwrap();
    assert!(cfg.read_json(&path).is_err());
    fs::write(&path, r#"{"enabled":null}"#).unwrap();
    assert_eq!(
        cfg.read_json(&path).unwrap()["enabled"],
        serde_json::Value::Null
    );
}

#[test]
fn phase03_reader_jsonc_strings_comments_trailing_commas_and_hostile_eof() {
    let home = Temp::new();
    let path = home.0.join("settings.json");
    let cfg = ConfigReader::isolated(home.0.clone());
    fs::write(&path, "{ // comment\n\"text\":\"https://example.test/\\\"/*not comment*/\", /* x */\"list\":[1,2,],}").unwrap();
    let json = cfg.read_jsonc(&path).unwrap();
    assert_eq!(json["text"], "https://example.test/\"/*not comment*/");
    assert_eq!(json["list"], serde_json::json!([1, 2]));
    for bad in [
        "/*",
        "{\"x\":\"\\",
        "{\"x\":1} garbage",
        "{,}",
        "[1,,]",
        "\0",
        "{\"x\":1/*unterminated",
        "{\"x\":1,//EOF",
    ] {
        fs::write(&path, bad).unwrap();
        assert!(cfg.read_jsonc(&path).is_err(), "malformed input accepted");
    }
    for n in 0..128 {
        fs::write(&path, format!("{}{}", "[/*\"\\".repeat(n), "*/]".repeat(n))).unwrap();
        let _ = cfg.read_jsonc(&path);
    }
}

#[test]
fn phase03_reader_symlink_and_root_escape_never_read_secret() {
    let home = Temp::new();
    let outside = Temp::new();
    let private = outside.0.join("secret.json");
    fs::write(&private, "SECRET_SENTINEL").unwrap();
    let link = home.0.join("link.json");
    symlink(&private, &link).unwrap();
    let cfg = ConfigReader::isolated(home.0.clone());
    assert_eq!(cfg.read_jsonc(&link), Err("outside_fixture_root"));
    assert_eq!(cfg.disk_reads(), 0);
    assert_eq!(fs::read_to_string(private).unwrap(), "SECRET_SENTINEL");
}

#[test]
fn phase03_reader_debug_redacts_cached_raw_config_and_plist_reads_use_mtime_cache() {
    let home = Temp::new();
    let cfg = ConfigReader::isolated(home.0.clone());
    let json = home.0.join("settings.json");
    fs::write(&json, r#"{"secret":"RAW_SECRET_SENTINEL"}"#).unwrap();
    assert!(cfg.read_json(&json).is_ok());
    assert!(!format!("{cfg:?}").contains("RAW_SECRET_SENTINEL"));
    let path = home.0.join("Info.plist");
    let value = plist::Value::String("public-app-name".into());
    value.to_file_binary(&path).unwrap();
    assert_eq!(cfg.read_plist(&path), Ok(value.clone()));
    assert_eq!(cfg.read_plist(&path), Ok(value));
    assert_eq!(cfg.disk_reads(), 2);
    let next = plist::Value::String("changed-app".into());
    next.to_file_xml(&path).unwrap();
    assert_eq!(cfg.read_plist(&path), Ok(next));
    assert_eq!(cfg.disk_reads(), 3);
    fs::write(
        &path,
        "<?xml version=\"1.0\"?><plist><dict><key>RAW_SECRET_SENTINEL</key>",
    )
    .unwrap();
    assert_eq!(cfg.read_plist(&path), Err("app_metadata_unavailable"));
    fs::remove_file(&path).unwrap();
    assert!(cfg.read_plist(&path).is_err());
    assert!(!format!("{cfg:?}").contains("RAW_SECRET_SENTINEL"));
}
