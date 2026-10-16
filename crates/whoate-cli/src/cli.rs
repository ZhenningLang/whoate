use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum, parser::ValueSource};
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(
    name = "whoate",
    version,
    about = "Attribute macOS process resources to their owners",
    after_help = "Read-only validation capability: WHOATE_SESSION_PID=<positive Claude PID> restricts all session sources before open to that PID, and its transcript to the first user/assistant record pair (no tail or read-ahead). Other session sources are not opened. Unset preserves normal behavior; this never grants permission."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,
    #[arg(long, global = true)]
    pub json: bool,
    #[arg(long, global = true, conflicts_with = "redact")]
    pub prompts: bool,
    #[arg(long, global = true)]
    pub redact: bool,
    #[arg(long, global = true)]
    pub fresh: bool,
    #[arg(long, global = true, value_enum, default_value = "mem")]
    pub by: Sort,
    #[arg(short = 'n', global = true, default_value_t = 10)]
    pub count: usize,
}
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum Sort {
    Mem,
    Cpu,
}
#[derive(Debug, Subcommand)]
pub enum Command {
    Top,
    Why {
        #[arg(value_parser = kill_target)]
        target: String,
    },
    Orphans,
    Swarms,
    Pane {
        #[arg(value_parser = pane_target)]
        target: String,
    },
    Agents,
    Kill {
        #[arg(required = true, num_args = 1.., value_parser = kill_target)]
        targets: Vec<String>,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        yes: bool,
    },
    Watch {
        #[arg(long, default_value = "2s", value_parser = interval)]
        interval: Duration,
        #[arg(long)]
        json_lines: bool,
    },
    AgentGuide,
    Insights {
        #[arg(long, required = true)]
        list: bool,
    },
}

/// Global flags stay available before/after a subcommand, but only where their
/// behavior is defined. Check value sources so explicit defaults are not no-ops.
pub fn parse() -> Cli {
    let matches = Cli::command().get_matches();
    let (name, selected) = matches.subcommand().unwrap_or(("top", &matches));
    let query = matches!(
        name,
        "top" | "why" | "orphans" | "swarms" | "pane" | "agents"
    );
    for flag in ["json", "prompts", "redact", "fresh", "by", "count"] {
        let supplied = selected.value_source(flag) == Some(ValueSource::CommandLine)
            || matches.value_source(flag) == Some(ValueSource::CommandLine);
        let allowed = match flag {
            "by" | "count" => name == "top",
            "json" => query,
            _ => query || name == "watch",
        };
        if supplied && !allowed {
            Cli::command()
                .error(
                    clap::error::ErrorKind::ArgumentConflict,
                    format!("{flag} is not supported by {name}"),
                )
                .exit();
        }
    }
    Cli::from_arg_matches(&matches).unwrap_or_else(|error| error.exit())
}
fn positive_pid(value: &str) -> Result<i32, String> {
    value
        .parse::<i32>()
        .ok()
        .filter(|n| *n > 0)
        .ok_or_else(|| "PID must be a positive 32-bit integer".into())
}
fn kill_target(value: &str) -> Result<String, String> {
    if value.bytes().all(|b| b.is_ascii_digit()) || value.starts_with('-') {
        positive_pid(value)?;
    } else if !value.contains(':') || value.chars().any(char::is_control) {
        return Err("expected a positive PID or owner-id".into());
    }
    Ok(value.into())
}
fn pane_target(value: &str) -> Result<String, String> {
    let valid_id = value
        .strip_prefix('%')
        .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()));
    let valid_target = value.rsplit_once(':').is_some_and(|(session, rest)| {
        !session.is_empty()
            && rest.split_once('.').is_some_and(|(w, p)| {
                !w.is_empty()
                    && !p.is_empty()
                    && w.bytes().chain(p.bytes()).all(|b| b.is_ascii_digit())
            })
    });
    if (valid_id || valid_target) && !value.chars().any(char::is_control) {
        Ok(value.into())
    } else {
        Err("expected %id or session:window.pane".into())
    }
}
fn interval(value: &str) -> Result<Duration, String> {
    let seconds = if let Some(ms) = value.strip_suffix("ms") {
        ms.parse::<f64>().ok().map(|n| n / 1000.0)
    } else {
        value.strip_suffix('s').and_then(|s| s.parse::<f64>().ok())
    };
    seconds
        .filter(|s| s.is_finite() && *s >= 1.0)
        .and_then(|s| Duration::try_from_secs_f64(s).ok())
        .ok_or_else(|| "interval must be at least 1s (seconds or milliseconds)".into())
}

