use std::{sync::atomic::AtomicBool, time::Duration};
use whoate_core::sample::{
    command::read_command,
    top::{self, memory_bytes, parse_top},
};

#[test]
fn macos_top_percent_cpu_header_yields_complete_numeric_sample() {
    // The live -stats pid,mem,cpu probe emits %CPU, not CPU (877/877 rows).
    // Keep only public headers and synthetic numeric values in this regression.
    let sample = parse_top(
        "Processes: 2 total, 1 running, 1 sleeping, 4 threads\n\
         2026/10/15 01:36:00\n\
         CPU usage: 2.0% user, 1.0% sys, 97.0% idle\n\n\
         PID    MEM    %CPU\n\
         42     128M   0.0\n\
         1      1.5M   0.0\n",
    )
    .unwrap();
    assert_eq!(sample.declared_count, 2);
    assert_eq!(sample.rows.len(), 2);
    assert_eq!(sample.rows[&42].footprint_bytes, 134_217_728);
    assert_eq!(sample.rows[&1].footprint_bytes, 1_572_864);
    assert_eq!(sample.rows[&42].cpu_pct_sampled, None);
}

#[test]
fn second_top_frame_supplies_interval_cpu_without_reusing_first_frame() {
    let sample = parse_top(
        "Processes: 1 total\nPID MEM %CPU\n42 1M 99.0\n\n\
         Processes: 1 total\nPID MEM %CPU\n42 2M 3.5\n",
    )
    .unwrap();
    assert_eq!(sample.rows[&42].footprint_bytes, 2_097_152);
    assert_eq!(sample.rows[&42].cpu_pct_sampled, Some(3.5));
}

#[test]
fn top_parser_requires_complete_unique_result_and_footprint_units() {
    let sample =
        parse_top("Processes: 3 total\nPID MEM CPU\n1 1.5M 0.0\n2 1024K+ 2.4\n3 99B- 0.0\n")
            .unwrap();
    assert_eq!(sample.rows[&1].footprint_bytes, 1_572_864);
    assert_eq!(sample.rows[&2].footprint_bytes, 1_048_576);
    assert_eq!(sample.rows[&3].footprint_bytes, 99);
    assert!(
        sample
            .rows
            .values()
            .all(|row| row.cpu_pct_sampled.is_none())
    );
    assert_eq!(memory_bytes("2G").unwrap(), 2_147_483_648);
    assert_eq!(memory_bytes("1T").unwrap(), 1_099_511_627_776);
    for text in [
        "Processes: 3 total\n",
        "Processes: 2 total\nPID MEM CPU\n1 1M 0.0\n",
        "Processes: 2 total\nPID MEM CPU\n1 1M 0.0\n1 1M 0.0\n",
        "Processes: 1 total\nPID MEM CPU\n1 junk 0.0\n",
        "Processes: 1 total\nPID MEM CPU\n1 1M NaN\n",
        "Processes: 10001 total\nPID MEM CPU\n1 1M 0.0\n",
        "Processes: 1 total\nPID MEM CPU\n1 1M 0.0 EXTRA\n",
    ] {
        assert!(parse_top(text).is_err());
    }
    for text in [
        "-1M",
        "NaN",
        "1P",
        "1.0.0M",
        "",
        "999999999999999999999999T",
    ] {
        assert!(memory_bytes(text).is_err());
    }
    assert_eq!(
        top::TOP_ARGS,
        &["-l", "2", "-s", "1", "-stats", "pid,mem,cpu", "-n", "10000"]
    );
    assert_eq!(
        top::TOP_ARGS.last().unwrap().parse::<usize>().unwrap(),
        top::MAX_PROCESSES
    );
}

#[test]
fn unavailable_second_frame_keeps_cpu_null_and_reports_safe_issue() {
    let first = "Processes: 1 total\nPID MEM %CPU\n42 1M 99.0\n";
    let sample = parse_top(first).unwrap();
    assert_eq!(sample.rows[&42].cpu_pct_sampled, None);
    assert_eq!(sample.cpu_issue.as_deref(), Some("second_frame_missing"));
    let sample = parse_top(&format!(
        "{first}Processes: 2 total\nPID MEM %CPU\n42 2M 4.0\n"
    ))
    .unwrap();
    assert_eq!(sample.rows[&42].footprint_bytes, 1_048_576);
    assert_eq!(sample.rows[&42].cpu_pct_sampled, None);
    assert_eq!(
        sample.cpu_issue.as_deref(),
        Some("second_frame_unavailable;incomplete_result")
    );
}

#[test]
fn external_tool_failure_timeout_output_budget_and_cancellation_are_errors() {
    let stop = AtomicBool::new(false);
    let output = read_command("/usr/bin/false", &[], &stop, Duration::from_secs(1), 100).unwrap();
    assert!(!output.success);
    assert_eq!(
        read_command("/bin/sleep", &["1"], &stop, Duration::from_millis(5), 100)
            .err()
            .unwrap()
            .code,
        "timeout"
    );
    assert_eq!(
        read_command(
            "/bin/echo",
            &["0123456789"],
            &stop,
            Duration::from_secs(1),
            2
        )
        .err()
        .unwrap()
        .code,
        "output_limit"
    );
    let stop = AtomicBool::new(true);
    assert_eq!(
        read_command("/bin/sleep", &["1"], &stop, Duration::from_secs(1), 100)
            .err()
            .unwrap()
            .code,
        "cancelled"
    );
}

#[test]
fn malformed_top_corpus_terminates_and_never_panics() {
    for tail in [
        "<",
        "\0",
        "1\t\t\n",
        "1 1M",
        "--- --- ---",
        "1 1M -1",
        "1 1M inf",
        "1 1M 0\n1 1M 0",
        "é 1M 0",
    ] {
        assert!(parse_top(&format!("Processes: 1 total\nPID MEM CPU\n{tail}\n")).is_err());
    }
}
