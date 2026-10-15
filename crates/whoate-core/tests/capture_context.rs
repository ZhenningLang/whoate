use whoate_core::{
    sample::{libproc::responsible_value, sysctl, tmux},
    snapshot::Pressure,
};

fn pages(
    size: u64,
    internal: u64,
    purgeable: u64,
    wired: u64,
    compressed: u64,
) -> sysctl::PageCounts {
    sysctl::PageCounts {
        page_size: size,
        internal,
        purgeable,
        wired,
        compressed,
    }
}

#[test]
fn host_uses_actual_page_size_and_anonymous_wired_compressor_not_process_sum() {
    let host = sysctl::host_from_pages(1_000_000, pages(16_384, 10, 2, 3, 4), 123, 2).unwrap();
    assert_eq!(host.mem_used_bytes, 245_760);
    assert_eq!(host.compressed_bytes, 65_536);
    assert_eq!(host.wired_bytes, 49_152);
    assert_eq!(host.swap_used_bytes, 123);
    assert_eq!(host.pressure, Pressure::Warn);
    assert_eq!(
        sysctl::host_from_pages(100, pages(4096, 0, 0, 0, 0), 0, 1)
            .unwrap()
            .pressure,
        Pressure::Normal
    );
    assert_eq!(
        sysctl::host_from_pages(100, pages(4096, 0, 0, 0, 0), 0, 4)
            .unwrap()
            .pressure,
        Pressure::Critical
    );
    assert!(sysctl::host_from_pages(100, pages(0, 0, 0, 0, 0), 0, 1).is_err());
    assert!(sysctl::host_from_pages(100, pages(4096, 1, 2, 0, 0), 0, 1).is_err());
    assert!(sysctl::host_from_pages(100, pages(4096, 0, 0, 0, 0), 0, 7).is_err());
}

#[test]
fn tmux_five_columns_keep_paths_with_spaces_and_responsible_degrades() {
    let panes =
        tmux::parse_panes("%42\tsession:1.2\twindow name\t123\t/Users/user/project with spaces\n")
            .unwrap();
    assert_eq!(panes[0].pane_pid, 123);
    assert_eq!(
        panes[0].cwd.as_deref(),
        Some("/Users/user/project with spaces")
    );
    assert_eq!(panes[0].target, "session:1.2");
    assert!(tmux::parse_panes("").unwrap().is_empty());
    for input in [
        "%1\ta\tb\t123",
        "%1\ta\tb\tbad\t/x",
        "private\ta\tb\t123\t/x",
    ] {
        assert!(tmux::parse_panes(input).is_err());
    }
    for value in [None, Some(-1), Some(0), Some(1)] {
        assert_eq!(responsible_value(value), None);
    }
    assert_eq!(responsible_value(Some(123)), Some(123));
}

#[test]
fn tmux_command_failures_distinguish_missing_no_server_and_real_errors() {
    use whoate_core::sample::{SampleError, command::Output};
    let run = |success, stdout: &str, stderr: &str| {
        tmux::capture_with(|| {
            Ok(Output {
                success,
                stdout: stdout.as_bytes().to_vec(),
                stderr: stderr.as_bytes().to_vec(),
            })
        })
    };
    assert!(
        tmux::capture_with(|| Err(SampleError::os("command_spawn", libc::ENOENT)))
            .unwrap()
            .is_empty()
    );
    assert!(
        run(false, "", "no server running on /isolated/tmux/default")
            .unwrap()
            .is_empty()
    );
    assert!(
        run(
            false,
            "",
            "error connecting to /isolated/tmux/default (No such file or directory)"
        )
        .unwrap()
        .is_empty()
    );
    assert_eq!(
        run(false, "", "permission denied PRIVATE_ENV_SENTINEL")
            .unwrap_err()
            .code,
        "nonzero_exit"
    );
    assert!(
        !format!("{:?}", run(false, "", "PRIVATE_ENV_SENTINEL").unwrap_err())
            .contains("PRIVATE_ENV_SENTINEL")
    );
    assert!(tmux::capture_with(|| Err(SampleError::os("command_spawn", libc::EACCES))).is_err());
    assert!(run(true, "malformed pane columns\n", "").is_err());
    assert_eq!(
        run(true, "%1\ts:0.0\tw\t123\t/path with spaces\n", "").unwrap()[0].pane_pid,
        123
    );
}
