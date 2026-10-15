//! Shared pipeline for CLI and GUI. Only the sampler owns slow-capture workers.
use crate::{
    attribute,
    history::History,
    insights::{self, Catalog},
    kill,
    model::{OwnerKind, Report},
    recognizers::{self, ConfigReader, Recognizer},
    sample::{MacSampler, SampleError, SampleResult, Sampler},
    snapshot::{SampleIssue, Snapshot},
    sources::Sessions,
    swarm,
    verdict::{self, Language},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

pub trait Clock {
    fn now_ns(&self) -> u128;
    fn wait(&mut self, duration: Duration);
}
pub struct SystemClock(Instant);
impl Default for SystemClock {
    fn default() -> Self {
        Self(Instant::now())
    }
}
impl Clock for SystemClock {
    fn now_ns(&self) -> u128 {
        self.0.elapsed().as_nanos()
    }
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
pub struct Context {
    pub cfg: ConfigReader,
    pub uid: u32,
    pub language: Language,
    pub platform_probe: fn(i32) -> Result<bool, &'static str>,
}
pub struct Engine<S = MacSampler, C = SystemClock> {
    sampler: S,
    clock: C,
    context: Context,
    sessions: Sessions,
    history: History,
    recognizers: Vec<Box<dyn Recognizer>>,
    pub catalog: Catalog,
    previous: Option<(Snapshot, u128)>,
    latest: Option<Snapshot>,
}
impl Engine<MacSampler, SystemClock> {
    pub fn new() -> SampleResult<Self> {
        let cfg = ConfigReader::from_env().map_err(|e| SampleError::new("config", e))?;
        Ok(Self::with_sampler(
            MacSampler::new()?,
            SystemClock::default(),
            Context {
                cfg,
                uid: unsafe { libc::getuid() },
                language: Language::system(),
                platform_probe: kill::platform_binary,
            },
        ))
    }
}
impl<S: Sampler, C: Clock> Engine<S, C> {
    pub fn with_sampler(sampler: S, clock: C, mut context: Context) -> Self {
        context.cfg.language = context.language;
        Self {
            sampler,
            clock,
            context,
            sessions: Sessions::default(),
            history: History::default(),
            recognizers: recognizers::builtins(),
            catalog: Catalog::new(insights::all()),
            previous: None,
            latest: None,
        }
    }
    pub fn latest_snapshot(&self) -> Option<&Snapshot> {
        self.latest.as_ref()
    }
    pub fn history(&self) -> &History {
        &self.history
    }
    pub fn sample_once(&mut self) -> SampleResult<Report> {
        self.run(false)
    }
    pub fn tick(&mut self) -> SampleResult<Report> {
        self.run(true)
    }
    fn run(&mut self, continuous: bool) -> SampleResult<Report> {
        if !continuous || self.previous.is_none() {
            let first = self.sampler.sample()?;
            let at = self.clock.now_ns();
            self.previous = Some((first, at));
            self.clock.wait(Duration::from_millis(200));
        }
        let mut current = self.sampler.sample()?;
        let now = self.clock.now_ns();
        let mut seen = BTreeSet::new();
        let mut memory = 0_u64;
        for p in &current.procs {
            if !seen.insert(p.pid) {
                return Err(SampleError::new("snapshot", "duplicate_pid"));
            }
            memory = memory
                .checked_add(p.footprint_bytes.unwrap_or(0))
                .ok_or_else(|| SampleError::new("snapshot", "resource_overflow"))?;
        }
        let (previous, at) = self.previous.as_ref().expect("initial sample");
        let elapsed = now.checked_sub(*at).filter(|n| *n > 0);
        let old: BTreeMap<_, _> = previous.procs.iter().map(|p| (p.pid, p)).collect();
        for p in &mut current.procs {
            if p.uid == self.context.uid {
                p.cpu_pct_sampled = old
                    .get(&p.pid)
                    .filter(|old| p.same_identity(old))
                    .and_then(|old| p.cpu_time_ns.zip(old.cpu_time_ns))
                    .and_then(|(new, old)| new.checked_sub(old))
                    .zip(elapsed)
                    .map(|(cpu, elapsed)| cpu as f64 / elapsed as f64 * 100.0);
            }
        }
        self.previous = Some((current.clone(), now));
        let own_agents: Vec<_> = current
            .procs
            .iter()
            .filter(|p| p.uid == self.context.uid && crate::sources::tool(p).is_some())
            .cloned()
            .collect();
        let (agents, issues) = self.sessions.collect(&own_agents, &self.context.cfg);
        current.issues.extend(issues);
        let mut report = Report::from_snapshot(&current);
        report.owners =
            attribute::attribute(&current, self.context.uid, &self.context.cfg, &agents);
        for owner in &mut report.owners {
            for recognizer in &self.recognizers {
                let matched = match recognizers::apply(
                    recognizer.as_ref(),
                    owner,
                    &current,
                    &self.context.cfg,
                ) {
                    Ok(matched) => matched,
                    Err(code) => {
                        report.issues.push(SampleIssue {
                            stage: recognizer.id().into(),
                            pid: None,
                            code: code.into(),
                        });
                        false
                    }
                };
                if !matched {
                    continue;
                }
                if let Err(code) = self
                    .catalog
                    .attach(recognizer.id(), owner, &self.context.cfg)
                {
                    report.issues.push(SampleIssue {
                        stage: "insights".into(),
                        pid: None,
                        code: code.into(),
                    });
                }
            }
            if current
                .procs
                .iter()
                .any(|p| owner.pids.contains(&p.pid) && p.cpu_pct_sampled.is_none())
            {
                owner.cpu_pct = None;
            }
            if continuous {
                if let Some(at) = swarm::timestamp_us(&current) {
                    self.history
                        .record_snapshot_at(owner, &current, (at / 1_000_000) as i64);
                }
                owner.history = self.history.series(&owner.id);
            }
            let mut platform = false;
            if owner.kind == OwnerKind::Service {
                for pid in &owner.pids {
                    match (self.context.platform_probe)(*pid) {
                        Ok(true) => {
                            platform = true;
                            break;
                        }
                        Ok(false) => {}
                        Err(code) => report.issues.push(SampleIssue {
                            stage: "platform_signature".into(),
                            pid: Some(*pid),
                            code: code.into(),
                        }),
                    }
                }
            }
            owner.verdict = verdict::rules::owner(
                owner,
                &current,
                &self.history,
                continuous,
                platform,
                self.context.language,
            );
        }
        self.history
            .retain(&report.owners.iter().map(|o| o.id.clone()).collect());
        report.swarms = swarm::detect(&current, &report.owners);
        for group in &mut report.swarms {
            group.verdict = verdict::rules::swarm(group, self.context.language);
        }
        self.latest = Some(current);
        Ok(report)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use crate::{
        recognizers::ConfigReader,
        sample::{SampleResult, Sampler},
        snapshot::*,
        sources::testing::Temp,
        verdict::Language,
    };
    use std::{collections::VecDeque, time::Duration};
    #[test]
    fn phase02_engine_blocked_real_worker_keeps_public_pipeline_available() {
        use crate::sample::{
            SampleError,
            cache::{self, SlowSource, SystemCache, Worker},
        };
        use std::{
            sync::{
                Arc,
                atomic::{AtomicBool, AtomicUsize, Ordering},
                mpsc,
            },
            time::Instant,
        };
        struct Blocked {
            calls: Arc<AtomicUsize>,
            started: mpsc::Sender<()>,
            cancelled: mpsc::Sender<()>,
        }
        impl SlowSource for Blocked {
            fn capture(&mut self, stop: &AtomicBool) -> SampleResult<SystemCache> {
                self.calls.fetch_add(1, Ordering::AcqRel);
                self.started.send(()).unwrap();
                while !stop.load(Ordering::Acquire) {
                    std::thread::park_timeout(Duration::from_millis(1));
                }
                self.cancelled.send(()).unwrap();
                Err(SampleError::new("top", "cancelled"))
            }
        }
        struct Cached {
            worker: Worker,
            snapshot: Snapshot,
        }
        impl Sampler for Cached {
            fn sample(&mut self) -> SampleResult<Snapshot> {
                let state = self.worker.state();
                let mut s = self.snapshot.clone();
                state.merge(&mut s.procs, 100);
                s.system_procs = state.status(100);
                Ok(s)
            }
        }
        let t = Temp::new();
        let path = t.0.join("system.json");
        cache::write_atomic(
            &path,
            &SystemCache {
                as_of: 100,
                cpu_issue: None,
                rows: std::collections::BTreeMap::from([(
                    999001,
                    cache::CachedProc {
                        start_time: 1,
                        uid: 0,
                        footprint_bytes: 1024,
                        cpu_pct_sampled: Some(4.0),
                    },
                )]),
            },
        )
        .unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let (started_tx, started_rx) = mpsc::channel();
        let (cancelled_tx, cancelled_rx) = mpsc::channel();
        let worker = Worker::start(
            path,
            Blocked {
                calls: calls.clone(),
                started: started_tx,
                cancelled: cancelled_tx,
            },
        );
        started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        let s = Snapshot::historical(
            "2026-10-14T19:40:00+08:00".into(),
            vec![Proc::identity(
                999001,
                1,
                0,
                Some(1),
                "system-service".into(),
            )],
        );
        let mut engine = super::Engine::with_sampler(
            Cached {
                worker,
                snapshot: s,
            },
            Time::default(),
            context(t.0.clone()),
        );
        let begin = Instant::now();
        let report = engine.sample_once().unwrap();
        assert_eq!(report.system_procs.status, Freshness::Fresh);
        assert_eq!(report.owners[0].mem_bytes, 1024);
        for _ in 0..10 {
            engine.tick().unwrap();
        }
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert!(begin.elapsed() < Duration::from_secs(1));
        let before_drop = Instant::now();
        drop(engine);
        assert!(cancelled_rx.recv_timeout(Duration::from_secs(1)).is_ok());
        assert!(before_drop.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn phase02_engine_unmatched_software_does_not_receive_insights() {
        use crate::insights::{Catalog, Insight, Localized, Trigger};
        let t = Temp::new();
        let mut p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        p.footprint_bytes = Some(1024);
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut engine = super::Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap])),
            Time::default(),
            context(t.0.clone()),
        );
        engine.catalog = Catalog::new(vec![Insight {
            id: "runtime-only".into(),
            recognizer: "dev-runtimes".into(),
            trigger: Trigger::Memory { min_bytes: 1024 },
            cause: Localized {
                zh: "原因".into(),
                en: "Cause".into(),
            },
            advice: Localized {
                zh: "建议".into(),
                en: "Advice".into(),
            },
            source: "https://example.com/evidence".into(),
            checked: "2026-10-15".into(),
        }]);
        let report = engine.tick().unwrap();
        assert!(report.owners[0].insights.is_empty());
    }
    #[test]
    fn phase02_engine_language_context_reaches_fact_and_part_labels() {
        let t = Temp::new();
        t.write("project/package.json", r#"{"name":"locale-project"}"#);
        t.write(
            ".claude/sessions/999002.json",
            r#"{"sessionId":"locale-session","cwd":"/demo"}"#,
        );
        t.write(
            ".claude/projects/-demo/locale-session.jsonl",
            "{\"type\":\"user\",\"message\":{\"content\":\"Question\"}}\n",
        );
        let mut node = Proc::identity(999001, 1, 501, Some(1), "node".into());
        node.cwd = Some(t.0.join("project").to_string_lossy().into());
        node.footprint_bytes = Some(1);
        let mut agent = Proc::identity(999002, 1, 501, Some(1), "claude".into());
        agent.footprint_bytes = Some(1);
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![node, agent]);
        let mut ctx = context(t.0.clone());
        ctx.language = Language::Zh;
        let mut engine = super::Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap])),
            Time::default(),
            ctx,
        );
        let report = engine.sample_once().unwrap();
        assert_eq!(
            report
                .owners
                .iter()
                .flat_map(|o| &o.facts)
                .find(|f| f.key == "project")
                .unwrap()
                .label,
            "项目"
        );
        assert!(
            report
                .owners
                .iter()
                .flat_map(|o| &o.parts)
                .any(|p| p.name == "会话")
        );
    }
    #[test]
    fn phase02_engine_public_tick_connects_insights_and_config_facts() {
        use crate::{
            insights::{Catalog, Insight, Localized, Trigger},
            model::FactValue,
        };
        let t = Temp::new();
        t.write("project/package.json", r#"{"name":"pipeline-project"}"#);
        let mut p = Proc::identity(999001, 1, 501, Some(1), "node".into());
        p.cwd = Some(t.0.join("project").to_string_lossy().into());
        p.footprint_bytes = Some(1024);
        p.argv = vec!["node".into(), "vite".into()];
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut engine = super::Engine::with_sampler(
            Replay(VecDeque::from([snap.clone(), snap])),
            Time::default(),
            context(t.0.clone()),
        );
        engine.catalog = Catalog::new(vec![Insight {
            id: "pipeline-insight".into(),
            recognizer: "dev-runtimes".into(),
            trigger: Trigger::Memory { min_bytes: 1024 },
            cause: Localized {
                zh: "原因".into(),
                en: "Cause".into(),
            },
            advice: Localized {
                zh: "建议".into(),
                en: "Advice".into(),
            },
            source: "https://example.com/evidence".into(),
            checked: "2026-10-15".into(),
        }]);
        let report = engine.tick().unwrap();
        assert_eq!(report.owners[0].insights, vec!["pipeline-insight"]);
        assert_eq!(report.owners[0].parts[0].name, "vite");
        assert!(
            report.owners[0].facts.iter().any(
                |f| f.key == "project" && f.value == FactValue::Text("pipeline-project".into())
            )
        );
        assert_eq!(report.owners[0].history.len(), 1);
        assert_eq!(report.owners[0].verdict.rule, "default");
    }
    #[test]
    fn phase02_engine_zero_elapsed_preserves_stale_issues_and_drops_sampler() {
        use std::{cell::Cell, rc::Rc};
        struct Zero;
        impl super::Clock for Zero {
            fn now_ns(&self) -> u128 {
                0
            }
            fn wait(&mut self, d: Duration) {
                assert_eq!(d, Duration::from_millis(200));
            }
        }
        struct Observed {
            frames: VecDeque<Snapshot>,
            dropped: Rc<Cell<bool>>,
            calls: Rc<Cell<usize>>,
        }
        impl Sampler for Observed {
            fn sample(&mut self) -> SampleResult<Snapshot> {
                self.calls.set(self.calls.get() + 1);
                Ok(self.frames.pop_front().unwrap())
            }
        }
        impl Drop for Observed {
            fn drop(&mut self) {
                self.dropped.set(true);
            }
        }
        let t = Temp::new();
        let mut p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        p.footprint_bytes = Some(1);
        p.cpu_time_ns = Some(1);
        let mut snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        snap.system_procs = SystemProcs {
            status: Freshness::Stale,
            as_of: Some("2026-10-14T19:39:00+08:00".into()),
        };
        snap.issues.push(SampleIssue {
            stage: "top".into(),
            pid: None,
            code: "cached_only".into(),
        });
        let dropped = Rc::new(Cell::new(false));
        let calls = Rc::new(Cell::new(0));
        {
            let sampler = Observed {
                frames: VecDeque::from([snap.clone(), snap]),
                dropped: dropped.clone(),
                calls: calls.clone(),
            };
            let mut engine = super::Engine::with_sampler(sampler, Zero, context(t.0.clone()));
            let report = engine.sample_once().unwrap();
            assert_eq!(report.owners[0].cpu_pct, None);
            assert_eq!(report.system_procs.status, Freshness::Stale);
            assert_eq!(
                report.system_procs.as_of.as_deref(),
                Some("2026-10-14T19:39:00+08:00")
            );
            assert_eq!(report.issues[0].code, "cached_only");
            assert_eq!(calls.get(), 2);
        }
        assert!(dropped.get());
    }
    pub struct Replay(pub VecDeque<Snapshot>);
    impl Sampler for Replay {
        fn sample(&mut self) -> SampleResult<Snapshot> {
            self.0
                .pop_front()
                .ok_or_else(|| crate::sample::SampleError::new("replay", "exhausted"))
        }
    }
    #[derive(Default)]
    pub struct Time {
        pub now: u128,
        pub requested: Vec<Duration>,
    }
    impl super::Clock for Time {
        fn now_ns(&self) -> u128 {
            self.now
        }
        fn wait(&mut self, d: Duration) {
            self.requested.push(d);
            self.now += 500_000_000;
        }
    }
    pub fn context(home: std::path::PathBuf) -> super::Context {
        super::Context {
            cfg: ConfigReader::isolated(home),
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        }
    }
    #[test]
    fn phase02_engine_two_samples_real_elapsed_and_multicore_cpu() {
        let temp = Temp::new();
        let mut p = Proc::identity(999001, 1, 501, Some(1), "demo".into());
        p.cpu_time_ns = Some(1_000_000_000);
        p.footprint_bytes = Some(1024);
        p.cpu_pct_sampled = Some(99.0);
        let first = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p.clone()]);
        p.cpu_time_ns = Some(2_000_000_000);
        let second = Snapshot::historical(first.taken_at.clone(), vec![p]);
        let mut engine = super::Engine::with_sampler(
            Replay(VecDeque::from([first, second])),
            Time::default(),
            context(temp.0.clone()),
        );
        let report = engine.sample_once().unwrap();
        assert_eq!(report.owners[0].cpu_pct, Some(200.0));
        assert_eq!(engine.clock.requested, vec![Duration::from_millis(200)]);
        assert!(report.owners[0].history.is_empty());
        assert_eq!(report.owners[0].verdict.rule, "default");
    }
}
