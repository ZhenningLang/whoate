mod cli;
mod locale;
mod platform;
mod privacy;
mod render;
mod terminate;
mod watch;
#[cfg(test)]
use clap::Parser;
use std::io::{self, Write};
use whoate_core::{
    engine::{Context, Engine, SystemClock},
    recognizers::ConfigReader,
    sample::SampleError,
};

fn main() {
    let args = cli::parse();
    let code = match run(&args) {
        Ok(code) => code,
        Err(error) => {
            if error.kind() == io::ErrorKind::BrokenPipe {
                0
            } else {
                eprintln!("whoate: output_failed ({:?})", error.kind());
                1
            }
        }
    };
    std::process::exit(code);
}
fn fatal(args: &cli::Cli, code: i32, message: &str) -> io::Result<i32> {
    eprintln!("whoate: {}", render::safe(message));
    if args.json {
        writeln!(
            io::stdout().lock(),
            "{}",
            serde_json::json!({"schema":"whoate/v1","error":{"code":code,"message":message}})
        )?;
    }
    Ok(code)
}
fn engine(
    args: &cli::Cli,
    language: locale::Language,
) -> Result<Engine<platform::NativeSampler, SystemClock>, SampleError> {
    let cfg = ConfigReader::from_env().map_err(|code| SampleError::new("config", code))?;
    Engine::try_with_sampler(
        platform::NativeSampler::new(args.fresh)?,
        SystemClock::default(),
        Context {
            cfg,
            uid: platform::uid(),
            language,
            platform_probe: whoate_core::kill::platform_binary,
        },
    )
}
fn run(args: &cli::Cli) -> io::Result<i32> {
    if matches!(args.command, Some(cli::Command::AgentGuide)) {
        io::stdout()
            .lock()
            .write_all(include_bytes!("../../../skills/whoate/SKILL.md"))?;
        return Ok(0);
    }
    let language = locale::system();
    if matches!(args.command, Some(cli::Command::Insights { .. })) {
        let entries = match whoate_core::insights::registry::load() {
            Ok(entries) => entries,
            Err(code) => return fatal(args, 1, code),
        };
        let text = render::insights(&entries, language);
        io::stdout().lock().write_all(text.as_bytes())?;
        return Ok(0);
    }
    let mut engine = match engine(args, language) {
        Ok(engine) => engine,
        Err(error) => return fatal(args, 1, &error.to_string()),
    };
    if let Some(cli::Command::Watch {
        interval,
        json_lines,
    }) = &args.command
    {
        return watch::run(&mut engine, args, *interval, *json_lines, language);
    }
    let report = match engine.sample_once() {
        Ok(report) => report,
        Err(error) => return fatal(args, 1, &error.to_string()),
    };
    let snapshot = engine
        .latest_snapshot()
        .expect("successful sample has snapshot");
    if let Some(cli::Command::Kill {
        targets,
        yes,
        dry_run,
    }) = &args.command
    {
        use std::io::IsTerminal;
        let plan = whoate_core::kill::plan(targets, &report, snapshot);
        let terminate = if args.json {
            terminate::run_json
        } else {
            terminate::run
        };
        return terminate(
            &plan,
            terminate::Options {
                yes: *yes,
                dry_run: *dry_run,
                tty: io::stdin().is_terminal(),
                language,
            },
            &mut whoate_core::kill::MacBackend,
            &mut io::stdin().lock(),
            &mut io::stdout().lock(),
            &mut io::stderr().lock(),
        );
    }
    let (output, code) = match public_view(&report, snapshot, args) {
        Ok(result) => result,
        Err(reason) => return fatal(args, 3, reason),
    };
    let text = if args.json {
        serde_json::to_string(&output).map_err(io::Error::other)? + "\n"
    } else {
        render::report(&output, engine.catalog.all(), language)
    };
    io::stdout().lock().write_all(text.as_bytes())?;
    Ok(code)
}

