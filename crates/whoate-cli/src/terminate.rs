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
    let ids: std::collections::BTreeSet<_> = plan.pids.iter().map(|p| p.pid).collect();
    let preview = format!(
        "{}: {ids:?}; {}: {}\n",
        options.language.choose("进程PID", "PIDs"),
        options.language.choose("不存在的目标", "missing targets"),
        plan.missing_targets.len()
    );
    out.write_all(preview.as_bytes())?;
    // Flush preview before the first possible signal.
    out.flush()?;
    err.flush()?;
    if options.dry_run || !options.yes {
        writeln!(
            out,
            "{}",
            options.language.choose(
                "仅预览；--yes 只确认 SIGTERM，不会自动升级 SIGKILL。",
                "Preview only; --yes authorizes SIGTERM, never automatic SIGKILL."
            )
        )?;
        return Ok(
            if plan.pids.iter().any(|p| {
                p.pid <= 0
                    || p.pid == backend.self_pid()
                    || p.expected
                        .as_ref()
                        .is_some_and(|id| id.uid == 0 || id.start_time.is_none())
            }) {
                4
            } else if !plan.missing_targets.is_empty()
                || plan.pids.iter().any(|p| p.expected.is_none())
            {
                3
            } else {
                0
            },
        );
    }
    let mut result = kill::execute(plan, false, backend);
    write_results(&result, out)?;
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
            write_results(&result, out)?;
        }
    }
    Ok(result.exit_code())
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
            self.sent.push((pid, signal));
            Ok(())
        }
        fn wait(&mut self, d: Duration) {
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