use whoate_core::{
    model::{Level, OwnerKind, Report},
    snapshot::Snapshot,
};

pub fn view(
    input: &Report,
    snapshot: &Snapshot,
    args: &Cli,
) -> Result<(Report, i32), &'static str> {
    let mut output = input.clone();
    match &args.command {
        Some(Command::Why { target }) => output.owners.retain(|o| {
            o.id == *target || target.parse::<i32>().is_ok_and(|pid| o.pids.contains(&pid))
        }),
        Some(Command::Pane { target }) => {
            let pane = if target.starts_with('%') {
                target.as_str()
            } else {
                snapshot
                    .tmux_panes
                    .iter()
                    .find(|p| p.target == *target)
                    .map(|p| p.pane_id.as_str())
                    .ok_or("target_missing")?
            };
            output
                .owners
                .retain(|o| o.origin.as_ref().and_then(|o| o.tmux_pane.as_deref()) == Some(pane));
        }
        Some(Command::Orphans) => output.owners.retain(|o| o.kind == OwnerKind::Orphan),
        Some(Command::Agents) => output.owners.retain(|o| o.agent.is_some()),
        Some(Command::Swarms) => output.owners.retain(|o| {
            input
                .swarms
                .iter()
                .any(|s| s.owners.iter().any(|r| r.id == o.id))
        }),
        _ => {}
    }
    if matches!(
        args.command,
        Some(Command::Why { .. } | Command::Pane { .. })
    ) && output.owners.is_empty()
    {
        return Err("target_missing");
    }
    let top = matches!(args.command, None | Some(Command::Top));
    let act = matches!(
        args.command,
        None | Some(Command::Top | Command::Orphans | Command::Agents)
    ) && (output.owners.iter().any(|o| o.verdict.level == Level::Act)
        || input.swarms.iter().any(|s| {
            s.verdict.level == Level::Act
                && s.owners
                    .iter()
                    .any(|r| output.owners.iter().any(|o| o.id == r.id))
        }));
    if top {
        output.owners.sort_by(|a, b| {
            let order = match args.by {
                Sort::Mem => b.mem_bytes.cmp(&a.mem_bytes),
                Sort::Cpu => match (a.cpu_pct, b.cpu_pct) {
                    (Some(a), Some(b)) => b.total_cmp(&a),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                },
            };
            order.then_with(|| a.id.cmp(&b.id))
        });
        output.owners.truncate(args.count);
    }
    output.swarms.retain(|s| {
        s.owners
            .iter()
            .any(|r| output.owners.iter().any(|o| o.id == r.id))
    });
    Ok((output, i32::from(act)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use whoate_core::model::Owner;
    #[test]
    fn phase04_views_filters_keep_exact_targets_and_complete_swarm_distribution() {
        use whoate_core::{
            model::{Origin, SwarmOwner},
            snapshot::TmuxPane,
        };
        let mut r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        r.owners[0].origin.as_mut().unwrap().tmux_pane = Some("%42".into());
        let mut second = Owner::new("orphan:940002".into(), OwnerKind::Orphan, "orphan".into());
        second.pids = vec![940002];
        second.origin = Some(Origin {
            tmux_pane: Some("%42".into()),
            ..Origin::default()
        });
        second.verdict.level = Level::Act;
        r.owners.push(second);
        r.swarms[0].owners.push(SwarmOwner {
            id: "orphan:940002".into(),
            count: 1,
        });
        let mut snap = Snapshot::historical(r.taken_at.clone(), vec![]);
        snap.tmux_panes.push(TmuxPane {
            pane_id: "%42".into(),
            target: "work:0.1".into(),
            window_name: None,
            pane_pid: 900001,
            cwd: None,
        });
        for (args, ids, code) in [
            (vec!["why", "900001"], vec!["agent:claude:demo-session"], 0),
            (vec!["why", "orphan:940002"], vec!["orphan:940002"], 0),
            (vec!["orphans"], vec!["orphan:940002"], 1),
            (vec!["agents"], vec!["agent:claude:demo-session"], 1),
            (
                vec!["swarms"],
                vec!["agent:claude:demo-session", "orphan:940002"],
                0,
            ),
            (
                vec!["pane", "%42"],
                vec!["agent:claude:demo-session", "orphan:940002"],
                0,
            ),
            (
                vec!["pane", "work:0.1"],
                vec!["agent:claude:demo-session", "orphan:940002"],
                0,
            ),
        ] {
            let args = Cli::parse_from(std::iter::once("whoate").chain(args));
            let (view, status) = view(&r, &snap, &args).unwrap();
            assert_eq!(
                view.owners
                    .iter()
                    .map(|o| o.id.as_str())
                    .collect::<Vec<_>>(),
                ids
            );
            assert_eq!(view.swarms[0].owners.len(), 2);
            assert_eq!(status, code);
            assert_eq!(view.host, r.host);
        }
        assert_eq!(
            view(&r, &snap, &Cli::parse_from(["whoate", "why", "2147483647"])).unwrap_err(),
            "target_missing"
        );
        r.owners.clear();
        r.swarms.clear();
        assert_eq!(
            view(&r, &snap, &Cli::parse_from(["whoate", "orphans"]))
                .unwrap()
                .1,
            0
        );
    }
    #[test]
    fn phase04_args_all_valid_command_shapes_and_malformed_duration_table() {
        for args in [
            vec![],
            vec!["top", "--fresh", "--redact", "-n", "0"],
            vec!["why", "agent:claude:demo", "--prompts", "--json"],
            vec!["watch", "--json-lines", "--interval", "1000ms"],
            vec!["insights", "--list"],
            vec!["kill", "123", "pane:%42", "--yes", "--dry-run"],
        ] {
            assert!(Cli::try_parse_from(std::iter::once("whoate").chain(args)).is_ok());
        }
        for duration in [
            "",
            "NaNs",
            "infs",
            "-1s",
            "999999999999999999999999s",
            "1ms",
            "1ss",
            "1\0s",
        ] {
            assert!(Cli::try_parse_from(["whoate", "watch", "--interval", duration]).is_err());
        }
    }
    #[test]
    fn phase04_views_top_sorts_truncates_and_preserves_host() {
        let mut r = Report::from_snapshot(&Snapshot::historical(
            "2026-10-14T19:40:00+08:00".into(),
            vec![],
        ));
        for (id, mem, cpu) in [
            ("service:1", 10, Some(125.5)),
            ("service:2", 30, None),
            ("service:3", 20, Some(0.0)),
        ] {
            let mut o = Owner::new(id.into(), OwnerKind::Service, id.into());
            o.mem_bytes = mem;
            o.cpu_pct = cpu;
            r.owners.push(o);
        }
        r.owners[0].verdict.level = Level::Act;
        let snap = Snapshot::historical(r.taken_at.clone(), vec![]);
        let (top, code) = view(&r, &snap, &Cli::parse_from(["whoate", "top", "-n", "1"])).unwrap();
        assert_eq!(
            top.owners.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            vec!["service:2"]
        );
        assert_eq!(top.host, r.host);
        assert_eq!(code, 1, "act detection precedes display truncation");
        let (cpu, _) = view(&r, &snap, &Cli::parse_from(["whoate", "--by", "cpu"])).unwrap();
        assert_eq!(
            cpu.owners.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(),
            vec!["service:1", "service:3", "service:2"]
        );
    }

    #[test]
    fn phase04_views_default_top_has_ten_exact_rows_without_recounting_host() {
        let mut r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        r.owners.clear();
        r.swarms.clear();
        r.host.as_mut().unwrap().mem_used_bytes = 999;
        for number in 1..=12 {
            let mut owner = Owner::new(
                format!("service:{number}"),
                OwnerKind::Service,
                "process".into(),
            );
            owner.pids = vec![940000 + number];
            owner.mem_bytes = number as u64;
            r.owners.push(owner);
        }
        let snapshot = Snapshot::historical(r.taken_at.clone(), vec![]);
        for args in [vec!["whoate"], vec!["whoate", "top"]] {
            let (report, code) = view(&r, &snapshot, &Cli::parse_from(args)).unwrap();
            assert_eq!(
                report
                    .owners
                    .iter()
                    .map(|o| o.id.as_str())
                    .collect::<Vec<_>>(),
                vec![
                    "service:12",
                    "service:11",
                    "service:10",
                    "service:9",
                    "service:8",
                    "service:7",
                    "service:6",
                    "service:5",
                    "service:4",
                    "service:3"
                ]
            );
            assert_eq!(report.host.as_ref().unwrap().mem_used_bytes, 999);
            assert_eq!(code, 0);
        }
        let (empty, _) = view(
            &r,
            &snapshot,
            &Cli::parse_from(["whoate", "top", "-n", "0"]),
        )
        .unwrap();
        assert!(empty.owners.is_empty());
        assert_eq!(empty.host.as_ref().unwrap().mem_used_bytes, 999);
        for target in ["%999", "absent:0.0"] {
            assert_eq!(
                view(&r, &snapshot, &Cli::parse_from(["whoate", "pane", target])).unwrap_err(),
                "target_missing"
            );
        }
    }
}
