//! Container stats are independent Linux facts, never extra macOS footprint.
use super::{
    ConfigReader, Recognizer,
    dev_runtimes::{fact, parts},
};
use crate::{
    model::{FactValue, Owner},
    sample::{
        cache::{FRESH_SECONDS, INTERVAL_SECONDS},
        command::read_command,
        top::MAX_PROCESSES,
    },
    snapshot::{Proc, Snapshot},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    os::unix::fs::FileTypeExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

#[derive(Clone, Debug, PartialEq)]
pub struct ContainerStats {
    pub id: String,
    pub name: String,
    pub memory_bytes: u64,
    pub limit_bytes: u64,
}
pub trait DockerSource: Send + 'static {
    /// Implementations must honor cancellation and return only safe error codes.
    fn capture(&mut self, stop: &AtomicBool) -> Result<Vec<ContainerStats>, &'static str>;
}
fn memory_bytes(value: &str) -> Option<u64> {
    let value = value.trim();
    let split = value
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(value.len());
    let number = value[..split].parse::<f64>().ok()?;
    let unit = match value[split..].trim() {
        "B" => 1_f64,
        "KiB" => 1024_f64,
        "MiB" => 1024_f64.powi(2),
        "GiB" => 1024_f64.powi(3),
        "TiB" => 1024_f64.powi(4),
        "kB" => 1000_f64,
        "MB" => 1_000_000_f64,
        "GB" => 1_000_000_000_f64,
        _ => return None,
    };
    let bytes = number * unit;
    (bytes.is_finite() && bytes >= 0.0 && bytes < u64::MAX as f64).then_some(bytes as u64)
}
pub fn parse_stats(text: &str) -> Result<Vec<ContainerStats>, &'static str> {
    #[derive(Deserialize)]
    struct Row {
        #[serde(rename = "ID")]
        id: String,
        #[serde(rename = "Name")]
        name: String,
        #[serde(rename = "MemUsage")]
        memory: String,
    }
    let mut rows = vec![];
    let mut ids = BTreeSet::new();
    for line in text.lines().filter(|s| !s.trim().is_empty()) {
        let row: Row = serde_json::from_str(line).map_err(|_| "invalid_stats")?;
        if row.id.is_empty()
            || !row.id.bytes().all(|b| b.is_ascii_hexdigit())
            || !ids.insert(row.id.clone())
            || row.name.is_empty()
            || !row
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'.'))
        {
            return Err("invalid_stats");
        }
        let (used, limit) = row.memory.split_once('/').ok_or("invalid_stats")?;
        rows.push(ContainerStats {
            id: row.id,
            name: row.name,
            memory_bytes: memory_bytes(used).ok_or("invalid_stats")?,
            limit_bytes: memory_bytes(limit).ok_or("invalid_stats")?,
        });
    }
    Ok(rows)
}
struct NativeDocker {
    home: PathBuf,
}
impl DockerSource for NativeDocker {
    fn capture(&mut self, stop: &AtomicBool) -> Result<Vec<ContainerStats>, &'static str> {
        if stop.load(Ordering::Acquire) {
            return Err("cancelled");
        }
        let socket = self.home.join(".docker/run/docker.sock");
        let local = socket
            .canonicalize()
            .ok()
            .zip(self.home.canonicalize().ok())
            .is_some_and(|(p, h)| p.starts_with(h));
        if !local || !std::fs::metadata(&socket).is_ok_and(|m| m.file_type().is_socket()) {
            return Err("daemon_unavailable");
        }
        let endpoint = format!("unix://{}", socket.display());
        let output = read_command(
            "/usr/bin/env",
            &[
                "-u",
                "DOCKER_CONTEXT",
                "-u",
                "DOCKER_HOST",
                "-u",
                "DOCKER_TLS_VERIFY",
                "-u",
                "DOCKER_CERT_PATH",
                "/Applications/Docker.app/Contents/Resources/bin/docker",
                "--host",
                &endpoint,
                "stats",
                "--no-stream",
                "--format",
                "{{json .}}",
            ],
            stop,
            Duration::from_secs(FRESH_SECONDS as u64),
            2 * (MAX_PROCESSES + 1) * 256,
        )
        .map_err(|e| match e.code.as_str() {
            "cancelled" => "cancelled",
            "timeout" => "timeout",
            "output_limit" => "output_limit",
            _ => "docker_command_failed",
        })?;
        if !output.success {
            return Err("daemon_unavailable");
        }
        parse_stats(std::str::from_utf8(&output.stdout).map_err(|_| "invalid_stats")?)
    }
}
#[derive(Default)]
struct State {
    rows: Vec<ContainerStats>,
    captured: Option<Instant>,
    as_of: Option<String>,
    error: Option<&'static str>,
    generation: u64,
}
#[derive(Default)]
struct Shared {
    state: Mutex<State>,
    updated: Condvar,
}
struct Worker {
    stop: Arc<AtomicBool>,
    join: JoinHandle<()>,
}
pub struct Monitor {
    shared: Arc<Shared>,
    source: Mutex<Option<Box<dyn DockerSource>>>,
    worker: Mutex<Option<Worker>>,
}
impl std::fmt::Debug for Monitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DockerMonitor").finish_non_exhaustive()
    }
}
impl Monitor {
    pub fn new(source: impl DockerSource) -> Self {
        Self {
            shared: Arc::default(),
            source: Mutex::new(Some(Box::new(source))),
            worker: Mutex::new(None),
        }
    }
    pub(crate) fn native(home: PathBuf) -> Self {
        Self::new(NativeDocker { home })
    }
    fn request(&self) {
        let mut worker = self.worker.lock().expect("docker worker lock");
        if worker.is_some() {
            return;
        }
        let Some(mut source) = self.source.lock().expect("docker source lock").take() else {
            return;
        };
        let shared = self.shared.clone();
        let stop = Arc::new(AtomicBool::new(false));
        let cancel = stop.clone();
        let join = thread::spawn(move || {
            while !cancel.load(Ordering::Acquire) {
                let started = Instant::now();
                let as_of = time::OffsetDateTime::now_utc()
                    .format(&time::format_description::well_known::Rfc3339)
                    .expect("timestamp format");
                let result = source.capture(&cancel);
                {
                    let mut state = shared.state.lock().expect("docker state lock");
                    state.generation += 1;
                    match result {
                        Ok(rows) => {
                            state.rows = rows;
                            state.captured = Some(started);
                            state.as_of = Some(as_of);
                            state.error = None;
                        }
                        Err(code) => {
                            state.error = Some(match code {
                                "cancelled"
                                | "timeout"
                                | "daemon_unavailable"
                                | "invalid_stats"
                                | "output_limit"
                                | "docker_command_failed" => code,
                                _ => "docker_stats_failed",
                            })
                        }
                    }
                    shared.updated.notify_all();
                }
                // The source may consume unpark while honoring cancellation.
                // Never enter the refresh wait after it returns cancelled.
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                thread::park_timeout(
                    Duration::from_secs(INTERVAL_SECONDS as u64).saturating_sub(started.elapsed()),
                );
            }
        });
        *worker = Some(Worker { stop, join });
    }
    /// Explicit waiting seam for tests; the Engine and recognizers never call it.
    pub fn wait_for_capture(&self, timeout: Duration) -> bool {
        let state = self.shared.state.lock().expect("docker state lock");
        let (state, _) = self
            .shared
            .updated
            .wait_timeout_while(state, timeout, |s| s.generation == 0)
            .expect("docker state lock");
        state.generation > 0
    }
}
impl Drop for Monitor {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.get_mut().expect("docker worker lock").take() {
            worker.stop.store(true, Ordering::Release);
            worker.join.thread().unpark();
            // Join owns cancellation of the read_command child, not any daemon.
            let Worker { stop, join } = worker;
            drop(stop);
            let _ = join.join();
        }
    }
}

