//! Phase 10 feedback 1 (QA7): an owner with unreadable processes still gets
//! history over its readable part, and a change in how many are unreadable
//! never becomes a trend.
use whoate_core::{
    history::History,
    model::{Owner, OwnerKind, Report},
};

/// An app owner with four processes, `unknown` of them unreadable.
fn owner(mem: u64, unknown: usize) -> Owner {
    let mut o = Owner::new(
        "app:com.brave.Browser".into(),
        OwnerKind::App,
        "Brave".into(),
    );
    o.pids = vec![1001, 1002, 1003, 1004];
    o.mem_bytes = mem;
    o.unknown_mem_count = unknown;
    o
}

#[test]
fn partly_unreadable_owners_get_points_over_the_readable_part() {
    let mut h = History::default();
    h.record_at(&owner(700, 3), &[], 10);
    let series = h.series("app:com.brave.Browser");
    assert_eq!(series.len(), 1, "a point despite 3 unreadable processes");
    assert_eq!(series[0].mem_bytes, 700, "only the readable process");
    assert_eq!(series[0].unknown_mem_count, Some(3));
}

#[test]
fn nothing_readable_leaves_a_gap_never_a_zero() {
    let mut h = History::default();
    h.record_at(&owner(0, 4), &[], 10);
    assert!(h.series("app:com.brave.Browser").is_empty());
}

#[test]
fn a_change_in_unreadable_count_breaks_the_trend_but_a_steady_count_does_not() {
    let mut h = History::default();
    let id = "app:com.brave.Browser";
    // t 1..=4 all read; 5 three unreadable (0 → 3); 6..=8 still three; 9 all read (3 → 0).
    for t in 1..=4 {
        h.record_at(&owner(1000 + t as u64, 0), &[], t);
    }
    for t in 5..=8 {
        h.record_at(&owner(400 + t as u64, 3), &[], t);
    }
    assert_eq!(
        h.window(id, 3).map(|w| w.growth_bytes),
        Some(3),
        "3 → 3: the deltas of points 6, 7, 8 (+1 each) count"
    );
    assert_eq!(
        h.window(id, 4),
        None,
        "0 → 3 at t=5: no delta, no trend across it"
    );
    h.record_at(&owner(2000, 0), &[], 9);
    assert_eq!(h.window(id, 1), None, "3 → 0 at t=9: no delta");
    let unknown: Vec<_> = h.series(id).iter().map(|p| p.unknown_mem_count).collect();
    assert_eq!(
        unknown,
        [
            None,
            None,
            None,
            None,
            Some(3),
            Some(3),
            Some(3),
            Some(3),
            None
        ]
    );
}

#[test]
fn old_fixtures_round_trip_unchanged_and_the_field_is_optional_in_the_schema() {
    for path in [
        "../../tests/fixtures/cli/output-v1.json",
        "../../tests/fixtures/attribution/sample-v1.json",
    ] {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
        let text = std::fs::read_to_string(&path).unwrap();
        let before: serde_json::Value = serde_json::from_str(&text).unwrap();
        let report: Report = serde_json::from_str(&text).unwrap();
        assert_eq!(
            serde_json::to_value(&report).unwrap(),
            before,
            "{}",
            path.display()
        );
        assert!(!text.contains("unknown_mem_count\": null"));
    }
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../../../schema/whoate-v1.schema.json")).unwrap();
    let items = &schema["$defs"]["history"]["items"];
    assert_eq!(
        items["properties"]["unknown_mem_count"]["$ref"],
        "#/$defs/uint"
    );
    let required: Vec<_> = items["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_str())
        .collect();
    assert_eq!(required, ["timestamp_s", "mem_bytes", "cpu_pct"]);
}
