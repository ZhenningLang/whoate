//! Phase 10 round 8, R21 (owner 2026-10-25): the host carries the swap files' current
//! total (optional, additive) so swap can be shown as used / total.
use whoate_core::{sample::libproc, snapshot::Host};

#[test]
fn qa63_live_host_reads_the_swap_total() {
    let host = libproc::host().expect("host sample");
    let total = host.swap_total_bytes.expect("vm.swapusage xsu_total");
    assert!(
        total >= host.swap_used_bytes,
        "{total} < {}",
        host.swap_used_bytes
    );
}

#[test]
fn qa63_swap_total_is_optional_on_the_wire() {
    let json = r#"{"mem_total_bytes":1,"mem_used_bytes":1,"compressed_bytes":0,"wired_bytes":0,"swap_used_bytes":0,"pressure":"normal"}"#;
    let mut host: Host = serde_json::from_str(json).expect("an older host still decodes");
    assert_eq!(host.swap_total_bytes, None);
    assert!(
        !serde_json::to_string(&host)
            .unwrap()
            .contains("swap_total_bytes")
    );
    host.swap_total_bytes = Some(10 << 30);
    assert!(
        serde_json::to_string(&host)
            .unwrap()
            .contains(r#""swap_total_bytes":10737418240"#)
    );
}
