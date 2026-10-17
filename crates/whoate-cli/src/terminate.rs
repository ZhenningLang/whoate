use std::io::{self, BufRead, Write};
use whoate_core::kill::{self, Backend, KillPlan, KillReport, KillStatus};
use whoate_core::verdict::Language;

pub struct Options {
    pub yes: bool,
    pub dry_run: bool,
    pub tty: bool,
    pub language: Language,
}
pub fn run(
    plan: &KillPlan,
    options: Options,
    backend: &mut dyn Backend,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<i32> {
    run_inner(plan, options, backend, input, out, err, false)
}
pub fn run_json(
    plan: &KillPlan,
    options: Options,
    backend: &mut dyn Backend,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> io::Result<i32> {
    run_inner(plan, options, backend, input, out, err, true)
}
fn run_inner(
    plan: &KillPlan,
    options: Options,
    backend: &mut dyn Backend,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
    err: &mut dyn Write,
    json: bool,
) -> io::Result<i32> {
    let before = backend.measure_memory(plan);
    let ids: std::collections::BTreeSet<_> = plan.pids.iter().map(|p| p.pid).collect();
    let preview = format!(
        "{}: {ids:?}; {}: {}\n",
        options.language.choose("进程PID", "PIDs"),
        options.language.choose("不存在的目标", "missing targets"),
        plan.missing_targets.len()
    );
    if !json {
        out.write_all(preview.as_bytes())?;
        writeln!(
            out,
            "{}: {} bytes; {}",
            options
                .language
                .choose("预计可腾出物理内存", "Estimated physical reclaim"),
            before
                .target_resident_bytes
                .map(|n| n.to_string())
                .unwrap_or_else(|| "missing".into()),
            options.language.choose(
                "RSS 粗估，非保证；共享页可能重复",
                "RSS estimate, not guaranteed; shared pages may overlap"
            )
        )?;
    }
    // Flush preview before the first possible signal.
    out.flush()?;
    err.flush()?;
    if options.dry_run || !options.yes {
        if !json {
            writeln!(
                out,
                "{}",
                options.language.choose(
                    "仅预览；--yes 只确认 SIGTERM，不会自动升级 SIGKILL。",
                    "Preview only; --yes authorizes SIGTERM, never automatic SIGKILL."
                )
            )?;
        }
        let preview = kill::preview(plan, backend);
        let code = preview.exit_code();
        if json {
            write_json(
                &preview,
                false,
                whoate_core::kill::MemoryChange::new(before, None),
                out,
            )?;
        }
        return Ok(code);
    }
    let mut result = kill::execute(plan, false, backend);
    if !json {
        write_results(&result, out)?;
    }
    let still: std::collections::BTreeSet<_> = result
        .results
        .iter()
        .filter(|r| r.status == KillStatus::StillAlive)
        .map(|r| r.pid)
        .collect();
    if options.tty && !still.is_empty() {
        writeln!(
            err,
            "{}: {still:?}. {} [y/N]",
            options.language.choose("仍存活", "Still alive"),
            options.language.choose(
                "向原身份进程发送 SIGKILL？",
                "Send SIGKILL to the SAME process identities?"
            )
        )?;
        err.flush()?;
        let mut answer = String::new();
        // Only a terminal can reach this read. EOF never grants authorization.
        let confirmed = input.read_line(&mut answer)? > 0
            && matches!(
                answer.trim().to_ascii_lowercase().as_str(),
                "y" | "yes" | "是"
            );
        if confirmed {
            let force = KillPlan {
                pids: plan
                    .pids
                    .iter()
                    .filter(|p| still.contains(&p.pid))
                    .cloned()
                    .collect(),
                missing_targets: vec![],
            };
            let forced = kill::execute(&force, true, backend);
            for item in forced.results {
                if let Some(old) = result.results.iter_mut().find(|r| r.pid == item.pid) {
                    *old = item;
                }
            }
            if !json {
                write_results(&result, out)?;
            }
        }
    }
    let after = backend.measure_memory(plan);
    let memory = whoate_core::kill::MemoryChange::new(before, Some(after));
    if json {
        write_json(&result, true, memory, out)?;
    } else {
        writeln!(
            out,
            "{}: {:?} bytes; {}: {:?} bytes; {}",
            options
                .language
                .choose("主机已用内存实测变化", "Observed host memory change"),
            memory.physical_memory_change_bytes,
            options
                .language
                .choose("目标驻留变化", "Target resident change"),
            memory.target_resident_change_bytes,
            options.language.choose(
                "非因果归因，其他软件会影响数值",
                "Non-causal observation; other software may affect the value"
            )
        )?;
    }
    Ok(result.exit_code())
}
fn write_json(
    result: &KillReport,
    executed: bool,
    memory: whoate_core::kill::MemoryChange,
    out: &mut dyn Write,
) -> io::Result<()> {
    serde_json::to_writer(&mut *out, &serde_json::json!({"schema":"whoate/v1","executed":executed,"results":result.results,"missing_targets":result.missing_targets,"memory":memory})).map_err(io::Error::other)?;
    writeln!(out)
}
fn write_results(report: &KillReport, out: &mut dyn Write) -> io::Result<()> {
    for item in &report.results {
        let status = match item.status {
            KillStatus::Terminated => "terminated",
            KillStatus::StillAlive => "still_alive",
            KillStatus::Refused => "refused",
            KillStatus::Missing => "missing",
        };
        writeln!(
            out,
            "{} {status} {}",
            item.pid,
            item.reason.as_deref().unwrap_or("")
        )?;
    }
    if !report.missing_targets.is_empty() {
        writeln!(out, "missing_targets={}", report.missing_targets.len())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{collections::BTreeMap, time::Duration};
    use whoate_core::kill::{Identity, PlannedPid};
    struct Fake {
        memory: std::collections::VecDeque<whoate_core::kill::MemoryMeasurement>,
        events: Vec<&'static str>,
        reads: usize,
        ids: BTreeMap<i32, Identity>,
        sent: Vec<(i32, i32)>,
        waited: Vec<Duration>,
        platform: Result<bool, &'static str>,
        reuse: bool,
        identity_error: bool,
    }
    impl Fake {
        fn new() -> Self {
            Self {
                memory: Default::default(),
                events: vec![],
                reads: 0,
                ids: BTreeMap::from([(
                    940001,
                    Identity {
                        pid: 940001,
                        uid: 501,
                        start_time: Some(1),
                    },
                )]),
                sent: vec![],
                waited: vec![],
                platform: Ok(false),
                reuse: false,
                identity_error: false,
            }
        }
    }
    impl Backend for Fake {
        fn measure_memory(&mut self, _: &KillPlan) -> whoate_core::kill::MemoryMeasurement {
            self.events.push("measure");
            self.memory
                .pop_front()
                .unwrap_or_else(|| whoate_core::kill::MemoryMeasurement::missing("unavailable"))
        }
        fn self_pid(&self) -> i32 {
            940099
        }
        fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
            if self.identity_error {
                return Err("identity_probe_failed");
            }
            self.reads += 1;
            let mut identity = self.ids.get(&pid).cloned();
            if self.reuse
                && self.reads >= 4
                && let Some(id) = &mut identity
            {
                id.start_time = Some(2);
            }
            Ok(identity)
        }
        fn platform_binary(&mut self, _: i32) -> Result<bool, &'static str> {
            self.platform
        }
        fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str> {
            self.events.push("signal");
            self.sent.push((pid, signal));
            Ok(())
        }
        fn wait(&mut self, d: Duration) {
            self.events.push("wait");
            self.waited.push(d);
        }
    }
    fn plan() -> KillPlan {
        KillPlan {
            pids: vec![PlannedPid {
                pid: 940001,
                expected: Some(Identity {
                    pid: 940001,
                    uid: 501,
                    start_time: Some(1),
                }),
            }],
            missing_targets: vec![],
        }
    }
    #[test]
    fn phase07_kill_json_measures_once_before_and_after_preserving_negative_host_delta() {
        use whoate_core::kill::MemoryMeasurement;
        let mut backend = Fake::new();
        backend.memory.extend([
            MemoryMeasurement {
                measured_at: Some("2026-10-17T00:00:00Z".into()),
                host_used_bytes: Some(100),
                target_resident_bytes: Some(4096),
                unknown_count: 0,
                reason: None,
            },
            MemoryMeasurement {
                measured_at: Some("2026-10-17T00:00:05Z".into()),
                host_used_bytes: Some(130),
                target_resident_bytes: Some(0),
                unknown_count: 0,
                reason: None,
            },
        ]);
        let mut out = vec![];
        let code = run_json(
            &plan(),
            Options {
                yes: true,
                dry_run: false,
                tty: false,
                language: Language::En,
            },
            &mut backend,
            &mut io::Cursor::new(b""),
            &mut out,
            &mut vec![],
        )
        .unwrap();
        assert_eq!(code, 0);
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["schema"], "whoate/v1");
        assert_eq!(json["executed"], true);
        assert_eq!(json["results"][0]["status"], "still_alive");
        assert_eq!(backend.events, vec!["measure", "signal", "wait", "measure"]);
        assert_eq!(json["memory"]["physical_memory_change_bytes"], -30);
        assert_eq!(json["memory"]["target_resident_change_bytes"], 4096);
        assert_eq!(backend.sent, vec![(940001, 15)]);
        assert_eq!(backend.waited, vec![Duration::from_secs(5)]);
        let mut backend = Fake::new();
        let mut out = vec![];
        run_json(
            &plan(),
            Options {
                yes: true,
                dry_run: true,
                tty: true,
                language: Language::En,
            },
            &mut backend,
            &mut io::Cursor::new(b"yes\n"),
            &mut out,
            &mut vec![],
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["executed"], false);
        assert_eq!(json["memory"]["after"], serde_json::Value::Null);
        assert_eq!(
            json["memory"]["physical_memory_change_bytes"],
            serde_json::Value::Null
        );
        assert_eq!(backend.events, vec!["measure"]);
        assert!(backend.sent.is_empty());
    }
    #[test]
    fn phase07_kill_preview_reports_missing_resident_not_footprint() {
        let mut fake = Fake::new();
        let mut out = vec![];
        run(
            &plan(),
            Options {
                yes: false,
                dry_run: true,
                tty: false,
                language: Language::En,
            },
            &mut fake,
            &mut io::Cursor::new(b""),
            &mut out,
            &mut vec![],
        )
        .unwrap();
        let text = String::from_utf8(out).unwrap();
        assert!(text.contains("Estimated physical reclaim"));
        assert!(
            text.lines()
                .any(|line| line.starts_with("Estimated physical reclaim: missing bytes;"))
        );
        assert!(text.contains("not guaranteed"));
        assert!(fake.sent.is_empty());
    }
    #[test]
    fn phase04_kill_confirm_preview_and_dry_run_never_signal() {
        for (yes, dry_run, tty) in [
            (false, false, true),
            (false, false, false),
            (true, true, true),
        ] {
            let mut fake = Fake::new();
            let mut out = vec![];
            let mut err = vec![];
            assert_eq!(
                run(
                    &plan(),
                    Options {
                        yes,
                        dry_run,
                        tty,
                        language: Language::En
                    },
                    &mut fake,
                    &mut &b"yes\n"[..],
                    &mut out,
                    &mut err
                )
                .unwrap(),
                0
            );
            assert!(fake.sent.is_empty());
            assert!(String::from_utf8(out).unwrap().contains("940001"));
        }
    }
    #[test]
    fn phase04_kill_stages_yes_only_terms_and_preserves_original_identity() {
        for (tty, answer, expected) in [
            (false, "yes\n", vec![(940001, 15)]),
            (true, "n\n", vec![(940001, 15)]),
            (true, "", vec![(940001, 15)]),
            (true, "yes\n", vec![(940001, 15), (940001, 9)]),
        ] {
            let mut fake = Fake::new();
            let mut out = vec![];
            let code = run(
                &plan(),
                Options {
                    yes: true,
                    dry_run: false,
                    tty,
                    language: Language::En,
                },
                &mut fake,
                &mut answer.as_bytes(),
                &mut out,
                &mut vec![],
            )
            .unwrap();
            assert_eq!(code, 0);
            assert_eq!(fake.sent, expected);
            assert_eq!(fake.waited, vec![Duration::from_secs(5)]);
            assert!(String::from_utf8(out).unwrap().contains("still_alive"));
        }
        let mut fake = Fake::new();
        fake.reuse = true;
        let code = run(
            &plan(),
            Options {
                yes: true,
                dry_run: false,
                tty: true,
                language: Language::En,
            },
            &mut fake,
            &mut &b"yes\n"[..],
            &mut vec![],
            &mut vec![],
        )
        .unwrap();
        assert_eq!(code, 4);
        assert_eq!(fake.sent, vec![(940001, 15)]);
    }
    #[test]
    fn phase04_kill_safety_dispatch_refuses_protected_and_failed_probes() {
        for mode in [
            "root",
            "self",
            "unknown_start",
            "apple",
            "probe",
            "reuse",
            "unknown_actual_start",
            "different_uid",
            "identity_probe",
        ] {
            let mut fake = Fake::new();
            let mut p = plan();
            match mode {
                "root" => p.pids[0].expected.as_mut().unwrap().uid = 0,
                "self" => p.pids[0].pid = 940099,
                "unknown_start" => p.pids[0].expected.as_mut().unwrap().start_time = None,
                "apple" => fake.platform = Ok(true),
                "probe" => fake.platform = Err("probe_failed"),
                "reuse" => p.pids[0].expected.as_mut().unwrap().start_time = Some(2),
                "unknown_actual_start" => fake.ids.get_mut(&940001).unwrap().start_time = None,
                "different_uid" => fake.ids.get_mut(&940001).unwrap().uid = 502,
                "identity_probe" => fake.identity_error = true,
                _ => unreachable!(),
            }
            let code = run(
                &p,
                Options {
                    yes: true,
                    dry_run: false,
                    tty: false,
                    language: Language::En,
                },
                &mut fake,
                &mut &b""[..],
                &mut vec![],
                &mut vec![],
            )
            .unwrap();
            assert_eq!(code, 4, "{mode}");
            assert!(fake.sent.is_empty(), "{mode}");
        }
    }

    #[test]
    fn phase04_exit_kill_mixed_refused_missing_and_still_alive_preserve_precedence() {
        for (root, missing, code) in [(true, true, 4), (false, true, 3), (false, false, 0)] {
            let mut p = plan();
            if root {
                p.pids.push(PlannedPid {
                    pid: 940002,
                    expected: Some(Identity {
                        pid: 940002,
                        uid: 0,
                        start_time: Some(2),
                    }),
                });
            }
            if missing {
                p.pids.push(PlannedPid {
                    pid: 940003,
                    expected: None,
                });
                p.missing_targets.push("service:absent".into());
            }
            let mut backend = Fake::new();
            let mut out = vec![];
            let actual = run(
                &p,
                Options {
                    yes: true,
                    dry_run: false,
                    tty: false,
                    language: Language::En,
                },
                &mut backend,
                &mut &b"yes\n"[..],
                &mut out,
                &mut vec![],
            )
            .unwrap();
            assert_eq!(actual, code);
            let text = String::from_utf8(out).unwrap();
            assert!(text.contains("940001 still_alive"));
            assert_eq!(text.contains("940002 refused"), root);
            assert_eq!(text.contains("940003 missing"), missing);
            assert_eq!(backend.sent, vec![(940001, 15)]);
            assert_eq!(backend.waited, vec![Duration::from_secs(5)]);
        }
    }
}
