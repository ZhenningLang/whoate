use super::{
    SampleError, SampleResult,
    command::{Output, read_command},
};
use crate::snapshot::TmuxPane;
use std::{sync::atomic::AtomicBool, time::Duration};

pub const FORMAT: &str = "#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}\t#{window_name}\t#{pane_pid}\t#{pane_current_path}";

pub fn parse_panes(text: &str) -> SampleResult<Vec<TmuxPane>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.splitn(5, '\t').collect();
            if fields.len() != 5 || !valid_pane(fields[0]) {
                return Err(SampleError::new("tmux", "invalid_columns"));
            }
            let pid = fields[3]
                .parse::<i32>()
                .ok()
                .filter(|pid| *pid > 0)
                .ok_or_else(|| SampleError::new("tmux", "invalid_pid"))?;
            Ok(TmuxPane {
                pane_id: fields[0].into(),
                target: fields[1].into(),
                window_name: Some(fields[2].into()),
                pane_pid: pid,
                cwd: Some(fields[4].into()),
            })
        })
        .collect()
}

pub fn valid_pane(value: &str) -> bool {
    value
        .strip_prefix('%')
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
}

pub fn capture() -> SampleResult<Vec<TmuxPane>> {
    capture_with(|| {
        read_command(
            "tmux",
            &["list-panes", "-a", "-F", FORMAT],
            &AtomicBool::new(false),
            Duration::from_secs(super::cache::FRESH_SECONDS as u64),
            (super::top::MAX_PROCESSES + 1) * libc::PATH_MAX as usize,
        )
    })
}

/// The production command boundary also accepts deterministic platform outcomes.
pub fn capture_with(run: impl FnOnce() -> SampleResult<Output>) -> SampleResult<Vec<TmuxPane>> {
    // Platform tool output is isolated. Ordinary missing server is not an error.
    let output = match run() {
        Err(e) if e.stage == "command_spawn" && e.code == format!("errno={}", libc::ENOENT) => {
            return Ok(vec![]);
        }
        result => result?,
    };
    if !output.success {
        let error = String::from_utf8_lossy(&output.stderr);
        if error.contains("no server running") || error.contains("No such file or directory") {
            return Ok(vec![]);
        }
        return Err(SampleError::new("tmux", "nonzero_exit"));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| SampleError::new("tmux", "invalid_utf8"))?;
    parse_panes(text)
}
