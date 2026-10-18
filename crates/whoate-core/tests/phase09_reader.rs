mod common;
use common::Temp;
use std::{fs, os::unix::fs::PermissionsExt};
use whoate_core::recognizers::ConfigReader;

#[test]
fn phase09_plist_clone_reuses_parsed_xml_and_binary() {
    let t = Temp::new();
    let cfg = ConfigReader::isolated(t.0.clone());
    let value = plist::Value::String("PRIVATE_PLIST_SENTINEL".into());
    for binary in [false, true] {
        let path = t.0.join(if binary { "binary" } else { "xml" });
        if binary {
            value.to_file_binary(&path).unwrap();
        } else {
            value.to_file_xml(&path).unwrap();
        }
        let before = cfg.plist_parses();
        assert_eq!(cfg.read_plist(&path), Ok(value.clone()));
        assert_eq!(cfg.clone().read_plist(&path), Ok(value.clone()));
        assert_eq!(cfg.plist_parses() - before, 1);
    }
    assert_eq!(cfg.disk_reads(), 2);
    assert!(!format!("{cfg:?}").contains("PRIVATE_PLIST_SENTINEL"));
}

#[test]
fn phase09_plist_changes_errors_permissions_and_root_are_not_fresh_hits() {
    let t = Temp::new();
    let cfg = ConfigReader::isolated(t.0.clone());
    let p = t.0.join("Info.plist");
    let old = plist::Value::String("AAAA".into());
    let new = plist::Value::String("BBBB".into());
    old.to_file_binary(&p).unwrap();
    assert_eq!(cfg.read_plist(&p), Ok(old));
    let modified = fs::metadata(&p).unwrap().modified().unwrap();
    let replacement = t.0.join("replacement");
    new.to_file_binary(&replacement).unwrap();
    fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(modified)
        .unwrap();
    fs::rename(replacement, &p).unwrap();
    assert_eq!(cfg.read_plist(&p), Ok(new.clone()));
    fs::write(&p, "bad update").unwrap();
    assert_eq!(cfg.read_plist(&p), Err("app_metadata_unavailable"));
    fs::remove_file(&p).unwrap();
    assert!(cfg.read_plist(&p).is_err());
    new.to_file_xml(&p).unwrap();
    assert_eq!(cfg.read_plist(&p), Ok(new.clone()));
    fs::set_permissions(&p, fs::Permissions::from_mode(0o0)).unwrap();
    assert!(cfg.read_plist(&p).is_err());
    fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(cfg.read_plist(&p), Ok(new));
    let mut outside = cfg.clone();
    outside.home = t.0.join("other-root");
    fs::create_dir(&outside.home).unwrap();
    assert_eq!(outside.read_plist(&p), Err("outside_fixture_root"));
}
