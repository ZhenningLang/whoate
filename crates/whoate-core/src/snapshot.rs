//! Replayable capture contract. Resource units are bytes and nanoseconds.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
    pub taken_at: String,
    /// None only when the source (e.g. an archive) did not capture host data.
    pub host: Option<Host>,
    pub procs: Vec<Proc>,
    pub tmux_panes: Vec<TmuxPane>,
    pub system_procs: SystemProcs,
    /// Safe codes, never command lines, environment values or filesystem paths.
    #[serde(default)]
    pub issues: Vec<SampleIssue>,
}

impl Snapshot {
    pub fn historical(taken_at: String, procs: Vec<Proc>) -> Self {
        Self {
            taken_at,
            host: None,
            procs,
            tmux_panes: vec![],
            system_procs: SystemProcs::missing(),
            issues: vec![],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub uid: u32,
    /// Unix microseconds. Archive identity has second precision, documented at source.
    pub start_time: Option<u64>,
    pub comm: String,
    pub exe_path: Option<String>,
    pub argv: Vec<String>,
    pub cwd: Option<String>,
    pub tmux_pane: Option<String>,
    pub tmux: Option<String>,
    pub footprint_bytes: Option<u64>,
    pub cpu_time_ns: Option<u64>,
    /// System interval CPU from top's second frame; None when unavailable.
    pub cpu_pct_sampled: Option<f64>,
    pub responsible_pid: Option<i32>,
    /// None = unqueried/unavailable; Some([])/Some(0) = queried and empty.
    pub listen_ports: Option<Vec<u16>>,
    pub established_count: Option<u32>,
    #[serde(default)]
    pub argv_lossy: bool,
}

impl Proc {
    pub fn identity(pid: i32, ppid: i32, uid: u32, start_time: Option<u64>, comm: String) -> Self {
        Self {
            pid,
            ppid,
            uid,
            start_time,
            comm,
            exe_path: None,
            argv: vec![],
            cwd: None,
            tmux_pane: None,
            tmux: None,
            footprint_bytes: None,
            cpu_time_ns: None,
            cpu_pct_sampled: None,
            responsible_pid: None,
            listen_ports: None,
            established_count: None,
            argv_lossy: false,
        }
    }
    pub fn same_identity(&self, other: &Self) -> bool {
        self.pid == other.pid
            && self.uid == other.uid
            && self.start_time.is_some()
            && self.start_time == other.start_time
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Host {
    pub mem_total_bytes: u64,
    /// Activity Monitor convention: anonymous minus purgeable + wired + compressor.
    pub mem_used_bytes: u64,
    pub compressed_bytes: u64,
    pub wired_bytes: u64,
    pub swap_used_bytes: u64,
    pub pressure: Pressure,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Pressure {
    Normal,
    Warn,
    Critical,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TmuxPane {
    pub pane_id: String,
    pub target: String,
    pub window_name: Option<String>,
    pub pane_pid: i32,
    pub cwd: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Freshness {
    Fresh,
    Stale,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemProcs {
    pub status: Freshness,
    pub as_of: Option<String>,
}

impl SystemProcs {
    pub fn missing() -> Self {
        Self {
            status: Freshness::Missing,
            as_of: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SampleIssue {
    pub stage: String,
    pub pid: Option<i32>,
    pub code: String,
}
