//! Phase 10 round 7, R19 (owner 2026-10-25): a spread process group says how much
//! memory and CPU each owner's members use (additive, optional `owners[].mem_bytes`,
//! `owners[].cpu_pct`). Its level and text (verdict/) are unchanged.
use serde_json::{Value, json};
use std::{fs, path::PathBuf};
use whoate_core::{
    model::{Level, Owner, OwnerKind, Shape},
    snapshot::{Proc, Snapshot},
    swarm::detect,
    verdict::{Language, rules},
};

/// Twelve WebKit content processes: Mail 4, WPS 3, Raycast 2, and three launchd-adopted ones.
fn world() -> (Snapshot, Vec<Owner>) {
    let shares = [
        ("app:com.apple.mail", 4),
        ("app:com.kingsoft.wpsoffice", 3),
        ("app:com.raycast.macos", 2),
        ("service:7101", 1),
        ("service:7102", 1),
        ("service:7103", 1),
    ];
    let (mut procs, mut owners, mut pid) = (vec![], vec![], 7000);
    for (id, count) in shares {
        let mut owner = Owner::new(id.into(), OwnerKind::App, id.into());
        for _ in 0..count {
            let mut p = Proc::identity(pid, 1, 501, Some(1), "com.apple.WebKit".into());
            // The unreadable one: Raycast's second process has neither memory nor CPU.
            let readable = !(id == "app:com.raycast.macos" && owner.pids.len() == 1);
            p.footprint_bytes = readable.then_some(100 << 20);
            p.cpu_pct_sampled = readable.then_some(0.5);
            owner.pids.push(pid);
            procs.push(p);
            pid += 1;
        }
        owners.push(owner);
    }
    (
        Snapshot::historical("2026-10-25T20:30:00+08:00".into(), procs),
        owners,
    )
}

#[test]
fn qa60_spread_group_counts_memory_and_cpu_per_owner() {
    let (snap, owners) = world();
    let groups = detect(&snap, &owners);
    assert_eq!(groups.len(), 1);
    let g = &groups[0];
    assert_eq!((g.shape, g.count), (Shape::Spread, 12));
    let mail = g
        .owners
        .iter()
        .find(|o| o.id == "app:com.apple.mail")
        .unwrap();
    assert_eq!(
        (mail.count, mail.mem_bytes, mail.cpu_pct),
        (4, Some(400 << 20), Some(2.0))
    );
    let raycast = g
        .owners
        .iter()
        .find(|o| o.id == "app:com.raycast.macos")
        .unwrap();
    assert_eq!(
        (raycast.count, raycast.mem_bytes),
        (2, Some(100 << 20)),
        "the readable member only"
    );
    // The per-owner sums never exceed the group's own.
    assert_eq!(
        g.owners.iter().filter_map(|o| o.mem_bytes).sum::<u64>(),
        g.mem_bytes
    );
    // Level and text are the core's as before (frozen): the GUI decides how to show it.
    let v = rules::swarm(g, Language::Zh);
    assert_eq!((v.rule.as_str(), v.level), ("swarm-spread", Level::Watch));
}

#[test]
fn qa60_new_fields_are_optional_and_valid() {
    let (snap, owners) = world();
    let mut group = serde_json::to_value(&detect(&snap, &owners)[0]).unwrap();
    group["verdict"] = json!({"level": "watch", "rule": "swarm-spread", "text": "t"});
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let schema: Value =
        serde_json::from_slice(&fs::read(root.join("schema/whoate-v1.schema.json")).unwrap())
            .unwrap();
    let mut def = schema["$defs"]["swarm"].clone();
    def["$defs"] = schema["$defs"].clone();
    let validator = jsonschema::options().build(&def).unwrap();
    assert!(
        validator.is_valid(&group),
        "{:?}",
        validator
            .iter_errors(&group)
            .map(|e| e.instance_path.to_string())
            .collect::<Vec<_>>()
    );
    // Absent is valid (older reports), and a member without readable processes leaves them out.
    let mut old = group.clone();
    for member in old["owners"].as_array_mut().unwrap() {
        member
            .as_object_mut()
            .unwrap()
            .retain(|k, _| k == "id" || k == "count");
    }
    assert!(validator.is_valid(&old));
    let mut wrong = group.clone();
    wrong["owners"][0]["mem_bytes"] = json!(-1);
    assert!(!validator.is_valid(&wrong), "mem_bytes is a uint");
    let decoded: whoate_core::model::Swarm = serde_json::from_value(old).unwrap();
    assert!(
        decoded
            .owners
            .iter()
            .all(|o| o.mem_bytes.is_none() && o.cpu_pct.is_none())
    );
}
