use std::time::Duration;

pub struct Schedule {
    interval: Duration,
    next_sample: Duration,
    next_output: Duration,
}
impl Schedule {
    pub fn new(interval: Duration) -> Self {
        Self {
            interval,
            next_sample: Duration::ZERO,
            next_output: Duration::ZERO,
        }
    }
    pub fn due(&mut self, now: Duration) -> Option<bool> {
        if now < self.next_sample {
            return None;
        }
        self.next_sample = now.saturating_add(Duration::from_secs(1));
        let output = now >= self.next_output;
        if output {
            self.next_output = now.saturating_add(self.interval);
        }
        Some(output)
    }
    pub fn wait(&self, now: Duration) -> Duration {
        self.next_sample.saturating_sub(now)
    }
}

use std::{
    io::{self, Write},
    sync::atomic::{AtomicBool, Ordering},
    time::Instant,
};
use whoate_core::{
    engine::{Clock, Engine},
    insights::Insight,
    model::Report,
    sample::Sampler,
    snapshot::Snapshot,
    verdict::Language,
};
static STOP: AtomicBool = AtomicBool::new(false);
extern "C" fn stop(_: i32) {
    STOP.store(true, Ordering::Relaxed);
}
unsafe extern "C" {
    fn signal(number: i32, handler: usize) -> usize;
}
struct Signals {
    old: [usize; 2],
}
impl Signals {
    fn install() -> io::Result<Self> {
        STOP.store(false, Ordering::Relaxed);
        let mut old = [0; 2];
        for (i, number) in [2, 15].into_iter().enumerate() {
            old[i] = unsafe { signal(number, stop as *const () as usize) };
            if old[i] == usize::MAX {
                for (n, handler) in [2, 15].into_iter().zip(old).take(i) {
                    unsafe {
                        signal(n, handler);
                    }
                }
                return Err(io::Error::last_os_error());
            }
        }
        Ok(Self { old })
    }
}
impl Drop for Signals {
    fn drop(&mut self) {
        for (number, old) in [2, 15].into_iter().zip(self.old) {
            unsafe {
                signal(number, old);
            }
        }
    }
}

pub fn run<S: Sampler, C: Clock>(
    engine: &mut Engine<S, C>,
    args: &crate::cli::Cli,
    interval: Duration,
    json_lines: bool,
    language: Language,
) -> io::Result<i32> {
    let _signals = Signals::install()?;
    let start = Instant::now();
    let mut schedule = Schedule::new(interval);
    let mut out = io::stdout().lock();
    while !STOP.load(Ordering::Relaxed) {
        if let Some(emit) = schedule.due(start.elapsed()) {
            let report = match engine.tick() {
                Ok(report) => report,
                Err(error) => return crate::fatal(args, 1, &error.to_string()),
            };
            if STOP.load(Ordering::Relaxed) {
                break;
            }
            if emit {
                let snapshot = engine
                    .latest_snapshot()
                    .expect("successful tick has snapshot");
                write_frame(
                    &report,
                    snapshot,
                    args,
                    engine.catalog.all(),
                    json_lines,
                    language,
                    &mut out,
                )?;
            }
        }
        std::thread::sleep(schedule.wait(start.elapsed()));
    }
    Ok(0)
}