pub struct Docker;
pub fn is_docker(proc: &Proc) -> bool {
    proc.exe_path
        .as_deref()
        .is_some_and(|s| s.contains("/Docker.app/Contents/"))
}
pub(crate) fn configure(owner: &mut Owner, snap: &Snapshot, cfg: &mut ConfigReader) {
    for key in ["docker.memory_default", "docker.resource_saver_disabled"] {
        cfg.values.insert(key.into(), serde_json::Value::Null);
    }
    if !snap
        .procs
        .iter()
        .any(|p| owner.pids.contains(&p.pid) && is_docker(p))
    {
        return;
    }
    cfg.docker_monitor().request();
    {
        let monitor = cfg.docker_monitor();
        let state = monitor.shared.state.lock().expect("docker state lock");
        let status = match state.captured {
            None => "missing",
            Some(at)
                if state.error.is_none()
                    && at.elapsed() <= Duration::from_secs(FRESH_SECONDS as u64) =>
            {
                "fresh"
            }
            _ => "stale",
        };
        fact(
            owner,
            "docker_stats_status",
            cfg.language.choose(
                "容器统计状态（独立口径）",
                "Container stats status (separate accounting)",
            ),
            FactValue::Text(status.into()),
        );
        if let Some(code) = state.error {
            fact(
                owner,
                "docker_stats_issue",
                cfg.language
                    .choose("容器统计不可用", "Container stats unavailable"),
                FactValue::Text(code.into()),
            );
        }
        if let Some(as_of) = &state.as_of {
            fact(
                owner,
                "docker_stats_as_of",
                cfg.language
                    .choose("容器统计采样时间", "Container stats sampled at"),
                FactValue::Text(as_of.clone()),
            );
        }
        if state.captured.is_some() {
            fact(
                owner,
                "docker_container_count",
                cfg.language.choose("容器数", "Container count"),
                FactValue::Count(state.rows.len() as u64),
            );
            for row in &state.rows {
                fact(
                    owner,
                    &format!("docker_container.{}.memory_bytes", row.id),
                    &row.name,
                    FactValue::Count(row.memory_bytes),
                );
            }
        }
    }
    let config = cfg.read_json(
        &cfg.home
            .join("Library/Group Containers/group.com.docker/settings-store.json"),
    );
    match config {
        Ok(v) if v.is_object() => {
            match v.get("MemoryMiB") {
                None => {
                    cfg.values
                        .insert("docker.memory_default".into(), true.into());
                    fact(
                        owner,
                        "docker_memory_default",
                        cfg.language.choose(
                            "VM 默认上限为本机内存的一半",
                            "VM default limit is half of host memory",
                        ),
                        FactValue::Bool(true),
                    );
                    if let Some(host) = &snap.host {
                        fact(
                            owner,
                            "docker_memory_limit_bytes",
                            cfg.language.choose(
                                "配置内存上限（非实际占用）",
                                "Configured memory limit (not usage)",
                            ),
                            FactValue::Count(host.mem_total_bytes / 2),
                        );
                    }
                }
                Some(value) => match value
                    .as_u64()
                    .filter(|n| *n > 0)
                    .and_then(|n| n.checked_mul(1024 * 1024))
                {
                    Some(bytes) => {
                        cfg.values
                            .insert("docker.memory_default".into(), false.into());
                        fact(
                            owner,
                            "docker_memory_limit_bytes",
                            cfg.language.choose(
                                "配置内存上限（非实际占用）",
                                "Configured memory limit (not usage)",
                            ),
                            FactValue::Count(bytes),
                        );
                    }
                    None => fact(
                        owner,
                        "config_issue.docker",
                        cfg.language.choose("配置无效", "Invalid configuration"),
                        FactValue::Text("invalid_memory_mib".into()),
                    ),
                },
            }
            let saver = match v.get("UseResourceSaver") {
                None => Some(true),
                Some(value) => value.as_bool(),
            };
            if let Some(saver) = saver {
                cfg.values
                    .insert("docker.resource_saver_disabled".into(), (!saver).into());
                fact(
                    owner,
                    "docker_resource_saver",
                    cfg.language
                        .choose("Resource Saver 用户设置", "Resource Saver user setting"),
                    FactValue::Bool(saver),
                );
            } else {
                fact(
                    owner,
                    "config_issue.docker",
                    cfg.language.choose("配置无效", "Invalid configuration"),
                    FactValue::Text("invalid_resource_saver".into()),
                );
            }
        }
        result => fact(
            owner,
            "config_issue.docker",
            cfg.language
                .choose("配置不可读", "Unreadable configuration"),
            FactValue::Text(result.err().unwrap_or("invalid_docker_config").into()),
        ),
    }
}
impl Recognizer for Docker {
    fn id(&self) -> &'static str {
        "docker"
    }
    fn matches(&self, proc: &Proc) -> bool {
        is_docker(proc)
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        let mut groups: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        let mcp = super::coding_agents::mcp_members(owner, snap);
        for p in &snap.procs {
            if !owner.pids.contains(&p.pid) || mcp.contains_key(&p.pid) {
                continue;
            }
            let vm_name = p
                .exe_path
                .as_deref()
                .and_then(|s| Path::new(s).file_name())
                .and_then(|s| s.to_str());
            let vm = matches!(
                vm_name,
                Some("com.docker.virtualization" | "com.docker.krun")
            ) || (vm_name == Some("com.apple.Virtualization.VirtualMachine")
                && p.responsible_pid.is_some_and(|pid| {
                    snap.procs
                        .iter()
                        .any(|q| q.pid == pid && owner.pids.contains(&pid) && is_docker(q))
                }));
            let label = if vm {
                cfg.language
                    .choose("Docker 虚拟机", "Docker virtual machine")
            } else if is_docker(p) {
                cfg.language.choose("Docker 服务", "Docker services")
            } else {
                continue;
            };
            groups.entry(label.into()).or_default().push(p);
        }
        parts(owner, groups);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        model::{FactValue, OwnerKind},
        sources::testing::Temp,
    };
    struct Empty;
    impl DockerSource for Empty {
        fn capture(&mut self, _: &AtomicBool) -> Result<Vec<ContainerStats>, &'static str> {
            Ok(vec![])
        }
    }
    #[test]
    fn phase03_docker_expired_or_failed_stats_keep_original_as_of_and_separate_resources() {
        let home = Temp::new();
        let mut cfg = ConfigReader::isolated(home.0.clone()).with_docker_source(Empty);
        let mut p = Proc::identity(950001, 1, 501, Some(1), "com.docker.backend".into());
        p.exe_path = Some("/Applications/Docker.app/Contents/MacOS/com.docker.backend".into());
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut owner = Owner::new("docker".into(), OwnerKind::App, "Docker".into());
        owner.pids = vec![950001];
        owner.mem_bytes = 123;
        configure(&mut owner, &snap, &mut cfg);
        let monitor = cfg.docker_monitor();
        assert!(monitor.wait_for_capture(Duration::from_secs(1)));
        for (age, error, expected) in [
            (0, None, "fresh"),
            (31, None, "stale"),
            (0, Some("timeout"), "stale"),
        ] {
            {
                let mut state = monitor.shared.state.lock().unwrap();
                state.captured = Some(Instant::now() - Duration::from_secs(age));
                state.as_of = Some("2000-01-01T00:00:00Z".into());
                state.error = error;
                state.rows = vec![ContainerStats {
                    id: "abc123".into(),
                    name: "test".into(),
                    memory_bytes: 999,
                    limit_bytes: 2000,
                }];
            }
            owner.facts.clear();
            configure(&mut owner, &snap, &mut cfg);
            assert_eq!(
                owner
                    .facts
                    .iter()
                    .find(|f| f.key == "docker_stats_status")
                    .map(|f| f.value.clone()),
                Some(FactValue::Text(expected.into()))
            );
            assert_eq!(
                owner
                    .facts
                    .iter()
                    .find(|f| f.key == "docker_stats_as_of")
                    .map(|f| f.value.clone()),
                Some(FactValue::Text("2000-01-01T00:00:00Z".into()))
            );
            assert_eq!(owner.mem_bytes, 123);
            assert!(owner.parts.is_empty());
        }
    }
}