fn public_view(
    report: &whoate_core::model::Report,
    snapshot: &whoate_core::snapshot::Snapshot,
    args: &cli::Cli,
) -> Result<(whoate_core::model::Report, i32), &'static str> {
    // Gather prompt-derived content from the complete Report before filtering.
    // Otherwise a selected fact can repeat an off-view session's hidden prompt.
    let output = privacy::project(
        report,
        snapshot,
        privacy::Policy {
            prompts: args.prompts,
            redact: args.redact,
        },
    );
    cli::view(&output, snapshot, args)
}

#[cfg(test)]
mod scenarios {
    use super::*;
    use std::{collections::VecDeque, fs, time::Duration};
    use whoate_core::{
        engine::Clock,
        model::*,
        sample::{SampleResult, Sampler},
        snapshot::*,
    };
    struct Replay(VecDeque<Snapshot>);
    impl Sampler for Replay {
        fn sample(&mut self) -> SampleResult<Snapshot> {
            Ok(self.0.pop_front().unwrap())
        }
    }
    #[derive(Default)]
    struct Time(u128);
    impl Clock for Time {
        fn now_ns(&self) -> u128 {
            self.0
        }
        fn wait(&mut self, d: Duration) {
            self.0 += d.as_nanos();
        }
    }
    #[test]
    fn phase07_output_top_why_agents_keep_host_and_show_new_fields_without_prompts() {
        use whoate_core::{
            sample::system_memory::{KernelZone, MemoryCache},
            snapshot::TmuxPane,
        };
        for language in [locale::Language::En, locale::Language::Zh] {
            let home = platform::Scratch::new().unwrap();
            let session = home.0.join(".claude/sessions");
            fs::create_dir_all(&session).unwrap();
            fs::write(
                session.join("940002.json"),
                r#"{"sessionId":"p07-output","cwd":"/demo","status":"idle"}"#,
            )
            .unwrap();
            let transcript = home.0.join(".claude/projects/-demo");
            fs::create_dir_all(&transcript).unwrap();
            fs::write(transcript.join("p07-output.jsonl"),concat!("{\"type\":\"user\",\"timestamp\":\"2026-10-16T00:00:00Z\",\"message\":{\"content\":\"PROMPT_SENTINEL\"}}\n","{\"type\":\"assistant\",\"timestamp\":\"2026-10-16T00:00:01Z\",\"message\":{\"content\":\"REPLY_SENTINEL\"}}\n")).unwrap();
            let mut system = Proc::identity(940001, 1, 0, Some(1), "fseventsd".into());
            system.footprint_bytes = Some(6_012_954_214);
            let mut agent = Proc::identity(940002, 1, 501, Some(1), "claude".into());
            agent.footprint_bytes = Some(734_003_200);
            agent.resident_bytes = Some(4096);
            let mut snap = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![system, agent]);
            snap.host = Some(Host {
                mem_total_bytes: 16_000_000_000,
                mem_used_bytes: 8_000_000_000,
                compressed_bytes: 100,
                wired_bytes: 2048,
                swap_used_bytes: 0,
                pressure: Pressure::Normal,
                kernel_zones: None,
                gpu_memory: None,
                memory_diagnostics: None,
                cpu: None,
            });
            let mut memory = MemoryCache::missing("fixture");
            memory.kernel_zones.status = whoate_core::snapshot::Freshness::Fresh;
            memory.kernel_zones.as_of = Some("2026-10-17T00:00:00Z".into());
            memory.kernel_zones.reason = None;
            memory.kernel_zones.top = vec![KernelZone {
                name: "data.kalloc.1024".into(),
                bytes: 4_294_967_296,
                collectible: Some(false),
                pageable: Some(false),
            }];
            memory.merge(snap.host.as_mut().unwrap(), 1_792_195_200);
            snap.tmux_panes = vec![TmuxPane {
                pane_id: "%42".into(),
                target: "work:0.0".into(),
                window_name: Some("window 雪".into()),
                pane_title: Some("pane\tline\n\u{1b}".into()),
                pane_pid: 940002,
                cwd: Some("/demo".into()),
            }];
            let mut engine = Engine::with_sampler(
                Replay(VecDeque::from([snap.clone(), snap.clone()])),
                Time::default(),
                Context {
                    cfg: ConfigReader::isolated(home.0.clone()),
                    uid: 501,
                    language,
                    platform_probe: |_| Ok(false),
                },
            );
            let report = engine.sample_once().unwrap();
            assert_eq!(
                report
                    .host
                    .as_ref()
                    .unwrap()
                    .memory_diagnostics
                    .as_ref()
                    .unwrap()[0]
                    .level,
                Level::Act
            );
            for args in [
                vec!["whoate", "top", "-n", "1", "--json"],
                vec!["whoate", "why", "940002", "--json"],
                vec!["whoate", "agents", "--json"],
            ] {
                let cli = cli::Cli::try_parse_from(args).unwrap();
                let (view, _) = public_view(&report, &snap, &cli).unwrap();
                assert_eq!(view.host, report.host);
                assert_eq!(view.owners.len(), 1);
                let text = render::report(&view, engine.catalog.all(), language);
                assert!(text.contains("wired=2048 bytes"));
                assert!(
                    text.lines()
                        .any(|line| line.starts_with("  data.kalloc.1024: 4294967296 bytes;"))
                );
                assert!(text.contains("GPU=Missing"));
                assert!(!text.contains("PROMPT_SENTINEL"));
                assert!(!text.contains("REPLY_SENTINEL"));
                if let Some(a) = &view.owners[0].agent {
                    assert_eq!(a.idle_seconds, Some(86_399));
                    assert_eq!(a.first_prompt, None);
                    assert_eq!(a.first_reply, None);
                    assert!(text.contains("2026-10-16T00:00:01Z"));
                    assert!(text.contains("4096 bytes"));
                    assert!(text.contains("window 雪"));
                    assert!(!text.contains('\u{1b}'));
                    assert!(!text.contains("pane\tline\n"));
                    let mut redact = cli;
                    redact.redact = true;
                    let (redacted, _) = public_view(&report, &snap, &redact).unwrap();
                    let origin = redacted.owners[0].origin.as_ref().unwrap();
                    assert_eq!(origin.pane_title.as_deref(), Some("<redacted>"));
                    assert_eq!(origin.window_name.as_deref(), Some("<redacted>"));
                }
            }
        }
    }
    #[test]
    fn phase07_output_nonroot_platform_restart_keeps_verified_authority_only() {
        let home = platform::Scratch::new().unwrap();
        let mut proc = Proc::identity(940001, 1, 89, Some(1), "mds_stores".into());
        proc.footprint_bytes = Some(5_000_000_000);
        let snap = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![proc]);
        let mut engine = Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap.clone()])),
            Time::default(),
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: locale::Language::En,
                platform_probe: |_| Ok(true),
            },
        );
        let report = engine.sample_once().unwrap();
        let cli = cli::Cli::try_parse_from(["whoate", "why", "940001", "--json"]).unwrap();
        let (view, _) = public_view(&report, &snap, &cli).unwrap();
        assert_eq!(
            view.owners[0].verdict.action.as_deref(),
            Some("sudo launchctl kickstart -k system/com.apple.metadata.mds")
        );
        // Exported strings alone must not grant the trusted-action exemption.
        let untrusted: Report =
            serde_json::from_value(serde_json::to_value(&report).unwrap()).unwrap();
        let (view, _) = public_view(&untrusted, &snap, &cli).unwrap();
        assert_eq!(
            view.owners[0].verdict.action.as_deref(),
            Some("whoate kill 940001")
        );
    }
    #[test]
    fn phase07_output_system_restart_survives_public_view() {
        let home = platform::Scratch::new().unwrap();
        let mut proc = Proc::identity(940001, 1, 0, Some(1), "fseventsd".into());
        proc.footprint_bytes = Some(6_012_954_214);
        let snap = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![proc]);
        let mut engine = Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap.clone()])),
            Time::default(),
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: locale::Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        let input = engine.sample_once().unwrap();
        let projected = privacy::project(&input, &snap, privacy::Policy::default());
        assert_eq!(
            projected.owners[0].verdict.action.as_deref(),
            Some("sudo launchctl kickstart -k system/com.apple.fseventsd")
        );
        let text = render::report(&projected, engine.catalog.all(), locale::Language::En);
        assert!(text.contains("Requires administrator privileges."));
        assert!(text.contains("watchers"));
        assert!(!text.contains("whoate kill"));
        let mut forged = input.clone();
        forged.owners[0].verdict.action = Some("sudo rm -rf /".into());
        assert_eq!(
            privacy::project(&forged, &snap, privacy::Policy::default()).owners[0]
                .verdict
                .action
                .as_deref(),
            Some("whoate kill 940001")
        );
    }
    #[test]
    fn phase04_render_real_docker_facts_have_units_and_separate_accounting() {
        use whoate_core::recognizers::docker::{ContainerStats, DockerSource};
        struct Containers;
        impl DockerSource for Containers {
            fn capture(
                &mut self,
                _: &std::sync::atomic::AtomicBool,
            ) -> Result<Vec<ContainerStats>, &'static str> {
                whoate_core::recognizers::docker::parse_stats(
                    r#"{"ID":"abc123","Name":"test-container","MemUsage":"3.5MiB / 2GiB"}"#,
                )
            }
        }
        let home = platform::Scratch::new().unwrap();
        let plist = home.0.join("Docker.app/Contents/Info.plist");
        fs::create_dir_all(plist.parent().unwrap()).unwrap();
        fs::write(plist, "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.docker.docker</string></dict></plist>").unwrap();
        let mut proc = Proc::identity(940001, 1, 501, Some(1), "com.docker.backend".into());
        proc.exe_path = Some(
            home.0
                .join("Docker.app/Contents/MacOS/com.docker.backend")
                .to_string_lossy()
                .into_owned(),
        );
        proc.footprint_bytes = Some(100);
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![proc]);
        let cfg = ConfigReader::isolated(home.0.clone()).with_docker_source(Containers);
        let monitor = cfg.docker_monitor();
        let mut engine = Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap.clone(), snap])),
            Time::default(),
            Context {
                cfg,
                uid: 501,
                language: locale::Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        engine.tick().unwrap();
        assert!(monitor.wait_for_capture(Duration::from_secs(2)));
        let mut input = engine.tick().unwrap();
        assert!(
            input.owners[0]
                .facts
                .iter()
                .any(|f| f.key == "docker_container.abc123.memory_bytes"
                    && f.value == FactValue::Count(3_670_016))
        );
        input.owners[0].facts.push(Fact {
            key: "docker_memory_limit_bytes".into(),
            label: "VM limit".into(),
            value: FactValue::Count(999),
        });
        for (language, annotation) in [
            (locale::Language::En, "independent Linux"),
            (locale::Language::Zh, "Linux独立统计"),
        ] {
            let text = render::report(&input, &[], language);
            let container = text
                .lines()
                .find(|s| s.contains("[docker_container.abc123.memory_bytes]"))
                .unwrap();
            assert!(container.contains("3670016 bytes") && container.contains(annotation));
            let vm = text
                .lines()
                .find(|s| s.contains("[docker_memory_limit_bytes]"))
                .unwrap();
            assert!(!vm.contains(annotation));
        }
        assert_eq!(input.owners[0].mem_bytes, 100);
    }
    #[test]
    fn phase04_holdout_frozen_mixed_unknowns_resources_and_privacy() {
        let data: serde_json::Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/cli/frozen-holdout.json"
        ))
        .unwrap();
        let mut snapshot = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![]);
        snapshot.host = Some(Host {
            kernel_zones: None,
            gpu_memory: None,
            memory_diagnostics: None,
            cpu: None,
            mem_total_bytes: 2048,
            mem_used_bytes: 1024,
            compressed_bytes: 10,
            wired_bytes: 20,
            swap_used_bytes: 0,
            pressure: Pressure::Normal,
        });
        let mut r = Report::from_snapshot(&snapshot);
        for row in data["owners"].as_array().unwrap() {
            let kind: OwnerKind = serde_json::from_value(row["kind"].clone()).unwrap();
            let mut o = Owner::new(
                row["id"].as_str().unwrap().into(),
                kind,
                "Unseen owner".into(),
            );
            o.pids = vec![row["pid"].as_i64().unwrap() as i32];
            o.mem_bytes = row["mem_bytes"].as_u64().unwrap();
            o.cpu_pct = row["cpu_pct"].as_f64();
            r.owners.push(o);
        }
        let secret = data["secret"].as_str().unwrap();
        let mut agent = Agent::unknown("claude");
        agent.first_prompt = Some(secret.into());
        agent.name = Some("Unseen session".into());
        r.owners[0].agent = Some(agent);
        r.owners[2].facts.push(Fact {
            key: "docker_container.example.memory_bytes".into(),
            label: "Linux memory".into(),
            value: FactValue::Count(70000),
        });
        r.owners[2].facts.push(Fact {
            key: "user_advice".into(),
            label: "Advice".into(),
            value: FactValue::Text(secret.into()),
        });
        r.owners[3].verdict.level = Level::Act;
        let mut p = Proc::identity(941001, 1, 501, Some(1), "claude".into());
        p.argv = vec!["claude".into(), format!("--question={secret}")];
        snapshot.procs.push(p);
        assert_eq!(
            r.owners.iter().map(|o| o.mem_bytes).sum::<u64>(),
            data["expected_total_known_bytes"].as_u64().unwrap()
        );
        let (top, code) = cli::view(
            &r,
            &snapshot,
            &cli::Cli::parse_from(["whoate", "top", "-n", "2"]),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(top.owners.iter().map(|o| &o.id).collect::<Vec<_>>()).unwrap(),
            data["expected_top_ids"]
        );
        assert_eq!(
            top.owners.iter().map(|o| o.mem_bytes).sum::<u64>(),
            data["expected_top_known_bytes"].as_u64().unwrap()
        );
        assert_eq!(code, 1);
        let (output, _) = public_view(
            &r,
            &snapshot,
            &cli::Cli::parse_from(["whoate", "top", "-n", "2"]),
        )
        .unwrap();
        assert!(!serde_json::to_string(&output).unwrap().contains(secret));
        assert_eq!(output.host.as_ref().unwrap().mem_used_bytes, 1024);
        let (cpu, _) = cli::view(
            &r,
            &snapshot,
            &cli::Cli::parse_from(["whoate", "--by", "cpu"]),
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(cpu.owners.iter().map(|o| &o.id).collect::<Vec<_>>()).unwrap(),
            data["expected_cpu_ids"]
        );
        let (why, status) = cli::view(
            &r,
            &snapshot,
            &cli::Cli::parse_from(["whoate", "why", "941001"]),
        )
        .unwrap();
        assert_eq!(status, 0);
        let output = privacy::project(&why, &snapshot, privacy::Policy::default());
        assert!(
            output.owners[0]
                .agent
                .as_ref()
                .unwrap()
                .first_prompt
                .is_none()
        );
        println!("QA24_FROZEN_HOLDOUT_FIRST_RUN_PASS");
    }
    #[test]
    fn phase04_agent_source_isolated_session_projects_only_authorized_prompts() {
        let home = platform::Scratch::new().unwrap();
        let sessions = home.0.join(".claude/sessions");
        fs::create_dir_all(&sessions).unwrap();
        fs::write(sessions.join("940001.json"),r#"{"sessionId":"cli-example","name":"CLI Session","nameSource":"manual","cwd":"/public/project","status":"working"}"#).unwrap();
        let transcripts = home.0.join(".claude/projects/-public-project");
        fs::create_dir_all(&transcripts).unwrap();
        fs::write(transcripts.join("cli-example.jsonl"),concat!(
            "{\"type\":\"user\",\"message\":{\"content\":\"<system-reminder>HIDDEN_WRAPPER</system-reminder>Explain this process\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"I will inspect evidence\"}],\"model\":\"example\"}}\n",
            "{\"type\":\"ai-title\",\"aiTitle\":\"CLI Example\"}\n")).unwrap();
        let mut p = Proc::identity(940001, 1, 501, Some(1), "claude".into());
        p.exe_path = Some("/public/claude".into());
        p.argv = vec!["claude".into()];
        p.footprint_bytes = Some(100);
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut engine = Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap])),
            Time::default(),
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: locale::Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        let report = engine.sample_once().unwrap();
        let (view, code) = cli::view(
            &report,
            engine.latest_snapshot().unwrap(),
            &cli::Cli::parse_from(["whoate", "why", "940001"]),
        )
        .unwrap();
        assert_eq!(code, 0);
        for prompts in [false, true] {
            let output = privacy::project(
                &view,
                engine.latest_snapshot().unwrap(),
                privacy::Policy {
                    prompts,
                    redact: false,
                },
            );
            let value = serde_json::to_value(&output).unwrap();
            let agent = &value["owners"][0]["agent"];
            assert_eq!(agent["name"], "CLI Session");
            assert_eq!(agent["title"], "CLI Example");
            if prompts {
                assert_eq!(agent["first_prompt"], "Explain this process");
                assert_eq!(agent["first_reply"], "I will inspect evidence");
            } else {
                assert!(agent["first_prompt"].is_null());
                assert!(agent["first_reply"].is_null());
            }
            let text = render::report(&output, engine.catalog.all(), locale::Language::En);
            assert_eq!(text.contains("Explain this process"), prompts);
            assert!(!text.contains("HIDDEN_WRAPPER"));
        }
        assert!(
            report.owners[0]
                .agent
                .as_ref()
                .unwrap()
                .first_prompt
                .is_some()
        );
        assert!(
            !report.owners[0]
                .facts
                .iter()
                .any(|f| f.key.starts_with("count."))
        );
    }
    #[test]
    fn phase04_json_snapshot_preserves_additive_fields_units_null_and_arrays() {
        let mut r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        r.owners[0].cpu_pct = Some(250.5);
        r.owners[0].unknown_mem_count = 1;
        r.owners[0].parts[0].unknown_mem_count = 1;
        let snap = Snapshot::historical(r.taken_at.clone(), vec![]);
        let output = privacy::project(&r, &snap, privacy::Policy::default());
        let value = serde_json::to_value(&output).unwrap();
        let expected: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/cli/output-v1.json"))
                .unwrap();
        assert_eq!(value["schema"], "whoate/v1");
        assert_eq!(value["taken_at"], "2026-10-14T19:40:00+08:00");
        assert_eq!(value["host"]["wired_bytes"], 2147483648_u64);
        assert_eq!(value["owners"][0]["mem_bytes"], 1572864000_u64);
        assert_eq!(value["owners"][0]["cpu_pct"], 250.5);
        assert_eq!(value["owners"][0]["unknown_mem_count"], 1);
        assert_eq!(value["owners"][0]["parts"][0]["unknown_mem_count"], 1);
        assert_eq!(value["owners"][0]["history"], serde_json::json!([]));
        assert_eq!(value["owners"][0]["agent"]["name_source"], "manual");
        assert_eq!(
            value["owners"][0]["agent"]["updated_at_ms"],
            1791978000000_u64
        );
        assert_eq!(value["owners"][0]["agent"]["stale"], false);
        assert!(value["owners"][0]["agent"]["first_prompt"].is_null());
        assert!(value["owners"][0]["verdict"].get("action").is_none());
        let empty = serde_json::to_value(Report::from_snapshot(&snap)).unwrap();
        assert!(empty["host"].is_null());
        assert_eq!(empty["issues"], serde_json::json!([]));
        assert_eq!(value, expected, "complete CLI wire snapshot");
    }
    #[test]
    fn phase04_burst_window_uses_snapshot_time_inclusive_600_seconds() {
        for (age, expected) in [
            (599_999_999_i64, 1),
            (600_000_000, 1),
            (600_000_001, 0),
            (-1, 0),
        ] {
            let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![]);
            for i in 0..30 {
                let mut p = Proc::identity(
                    940001 + i,
                    940099,
                    501,
                    Some((1791978000000000_i64 - age) as u64),
                    "sleep".into(),
                );
                p.argv = vec!["sleep".into(), "600".into()];
                snap.procs.push(p);
            }
            let groups = whoate_core::swarm::detect(&snap, &[]);
            assert_eq!(groups.len(), expected, "age={age}");
            if expected == 1 {
                assert_eq!(groups[0].pids, (940001..940031).collect::<Vec<_>>());
                assert_eq!(groups[0].parents, vec![940099]);
            }
        }
    }

    #[test]
    fn phase07_fresh_bypasses_new_memory_cache_without_changing_host_totals() {
        use whoate_core::sample::system_memory::{BASIS, GpuMemory, KernelZone, KernelZones};
        let home = platform::Scratch::new().unwrap();
        let mut own = Proc::identity(940001, 1, 501, Some(1), "shell".into());
        own.footprint_bytes = Some(123);
        let mut system = Proc::identity(940002, 1, 0, Some(1), "service".into());
        system.footprint_bytes = Some(456);
        system.cpu_pct_sampled = Some(7.0);
        let mut cached = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![own, system]);
        cached.system_procs = SystemProcs {
            status: Freshness::Fresh,
            as_of: Some(cached.taken_at.clone()),
        };
        cached.host = Some(Host {
            mem_total_bytes: 16_000,
            mem_used_bytes: 8_000,
            wired_bytes: 2_000,
            compressed_bytes: 300,
            swap_used_bytes: 400,
            pressure: Pressure::Warn,
            kernel_zones: Some(KernelZones {
                status: Freshness::Fresh,
                as_of: Some(cached.taken_at.clone()),
                basis: BASIS.into(),
                reason: None,
                top: vec![KernelZone {
                    name: "CACHE_SENTINEL".into(),
                    bytes: 4_294_967_296,
                    collectible: Some(false),
                    pageable: Some(false),
                }],
            }),
            // Deliberately populated: fresh must remove any cached GPU value too.
            gpu_memory: Some(GpuMemory {
                status: Freshness::Fresh,
                as_of: Some(cached.taken_at.clone()),
                bytes: Some(777),
                source: Some("CACHE_SENTINEL".into()),
                reason: None,
            }),
            memory_diagnostics: None,
            cpu: None,
        });
        for fresh in [false, true] {
            let mut snap = cached.clone();
            if fresh {
                platform::without_slow_cache(&mut snap, 501);
            }
            let mut engine = Engine::with_sampler(
                Replay(VecDeque::from([snap.clone(), snap.clone()])),
                Time::default(),
                Context {
                    cfg: ConfigReader::isolated(home.0.clone()),
                    uid: 501,
                    language: locale::Language::En,
                    platform_probe: |_| Ok(false),
                },
            );
            let report = engine.sample_once().unwrap();
            let args = cli::Cli::try_parse_from(["whoate", "top", "--json"]).unwrap();
            let (view, _) = public_view(&report, &snap, &args).unwrap();
            let value = serde_json::to_value(&view).unwrap();
            let h = &value["host"];
            assert_eq!(
                (
                    h["mem_total_bytes"].as_u64(),
                    h["mem_used_bytes"].as_u64(),
                    h["wired_bytes"].as_u64(),
                    h["compressed_bytes"].as_u64(),
                    h["swap_used_bytes"].as_u64()
                ),
                (Some(16_000), Some(8_000), Some(2_000), Some(300), Some(400))
            );
            assert_eq!(h["pressure"], "warn");
            assert_eq!(snap.procs[0].footprint_bytes, Some(123));
            if fresh {
                assert_eq!(h["kernel_zones"]["status"], "missing");
                assert_eq!(h["kernel_zones"]["reason"], "fresh_requested");
                assert_eq!(h["kernel_zones"]["top"], serde_json::json!([]));
                assert_eq!(h["gpu_memory"]["status"], "missing");
                assert!(h["gpu_memory"]["bytes"].is_null());
                assert!(
                    !serde_json::to_string(&view)
                        .unwrap()
                        .contains("CACHE_SENTINEL")
                );
                assert_eq!(snap.system_procs, SystemProcs::missing());
                assert_eq!(
                    (snap.procs[1].footprint_bytes, snap.procs[1].cpu_pct_sampled),
                    (None, None)
                );
            } else {
                assert_eq!(h["kernel_zones"]["top"][0]["name"], "CACHE_SENTINEL");
                assert_eq!(h["kernel_zones"]["top"][0]["bytes"], 4_294_967_296_u64);
                assert_eq!(h["gpu_memory"]["bytes"], 777);
            }
        }
    }
}