fn write_frame(
    report: &Report,
    snapshot: &Snapshot,
    args: &crate::cli::Cli,
    catalog: &[Insight],
    json_lines: bool,
    language: Language,
    out: &mut dyn Write,
) -> io::Result<()> {
    let output = crate::privacy::project(
        report,
        snapshot,
        crate::privacy::Policy {
            prompts: args.prompts,
            redact: args.redact,
        },
    );
    if json_lines {
        writeln!(
            out,
            "{}",
            serde_json::to_string(&output).map_err(io::Error::other)?
        )?;
    } else {
        out.write_all(crate::render::report(&output, catalog, language).as_bytes())?;
    }
    out.flush()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase04_watch_one_second_samples_two_second_output_without_backfill() {
        let mut schedule = Schedule::new(Duration::from_secs(2));
        assert_eq!(schedule.due(Duration::ZERO), Some(true));
        assert_eq!(schedule.due(Duration::from_millis(999)), None);
        assert_eq!(schedule.due(Duration::from_secs(1)), Some(false));
        assert_eq!(schedule.due(Duration::from_secs(2)), Some(true));
        assert_eq!(schedule.due(Duration::from_secs(5)), Some(true));
        assert_eq!(
            schedule.due(Duration::from_secs(5)),
            None,
            "do not synthesize missed ticks"
        );
        assert_eq!(
            schedule.wait(Duration::from_secs(5)),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn phase04_watch_real_engine_frames_keep_tmux_gaps_cpu_unknown_and_privacy() {
        use clap::Parser;
        use std::{collections::VecDeque, fs};
        use whoate_core::{
            engine::Context,
            recognizers::ConfigReader,
            sample::SampleResult,
            snapshot::{Proc, SampleIssue, TmuxPane},
        };
        struct Replay(VecDeque<Snapshot>);
        impl Sampler for Replay {
            fn sample(&mut self) -> SampleResult<Snapshot> {
                Ok(self.0.pop_front().unwrap())
            }
        }
        #[derive(Default)]
        struct Time(std::cell::Cell<u128>);
        impl Clock for Time {
            fn now_ns(&self) -> u128 {
                let now = self.0.get();
                self.0.set(now + 1_000_000_000);
                now
            }
            fn wait(&mut self, duration: Duration) {
                self.0.set(self.0.get() + duration.as_nanos());
            }
        }
        let home = crate::platform::Scratch::new().unwrap();
        fs::create_dir_all(home.0.join(".claude/sessions")).unwrap();
        fs::write(
            home.0.join(".claude/sessions/940099.json"),
            r#"{"sessionId":"watch-example","name":"Watch session","cwd":"/public/watch"}"#,
        )
        .unwrap();
        fs::create_dir_all(home.0.join(".claude/projects/-public-watch")).unwrap();
        fs::write(
            home.0
                .join(".claude/projects/-public-watch/watch-example.jsonl"),
            "{\"type\":\"user\",\"message\":{\"content\":\"WATCH_PRIVATE_SENTINEL\"}}\n",
        )
        .unwrap();
        let mut shell = Proc::identity(940010, 1, 501, Some(1), "zsh".into());
        shell.tmux_pane = Some("%42".into());
        shell.footprint_bytes = Some(10);
        shell.cpu_time_ns = Some(0);
        let mut runtime = Proc::identity(940001, 940010, 501, Some(2), "node".into());
        runtime.argv = vec!["node".into(), "vite".into()];
        runtime.footprint_bytes = Some(100);
        runtime.cpu_time_ns = Some(0);
        let mut agent = Proc::identity(940099, 1, 501, Some(3), "claude".into());
        agent.argv = vec!["claude".into()];
        agent.footprint_bytes = Some(50);
        agent.cpu_time_ns = Some(0);
        let mut first = Snapshot::historical(
            "2026-10-14T19:40:00+08:00".into(),
            vec![shell, runtime, agent],
        );
        first.tmux_panes = vec![TmuxPane {
            pane_id: "%42".into(),
            target: "work:0.0".into(),
            window_name: None,
            pane_pid: 940010,
            cwd: None,
        }];
        let mut second = first.clone();
        second.taken_at = "2026-10-14T19:40:01+08:00".into();
        let mut gap = second.clone();
        gap.taken_at = "2026-10-14T19:40:02+08:00".into();
        gap.tmux_panes.clear();
        gap.issues.push(SampleIssue {
            stage: "tmux".into(),
            pid: None,
            code: "nonzero_exit".into(),
        });
        gap.procs[1].footprint_bytes = None;
        let mut restored = second.clone();
        restored.taken_at = "2026-10-14T19:40:03+08:00".into();
        let mut child = Proc::identity(940002, 940001, 501, Some(4), "worker".into());
        child.footprint_bytes = Some(30);
        restored.procs.push(child);
        let mut engine = Engine::with_sampler(
            Replay(VecDeque::from([first, second, gap, restored])),
            Time::default(),
            Context {
                cfg: ConfigReader::isolated(home.0.clone()),
                uid: 501,
                language: Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        let args = crate::cli::Cli::parse_from(["whoate", "watch", "--json-lines"]);
        let mut stream = vec![];
        for _ in 0..3 {
            let report = engine.tick().unwrap();
            write_frame(
                &report,
                engine.latest_snapshot().unwrap(),
                &args,
                engine.catalog.all(),
                true,
                Language::En,
                &mut stream,
            )
            .unwrap();
        }
        let text = String::from_utf8(stream).unwrap();
        let frames: Vec<serde_json::Value> = text
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(frames.len(), 3);
        let select = |index: usize, id: &str| {
            frames[index]["owners"]
                .as_array()
                .unwrap()
                .iter()
                .find(|o| o["id"] == id)
                .unwrap()
        };
        assert_eq!(
            select(0, "pane:%42")["history"][0]["timestamp_s"],
            1791978001_i64
        );
        assert_eq!(select(0, "pane:%42")["cpu_pct"], 0.0);
        assert_eq!(
            select(1, "service:940010")["origin"]["pane_alive"],
            serde_json::Value::Null
        );
        assert!(
            select(1, "service:940010")["evidence"]
                .as_array()
                .unwrap()
                .iter()
                .any(|e| e["kind"] == "pane_status_unknown" && e["confidence"] == "weak")
        );
        assert_eq!(
            select(1, "service:940010")["history"],
            serde_json::json!([])
        );
        assert_eq!(select(2, "pane:%42")["cpu_pct"], serde_json::Value::Null);
        assert_eq!(
            select(2, "pane:%42")["history"],
            serde_json::json!([{"timestamp_s":1791978003_i64,"mem_bytes":140,"cpu_pct":null}])
        );
        assert!(!text.contains("WATCH_PRIVATE_SENTINEL"));
        for frame in &frames {
            let a = frame["owners"]
                .as_array()
                .unwrap()
                .iter()
                .find_map(|o| o["agent"].as_object())
                .unwrap();
            assert_eq!(a["name"], "Watch session");
            assert_eq!(a["first_prompt"], serde_json::Value::Null);
            assert_eq!(a["first_reply"], serde_json::Value::Null);
        }
        assert!(
            !select(1, "service:940010")["verdict"]["rule"]
                .as_str()
                .unwrap()
                .contains("growth")
        );
    }
}
