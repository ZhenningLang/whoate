mod common;
use common::Temp;
use std::{
    fs,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};
use whoate_core::{
    engine::{Clock, Context, Engine},
    recognizers::{
        ConfigReader,
        docker::{ContainerStats, DockerSource},
    },
    sample::{SampleResult, Sampler},
    snapshot::{Proc, Snapshot},
    verdict::Language,
};

struct Replay(Snapshot);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.clone())
    }
}
struct Time;
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        1
    }
    fn wait(&mut self, _: Duration) {}
}
fn snapshot(home: &Temp) -> Snapshot {
    let path = home.0.join("Docker.app/Contents/Info.plist");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, "<?xml version=\"1.0\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>com.docker.docker</string></dict></plist>").unwrap();
    let mut app = Proc::identity(930001, 1, 501, Some(1), "com.docker.backend".into());
    app.exe_path = Some(
        home.0
            .join("Docker.app/Contents/MacOS/com.docker.backend")
            .to_string_lossy()
            .into(),
    );
    app.footprint_bytes = Some(100);
    let mut vm = Proc::identity(930002, 1, 0, Some(2), "VirtualMachine".into());
    vm.exe_path = Some("/System/Library/com.apple.Virtualization.VirtualMachine".into());
    vm.responsible_pid = Some(930001);
    vm.footprint_bytes = Some(50);
    Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![app, vm])
}
struct Blocked {
    started: mpsc::Sender<()>,
    cancelled: mpsc::Sender<()>,
}
impl DockerSource for Blocked {
    fn capture(&mut self, stop: &AtomicBool) -> Result<Vec<ContainerStats>, &'static str> {
        self.started.send(()).unwrap();
        while !stop.load(Ordering::Acquire) {
            std::thread::park_timeout(Duration::from_millis(1));
        }
        self.cancelled.send(()).unwrap();
        Err("cancelled")
    }
}
#[test]
fn phase03_docker_blocked_slow_source_never_blocks_tick_and_drop_cancels_own_worker() {
    let home = Temp::new();
    let (started_tx, started_rx) = mpsc::channel();
    let (cancelled_tx, cancelled_rx) = mpsc::channel();
    let cfg = ConfigReader::isolated(home.0.clone()).with_docker_source(Blocked {
        started: started_tx,
        cancelled: cancelled_tx,
    });
    let mut engine = Engine::with_sampler(
        Replay(snapshot(&home)),
        Time,
        Context {
            cfg,
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let begin = Instant::now();
    let report = engine.tick().unwrap();
    assert_eq!(report.owners[0].mem_bytes, 150);
    started_rx.recv_timeout(Duration::from_secs(1)).unwrap();
    for _ in 0..10 {
        engine.tick().unwrap();
    }
    assert!(begin.elapsed() < Duration::from_secs(1));
    let drop_started = Instant::now();
    drop(engine);
    assert!(
        drop_started.elapsed() < Duration::from_secs(1),
        "drop waited for the refresh interval"
    );
    assert!(cancelled_rx.recv_timeout(Duration::from_secs(1)).is_ok());
}

#[test]
fn phase03_docker_stats_parser_rejects_bad_rows_and_keeps_container_memory_separate() {
    let data = r#"{"ID":"abc123","Name":"test-container","MemUsage":"3.5MiB / 2GiB"}"#;
    let rows = whoate_core::recognizers::docker::parse_stats(data).unwrap();
    assert_eq!(rows[0].memory_bytes, 3670016);
    assert_eq!(rows[0].limit_bytes, 2147483648);
    for bad in [
        "not json",
        r#"{"ID":"abc","Name":"test","MemUsage":"NaNMiB / 1GiB"}"#,
        r#"{"ID":"abc","Name":"test","MemUsage":"1e999GiB / 1GiB"}"#,
        r#"{"ID":"abc","Name":"test","MemUsage":"1bad / 1GiB"}"#,
    ] {
        assert!(whoate_core::recognizers::docker::parse_stats(bad).is_err());
    }
}

struct StaticSource(Result<Vec<ContainerStats>, &'static str>);
impl DockerSource for StaticSource {
    fn capture(&mut self, _: &AtomicBool) -> Result<Vec<ContainerStats>, &'static str> {
        self.0.clone()
    }
}
#[test]
fn phase03_docker_ready_and_failed_stats_never_change_vm_ownership_or_resource_totals() {
    use whoate_core::model::FactValue;
    for failure in [
        None,
        Some("daemon_unavailable"),
        Some("timeout"),
        Some("invalid_stats"),
    ] {
        let home = Temp::new();
        let path = home
            .0
            .join("Library/Group Containers/group.com.docker/settings-store.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, r#"{"MemoryMiB":512,"UseResourceSaver":false}"#).unwrap();
        let result = failure.map_or_else(
            || {
                Ok(vec![ContainerStats {
                    id: "abc123".into(),
                    name: "test-container".into(),
                    memory_bytes: 3670016,
                    limit_bytes: 2147483648,
                }])
            },
            Err,
        );
        let cfg = ConfigReader::isolated(home.0.clone()).with_docker_source(StaticSource(result));
        let monitor = cfg.docker_monitor();
        let mut engine = Engine::with_sampler(
            Replay(snapshot(&home)),
            Time,
            Context {
                cfg,
                uid: 501,
                language: Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        engine.tick().unwrap();
        assert!(monitor.wait_for_capture(Duration::from_secs(1)));
        let r = engine.tick().unwrap();
        let o = &r.owners[0];
        assert_eq!(o.id, "app:com.docker.docker");
        assert_eq!(o.pids, vec![930001, 930002]);
        assert_eq!(o.mem_bytes, 150);
        assert_eq!(
            o.parts
                .iter()
                .find(|p| p.name == "Docker virtual machine")
                .unwrap()
                .pids,
            vec![930002]
        );
        assert_eq!(
            o.parts
                .iter()
                .find(|p| p.name == "Docker virtual machine")
                .unwrap()
                .mem_bytes,
            50
        );
        assert!(o.facts.iter().any(
            |f| f.key == "docker_memory_limit_bytes" && f.value == FactValue::Count(536870912)
        ));
        if let Some(code) = failure {
            assert!(
                o.facts.iter().any(|f| f.key == "docker_stats_status"
                    && f.value == FactValue::Text("missing".into()))
            );
            assert!(
                o.facts
                    .iter()
                    .any(|f| f.key == "docker_stats_issue"
                        && f.value == FactValue::Text(code.into()))
            );
            assert!(!o.facts.iter().any(|f| f.key == "docker_container_count"));
        } else {
            assert!(
                o.facts.iter().any(|f| f.key == "docker_stats_status"
                    && f.value == FactValue::Text("fresh".into()))
            );
            assert!(
                o.facts
                    .iter()
                    .any(|f| f.key == "docker_container_count" && f.value == FactValue::Count(1))
            );
            assert!(
                o.facts
                    .iter()
                    .any(|f| f.key == "docker_container.abc123.memory_bytes"
                        && f.value == FactValue::Count(3670016))
            );
        }
        drop(monitor);
        drop(engine);
    }
}

#[test]
fn phase03_docker_valid_defaults_are_distinct_from_missing_invalid_and_deleted_settings() {
    use whoate_core::model::FactValue;
    for (contents, memory_default, saver) in [
        (Some("{}"), Some(true), Some(true)),
        (
            Some(r#"{"MemoryMiB":256,"UseResourceSaver":false}"#),
            Some(false),
            Some(false),
        ),
        (
            Some(r#"{"MemoryMiB":0,"UseResourceSaver":"bad"}"#),
            None,
            None,
        ),
        (Some("[]"), None, None),
        (Some("broken-SECRET_DOCKER_SENTINEL"), None, None),
        (None, None, None),
    ] {
        let home = Temp::new();
        let snap = snapshot(&home);
        let path = home
            .0
            .join("Library/Group Containers/group.com.docker/settings-store.json");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        if let Some(contents) = contents {
            fs::write(&path, contents).unwrap();
        }
        let cfg = ConfigReader::isolated(home.0.clone())
            .with_docker_source(StaticSource(Err("SECRET_DOCKER_SENTINEL")));
        let mut engine = Engine::with_sampler(
            Replay(snap),
            Time,
            Context {
                cfg,
                uid: 501,
                language: Language::En,
                platform_probe: |_| Ok(false),
            },
        );
        let r = engine.tick().unwrap();
        let o = &r.owners[0];
        assert_eq!(
            o.facts
                .iter()
                .find(|f| f.key == "docker_memory_default")
                .map(|f| f.value.clone()),
            memory_default.filter(|v| *v).map(FactValue::Bool)
        );
        assert_eq!(
            o.facts
                .iter()
                .find(|f| f.key == "docker_resource_saver")
                .map(|f| f.value.clone()),
            saver.map(FactValue::Bool)
        );
        assert_eq!(
            o.insights.contains(&"docker.default-memory".into()),
            memory_default == Some(true)
        );
        assert_eq!(
            o.insights.contains(&"docker.resource-saver".into()),
            saver == Some(false)
        );
        assert_eq!(o.mem_bytes, 150);
        assert!(
            !serde_json::to_string(&r)
                .unwrap()
                .contains("SECRET_DOCKER_SENTINEL")
        );
        if contents.is_some() {
            fs::remove_file(&path).unwrap();
            let r = engine.tick().unwrap();
            assert!(r.owners[0].insights.is_empty());
        }
    }
}
