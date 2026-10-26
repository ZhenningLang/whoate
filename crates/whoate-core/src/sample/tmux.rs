use super::{
    SampleError, SampleResult,
    command::{Output, read_command},
};
use crate::snapshot::{TmuxClient, TmuxPane};
use std::{sync::atomic::AtomicBool, time::Duration};

pub const FORMAT: &str = "#{pane_id}\t#{session_name}:#{window_index}.#{pane_index}\t#{q/a:window_name}\t#{pane_pid}\t#{q/a:pane_current_path}\t#{q/a:pane_title}";
/// R25: listed after the panes in the same tmux call; the session is unescaped
/// like the pane target's, so the two compare as they are.
pub const CLIENT_FORMAT: &str = "client\t#{client_pid}\t#{client_session}\t#{client_activity}";

fn decode_arg(text: &str) -> SampleResult<String> {
    // q/a emits bare, double-quoted, or single-quoted vis-escaped tokens.
    // Decode vis octal as bytes before UTF-8, not as Unicode code points.
    let (text, single_quoted) = if let Some(text) = text.strip_prefix('\'') {
        (
            text.strip_suffix('\'')
                .filter(|s| !s.contains('\''))
                .ok_or_else(|| SampleError::new("tmux", "invalid_quoting"))?,
            true,
        )
    } else if let Some(text) = text.strip_prefix('"') {
        (
            text.strip_suffix('"')
                .ok_or_else(|| SampleError::new("tmux", "invalid_quoting"))?,
            false,
        )
    } else {
        (text, false)
    };
    let mut chars = text.bytes();
    let mut out = Vec::with_capacity(text.len());
    while let Some(c) = chars.next() {
        if c == b'"' && !single_quoted {
            return Err(SampleError::new("tmux", "invalid_quoting"));
        }
        if c != b'\\' {
            out.push(c);
            continue;
        }
        let c = chars
            .next()
            .ok_or_else(|| SampleError::new("tmux", "invalid_escape"))?;
        match c {
            b'n' => out.push(b'\n'),
            b't' => out.push(b'\t'),
            b'r' => out.push(b'\r'),
            b'e' => out.push(27),
            b'a' => out.push(7),
            b'b' => out.push(8),
            b'f' => out.push(12),
            b'v' => out.push(11),
            b'0'..=b'7' => {
                let mut n = u16::from(c - b'0');
                for _ in 0..2 {
                    n = n * 8
                        + chars
                            .next()
                            .filter(|c| (b'0'..=b'7').contains(c))
                            .map(|c| u16::from(c - b'0'))
                            .ok_or_else(|| SampleError::new("tmux", "invalid_octal"))?;
                }
                out.push(u8::try_from(n).map_err(|_| SampleError::new("tmux", "invalid_octal"))?);
            }
            c if c.is_ascii_punctuation() || c == b' ' => out.push(c),
            _ => return Err(SampleError::new("tmux", "invalid_escape")),
        }
    }
    String::from_utf8(out).map_err(|_| SampleError::new("tmux", "invalid_utf8"))
}

pub fn parse_panes(text: &str) -> SampleResult<Vec<TmuxPane>> {
    text.lines()
        .filter(|line| !line.is_empty())
        .map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            if !matches!(fields.len(), 5 | 6) || !valid_pane(fields[0]) {
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
                // Metadata failure is local: retain the validated pane/PID.
                // The native sampler reports missing metadata as a safe issue.
                window_name: if fields.len() == 6 {
                    decode_arg(fields[2]).ok()
                } else {
                    Some(fields[2].into())
                },
                pane_title: if fields.len() == 6 {
                    decode_arg(fields[5]).ok()
                } else {
                    None
                },
                pane_pid: pid,
                cwd: if fields.len() == 6 {
                    decode_arg(fields[4]).ok()
                } else {
                    Some(fields[4].into())
                },
            })
        })
        .collect()
}

/// Pane lines and R25's client lines from one output. A malformed client line is
/// skipped: clients only name the terminal, panes stay strict.
pub fn parse(text: &str) -> SampleResult<(Vec<TmuxPane>, Vec<TmuxClient>)> {
    let (clients, panes): (Vec<_>, Vec<_>) = text.lines().partition(|l| l.starts_with("client\t"));
    let clients = clients
        .iter()
        .filter_map(|line| {
            let fields: Vec<_> = line.split('\t').collect();
            let [_, pid, session, activity] = fields[..] else {
                return None;
            };
            Some(TmuxClient {
                pid: pid.parse().ok().filter(|pid| *pid > 0)?,
                session: session.into(),
                activity: activity.parse().ok(),
            })
        })
        .collect();
    Ok((parse_panes(&panes.join("\n"))?, clients))
}

pub fn valid_pane(value: &str) -> bool {
    value
        .strip_prefix('%')
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|c| c.is_ascii_digit()))
}

pub fn capture() -> SampleResult<(Vec<TmuxPane>, Vec<TmuxClient>)> {
    capture_all_with(|| {
        read_command(
            "tmux",
            // read_command pins LC_ALL=C for numeric tools. Force tmux's UTF-8
            // client mode or it replaces tabs/Unicode with underscores.
            // R25: the clients come from the same call (";" separates tmux commands).
            &[
                "-u",
                "list-panes",
                "-a",
                "-F",
                FORMAT,
                ";",
                "list-clients",
                "-F",
                CLIENT_FORMAT,
            ],
            &AtomicBool::new(false),
            Duration::from_secs(super::cache::FRESH_SECONDS as u64),
            (super::top::MAX_PROCESSES + 1) * libc::PATH_MAX as usize,
        )
    })
}

/// The production command boundary also accepts deterministic platform outcomes.
pub fn capture_with(run: impl FnOnce() -> SampleResult<Output>) -> SampleResult<Vec<TmuxPane>> {
    capture_all_with(run).map(|(panes, _)| panes)
}

/// Panes and clients (R25).
pub fn capture_all_with(
    run: impl FnOnce() -> SampleResult<Output>,
) -> SampleResult<(Vec<TmuxPane>, Vec<TmuxClient>)> {
    // Platform tool output is isolated. Ordinary missing server is not an error.
    let output = match run() {
        Err(e) if e.stage == "command_spawn" && e.code == format!("errno={}", libc::ENOENT) => {
            return Ok((vec![], vec![]));
        }
        result => result?,
    };
    if !output.success {
        let error = String::from_utf8_lossy(&output.stderr);
        if error.contains("no server running") || error.contains("No such file or directory") {
            return Ok((vec![], vec![]));
        }
        return Err(SampleError::new("tmux", "nonzero_exit"));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| SampleError::new("tmux", "invalid_utf8"))?;
    parse(text)
}
