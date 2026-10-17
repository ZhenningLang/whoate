//! Shared two-stage termination. Confirmation belongs to CLI/GUI, never here.
use crate::{model::Report, snapshot::Snapshot};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Identity {
    pub pid: i32,
    pub uid: u32,
    pub start_time: Option<u64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedPid {
    pub pid: i32,
    pub expected: Option<Identity>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KillPlan {
    pub pids: Vec<PlannedPid>,
    pub missing_targets: Vec<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KillStatus {
    Terminated,
    StillAlive,
    Refused,
    Missing,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PidResult {
    pub pid: i32,
    pub status: KillStatus,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KillReport {
    pub results: Vec<PidResult>,
    pub missing_targets: Vec<String>,
}
impl KillReport {
    /// Request status, not liveness: still_alive requires explicit confirmation.
    /// Refusal wins over missing in mixed outcomes (4 > 3 > 0).
    pub fn exit_code(&self) -> i32 {
        if self.results.iter().any(|r| r.status == KillStatus::Refused) {
            4
        } else if !self.missing_targets.is_empty()
            || self.results.iter().any(|r| r.status == KillStatus::Missing)
        {
            3
        } else {
            0
        }
    }
}
pub fn plan(targets: &[String], report: &Report, snapshot: &Snapshot) -> KillPlan {
    let mut pids = BTreeSet::new();
    let mut missing_targets = vec![];
    for target in targets {
        if let Ok(pid) = target.parse::<i32>() {
            pids.insert(pid);
        } else if let Some(owner) = report.owners.iter().find(|o| &o.id == target) {
            pids.extend(&owner.pids);
        } else {
            missing_targets.push(target.clone());
        }
    }
    let index: BTreeMap<_, _> = snapshot.procs.iter().map(|p| (p.pid, p)).collect();
    KillPlan {
        pids: pids
            .into_iter()
            .map(|pid| PlannedPid {
                pid,
                expected: index.get(&pid).map(|p| Identity {
                    pid: p.pid,
                    uid: p.uid,
                    start_time: p.start_time,
                }),
            })
            .collect(),
        missing_targets,
    }
}
pub trait Backend {
    fn self_pid(&self) -> i32;
    fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str>;
    fn platform_binary(&mut self, pid: i32) -> Result<bool, &'static str>;
    fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str>;
    fn wait(&mut self, duration: Duration);
    fn measure_memory(&mut self, _: &KillPlan) -> MemoryMeasurement {
        MemoryMeasurement::missing("measurement_unavailable")
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryMeasurement {
    pub measured_at: Option<String>,
    pub host_used_bytes: Option<u64>,
    pub target_resident_bytes: Option<u64>,
    pub unknown_count: usize,
    pub reason: Option<String>,
}
impl MemoryMeasurement {
    pub fn missing(reason: &str) -> Self {
        Self {
            measured_at: None,
            host_used_bytes: None,
            target_resident_bytes: None,
            unknown_count: 0,
            reason: Some(reason.into()),
        }
    }
}
pub trait MemoryReader {
    fn memory_identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str>;
    fn resident_bytes(&mut self, pid: i32) -> Result<u64, &'static str>;
    fn host_used_bytes(&mut self) -> Result<u64, &'static str>;
    fn measured_at(&mut self) -> Option<String>;
}
/// Same original identities before/read/after. A proven absent target contributes
/// zero after termination; PID reuse or unreadable identities remain unknown.
pub fn measure_memory(plan: &KillPlan, reader: &mut dyn MemoryReader) -> MemoryMeasurement {
    let mut total = Some(0_u64);
    let mut unknown_count = 0;
    let mut seen = BTreeSet::new();
    for item in &plan.pids {
        if !seen.insert(item.pid) {
            continue;
        }
        let bytes = (|| {
            let expected = item
                .expected
                .as_ref()
                .filter(|id| id.start_time.is_some())?;
            let before = reader.memory_identity(item.pid).ok()?;
            let Some(before) = before else {
                return Some(0);
            };
            if &before != expected {
                return None;
            }
            let bytes = reader.resident_bytes(item.pid).ok()?;
            let after = reader.memory_identity(item.pid).ok()??;
            (&after == expected).then_some(bytes)
        })();
        match bytes {
            Some(bytes) => total = total.and_then(|n| n.checked_add(bytes)),
            None => unknown_count += 1,
        }
    }
    let host = reader.host_used_bytes().ok();
    let target = (unknown_count == 0).then_some(total).flatten();
    MemoryMeasurement {
        measured_at: reader.measured_at(),
        host_used_bytes: host,
        target_resident_bytes: target,
        unknown_count,
        reason: if host.is_none() || target.is_none() {
            Some("memory_measurement_incomplete".into())
        } else {
            None
        },
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct MemoryChange {
    pub before: MemoryMeasurement,
    pub after: Option<MemoryMeasurement>,
    pub physical_memory_change_bytes: Option<i64>,
    pub target_resident_change_bytes: Option<i64>,
    pub basis: String,
}
impl MemoryChange {
    pub fn new(before: MemoryMeasurement, after: Option<MemoryMeasurement>) -> Self {
        let diff = |before: Option<u64>, after: Option<u64>| {
            before
                .zip(after)
                .and_then(|(b, a)| i64::try_from(b as i128 - a as i128).ok())
        };
        let physical_memory_change_bytes = diff(
            before.host_used_bytes,
            after.as_ref().and_then(|a| a.host_used_bytes),
        );
        let target_resident_change_bytes = diff(
            before.target_resident_bytes,
            after.as_ref().and_then(|a| a.target_resident_bytes),
        );
        Self { before, after, physical_memory_change_bytes, target_resident_change_bytes, basis: "before host used - after host used; non-causal observation; RSS estimate includes shared pages".into() }
    }
}
pub struct MacBackend;
impl Backend for MacBackend {
    fn measure_memory(&mut self, plan: &KillPlan) -> MemoryMeasurement {
        measure_memory(plan, self)
    }
    fn self_pid(&self) -> i32 {
        std::process::id() as i32
    }
    fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
        let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
        let expected = std::mem::size_of_val(&info) as i32;
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDTBSDINFO,
                0,
                (&mut info as *mut libc::proc_bsdinfo).cast(),
                expected,
            )
        };
        if bytes == expected {
            Ok(Some(Identity {
                pid,
                uid: info.pbi_uid,
                start_time: info
                    .pbi_start_tvsec
                    .checked_mul(1_000_000)
                    .and_then(|s| s.checked_add(info.pbi_start_tvusec)),
            }))
        } else if bytes == 0 && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
        {
            Ok(None)
        } else {
            Err("identity_probe_failed")
        }
    }
    fn platform_binary(&mut self, pid: i32) -> Result<bool, &'static str> {
        platform_binary(pid)
    }
    fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str> {
        if unsafe { libc::kill(pid, signal) } == 0 {
            Ok(())
        } else {
            Err("signal_failed")
        }
    }
    fn wait(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
impl MemoryReader for MacBackend {
    fn memory_identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
        self.identity(pid)
    }
    fn resident_bytes(&mut self, pid: i32) -> Result<u64, &'static str> {
        crate::sample::libproc::resources(pid)
            .map_err(|_| "resident_unavailable")?
            .2
            .ok_or("resident_unavailable")
    }
    fn host_used_bytes(&mut self) -> Result<u64, &'static str> {
        crate::sample::libproc::host()
            .map(|h| h.mem_used_bytes)
            .map_err(|_| "host_unavailable")
    }
    fn measured_at(&mut self) -> Option<String> {
        crate::sample::sysctl::now_timestamp().ok()
    }
}
pub fn platform_binary(pid: i32) -> Result<bool, &'static str> {
    type Csops = unsafe extern "C" fn(i32, u32, *mut std::ffi::c_void, usize) -> i32;
    // Apple xnu bsd/sys/codesign.h: CS_OPS_STATUS=0; osfmk/kern/cs_blobs.h:
    // CS_PLATFORM_BINARY=0x04000000. This checks kernel signing flags, not a path.
    let symbol = unsafe { libc::dlsym(libc::RTLD_DEFAULT, c"csops".as_ptr()) };
    if symbol.is_null() {
        return Err("platform_probe_unavailable");
    }
    let csops: Csops = unsafe { std::mem::transmute(symbol) };
    let mut flags = 0_u32;
    if unsafe {
        csops(
            pid,
            0,
            (&mut flags as *mut u32).cast(),
            std::mem::size_of_val(&flags),
        )
    } != 0
    {
        return Err("platform_probe_failed");
    }
    Ok(flags & 0x04000000 != 0)
}
fn checked(
    item: &PlannedPid,
    backend: &mut dyn Backend,
) -> Result<Identity, (KillStatus, &'static str)> {
    if item.pid <= 0 || item.pid == backend.self_pid() {
        return Err((KillStatus::Refused, "protected_pid"));
    }
    let Some(expected) = &item.expected else {
        return Err((KillStatus::Missing, "not_in_snapshot"));
    };
    if expected.uid == 0 || expected.start_time.is_none() {
        return Err((KillStatus::Refused, "protected_or_unknown_identity"));
    }
    let actual = backend
        .identity(item.pid)
        .map_err(|e| (KillStatus::Refused, e))?
        .ok_or((KillStatus::Missing, "process_missing"))?;
    if &actual != expected {
        return Err((KillStatus::Refused, "identity_changed"));
    }
    if backend
        .platform_binary(item.pid)
        .map_err(|e| (KillStatus::Refused, e))?
    {
        return Err((KillStatus::Refused, "apple_platform_binary"));
    }
    let again = backend
        .identity(item.pid)
        .map_err(|e| (KillStatus::Refused, e))?
        .ok_or((KillStatus::Missing, "process_missing"))?;
    if again != actual {
        return Err((KillStatus::Refused, "identity_changed"));
    }
    Ok(actual)
}
/// Read-only safety preview. Never signals/waits; still_alive is an observation.
pub fn preview(plan: &KillPlan, backend: &mut dyn Backend) -> KillReport {
    let mut seen = BTreeSet::new();
    let results = plan
        .pids
        .iter()
        .filter(|item| seen.insert(item.pid))
        .map(|item| {
            let (status, reason) = match checked(item, backend) {
                Ok(_) => (KillStatus::StillAlive, "preview_only"),
                Err(failure) => failure,
            };
            PidResult {
                pid: item.pid,
                status,
                reason: Some(reason.into()),
            }
        })
        .collect();
    KillReport {
        results,
        missing_targets: plan.missing_targets.clone(),
    }
}
pub fn execute(plan: &KillPlan, force: bool, backend: &mut dyn Backend) -> KillReport {
    let mut results = vec![];
    let mut sent = BTreeMap::new();
    let mut seen = BTreeSet::new();
    for item in &plan.pids {
        if !seen.insert(item.pid) {
            continue;
        }
        let result = checked(item, backend).and_then(|identity| {
            backend
                .signal(item.pid, if force { libc::SIGKILL } else { libc::SIGTERM })
                .map(|_| identity)
                .map_err(|e| (KillStatus::Refused, e))
        });
        match result {
            Ok(identity) => {
                sent.insert(item.pid, identity);
                results.push(PidResult {
                    pid: item.pid,
                    status: KillStatus::StillAlive,
                    reason: None,
                });
            }
            Err((status, reason)) => results.push(PidResult {
                pid: item.pid,
                status,
                reason: Some(reason.into()),
            }),
        }
    }
    if !sent.is_empty() && !force {
        backend.wait(Duration::from_secs(5));
    }
    for result in &mut results {
        let Some(identity) = sent.get(&result.pid) else {
            continue;
        };
        match backend.identity(result.pid) {
            Ok(None) => result.status = KillStatus::Terminated,
            Ok(Some(actual)) if &actual == identity => {}
            Ok(Some(_)) => {
                result.status = KillStatus::Refused;
                result.reason = Some("identity_changed".into());
            }
            Err(code) => {
                result.status = KillStatus::Refused;
                result.reason = Some(code.into());
            }
        }
    }
    KillReport {
        results,
        missing_targets: plan.missing_targets.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    struct Safety {
        mode: &'static str,
        reads: usize,
        sent: Vec<(i32, i32)>,
        waited: bool,
    }
    impl Backend for Safety {
        fn self_pid(&self) -> i32 {
            999099
        }
        fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
            self.reads += 1;
            if self.mode == "missing" || (self.mode == "gone" && self.waited) {
                return Ok(None);
            }
            Ok(Some(Identity {
                pid,
                uid: 501,
                start_time: Some(if self.mode == "reuse" && self.reads > 1 {
                    2
                } else {
                    1
                }),
            }))
        }
        fn platform_binary(&mut self, _: i32) -> Result<bool, &'static str> {
            if self.mode == "probe_error" {
                Err("probe_failed")
            } else {
                Ok(false)
            }
        }
        fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str> {
            self.sent.push((pid, signal));
            Ok(())
        }
        fn wait(&mut self, _: std::time::Duration) {
            self.waited = true;
        }
    }
    #[test]
    fn phase02_kill_probe_failure_reuse_missing_and_termination_negative_controls() {
        let plan = KillPlan {
            pids: vec![PlannedPid {
                pid: 999001,
                expected: Some(Identity {
                    pid: 999001,
                    uid: 501,
                    start_time: Some(1),
                }),
            }],
            missing_targets: vec![],
        };
        for (mode, status, code) in [
            ("probe_error", KillStatus::Refused, 4),
            ("reuse", KillStatus::Refused, 4),
            ("missing", KillStatus::Missing, 3),
            ("gone", KillStatus::Terminated, 0),
        ] {
            let mut backend = Safety {
                mode,
                reads: 0,
                sent: vec![],
                waited: false,
            };
            let result = execute(&plan, false, &mut backend);
            assert_eq!(result.results[0].status, status);
            assert_eq!(result.exit_code(), code);
            if mode != "gone" {
                assert!(backend.sent.is_empty());
            }
        }
        for pid in [-1, 0] {
            let mut backend = Safety {
                mode: "safe",
                reads: 0,
                sent: vec![],
                waited: false,
            };
            let plan = KillPlan {
                pids: vec![PlannedPid {
                    pid,
                    expected: None,
                }],
                missing_targets: vec![],
            };
            assert_eq!(
                execute(&plan, true, &mut backend).results[0].status,
                KillStatus::Refused
            );
            assert!(backend.sent.is_empty());
        }
    }
    #[test]
    fn phase02_kill_expand_deduplicates_and_mixed_refusal_wins() {
        let mut p = crate::snapshot::Proc::identity(999001, 1, 501, Some(1), "demo".into());
        p.footprint_bytes = Some(1);
        let snapshot = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![p]);
        let mut report = Report::from_snapshot(&snapshot);
        let mut owner = crate::model::Owner::new(
            "service:demo".into(),
            crate::model::OwnerKind::Service,
            "demo".into(),
        );
        owner.pids = vec![999001, 999002];
        report.owners.push(owner);
        let plan = plan(
            &[
                "service:demo".into(),
                "999001".into(),
                "missing-owner".into(),
            ],
            &report,
            &snapshot,
        );
        assert_eq!(
            plan.pids.iter().map(|p| p.pid).collect::<Vec<_>>(),
            vec![999001, 999002]
        );
        assert_eq!(plan.missing_targets, vec!["missing-owner"]);
        let r = KillReport {
            results: vec![
                PidResult {
                    pid: 1,
                    status: KillStatus::Missing,
                    reason: None,
                },
                PidResult {
                    pid: 2,
                    status: KillStatus::Refused,
                    reason: None,
                },
            ],
            missing_targets: vec![],
        };
        assert_eq!(r.exit_code(), 4);
    }
    struct Fake {
        identities: BTreeMap<i32, Identity>,
        apple: bool,
        sent: Vec<(i32, i32)>,
        waited: std::time::Duration,
    }
    impl Backend for Fake {
        fn self_pid(&self) -> i32 {
            999003
        }
        fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
            Ok(self.identities.get(&pid).cloned())
        }
        fn platform_binary(&mut self, _: i32) -> Result<bool, &'static str> {
            Ok(self.apple)
        }
        fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str> {
            self.sent.push((pid, signal));
            Ok(())
        }
        fn wait(&mut self, duration: std::time::Duration) {
            self.waited += duration;
        }
    }
    fn protection_case(pid: i32, uid: u32, apple: bool) -> (KillReport, Vec<(i32, i32)>) {
        let identity = Identity {
            pid,
            uid,
            start_time: Some(1),
        };
        let mut backend = Fake {
            identities: BTreeMap::from([(pid, identity.clone())]),
            apple,
            sent: vec![],
            waited: std::time::Duration::ZERO,
        };
        let result = execute(
            &KillPlan {
                pids: vec![PlannedPid {
                    pid,
                    expected: Some(identity),
                }],
                missing_targets: vec![],
            },
            false,
            &mut backend,
        );
        (result, backend.sent)
    }
    #[test]
    fn phase02_kill_uid_zero_is_refused_without_signal() {
        let (r, sent) = protection_case(999001, 0, false);
        assert_eq!(r.results[0].status, KillStatus::Refused);
        assert!(sent.is_empty());
        assert_eq!(r.exit_code(), 4);
    }
    #[test]
    fn phase02_kill_platform_binary_is_refused_without_signal() {
        let (r, sent) = protection_case(999002, 501, true);
        assert_eq!(r.results[0].status, KillStatus::Refused);
        assert!(sent.is_empty());
        assert_eq!(r.exit_code(), 4);
    }
    #[test]
    fn phase02_kill_self_is_refused_without_signal() {
        let (r, sent) = protection_case(999003, 501, false);
        assert_eq!(r.results[0].status, KillStatus::Refused);
        assert!(sent.is_empty());
        assert_eq!(r.exit_code(), 4);
    }
    #[test]
    fn phase02_kill_term_waits_five_seconds_and_force_is_explicit() {
        let id = Identity {
            pid: 999001,
            uid: 501,
            start_time: Some(1),
        };
        let mut backend = Fake {
            identities: BTreeMap::from([(id.pid, id.clone())]),
            apple: false,
            sent: vec![],
            waited: std::time::Duration::ZERO,
        };
        let plan = KillPlan {
            pids: vec![PlannedPid {
                pid: id.pid,
                expected: Some(id),
            }],
            missing_targets: vec![],
        };
        let result = execute(&plan, false, &mut backend);
        assert_eq!(backend.sent, vec![(999001, libc::SIGTERM)]);
        assert_eq!(backend.waited, std::time::Duration::from_secs(5));
        assert_eq!(result.results[0].status, KillStatus::StillAlive);
        assert_eq!(
            serde_json::to_value(result.exit_code()).unwrap(),
            serde_json::json!(0)
        );
        execute(&plan, true, &mut backend);
        assert_eq!(backend.sent[1], (999001, libc::SIGKILL));
    }
}
