//! C ABI for the SwiftUI app. Engine, kill safety and JSON shapes come from
//! whoate-core; this layer serializes, remembers the latest live snapshot for
//! kill identities, and never lets a panic cross the boundary.
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    ffi::{CString, c_char},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Mutex,
    time::{Duration, Instant},
};
use whoate_core::{
    engine::{Clock, Engine},
    kill::{self, Backend, Identity, KillPlan, KillStatus, MacBackend, PidResult},
    model::{HistoryPoint, Report},
    sample::{SampleError, Sampler},
    snapshot::Snapshot,
};

/// Each owner's curve in a tick is capped; whoate_why returns the full series.
pub const TICK_HISTORY_POINTS: usize = 60;
/// After SIGKILL the kernel tears the process down asynchronously.
const KILL_SETTLE: Duration = Duration::from_secs(1);

pub struct WhoateEngine {
    source: Source,
    last: Option<Report>,
    #[cfg(debug_assertions)]
    scratch: Option<std::path::PathBuf>,
}
enum Source {
    Live(Box<dyn Tick>),
    #[cfg(debug_assertions)]
    Report(Box<Report>),
}
trait Tick {
    fn tick(&mut self) -> Result<(Report, Option<Snapshot>), SampleError>;
    fn set_active(&mut self, active: bool);
    #[cfg(test)]
    fn series(&self, id: &str) -> Vec<HistoryPoint>;
}
impl<S: Sampler, C: Clock> Tick for (Engine<S, C>, bool) {
    fn set_active(&mut self, active: bool) {
        self.0.set_active(active);
    }
    fn tick(&mut self) -> Result<(Report, Option<Snapshot>), SampleError> {
        let report = self.0.tick()?;
        // Only the native sampler's identities may authorize a later kill.
        let snapshot = self.1.then(|| self.0.latest_snapshot().cloned()).flatten();
        Ok((report, snapshot))
    }
    #[cfg(test)]
    fn series(&self, id: &str) -> Vec<HistoryPoint> {
        self.0.history().series(id)
    }
}
impl WhoateEngine {
    fn live<S: Sampler + 'static, C: Clock + 'static>(engine: Engine<S, C>, native: bool) -> Self {
        Self {
            source: Source::Live(Box::new((engine, native))),
            last: None,
            #[cfg(debug_assertions)]
            scratch: None,
        }
    }
    fn tick(&mut self) -> Result<&mut Report, SampleError> {
        let report = match &mut self.source {
            Source::Live(engine) => {
                let (report, snapshot) = engine.tick()?;
                if let Some(snapshot) = snapshot {
                    lock().snapshot = Some(snapshot);
                }
                report
            }
            #[cfg(debug_assertions)]
            Source::Report(report) => (**report).clone(),
        };
        Ok(self.last.insert(report))
    }
    #[cfg(test)]
    fn series(&self, id: &str) -> Vec<HistoryPoint> {
        match &self.source {
            Source::Live(engine) => engine.series(id),
            #[cfg(debug_assertions)]
            Source::Report(_) => vec![],
        }
    }
}
#[cfg(debug_assertions)]
impl Drop for WhoateEngine {
    fn drop(&mut self) {
        if let Some(dir) = &self.scratch {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

#[derive(Default)]
struct State {
    snapshot: Option<Snapshot>,
    /// Identities confirmed by the last force=0 request and still alive after it.
    pending: Option<(KillPlan, BTreeSet<i32>)>,
}
static STATE: Mutex<State> = Mutex::new(State {
    snapshot: None,
    pending: None,
});
fn lock() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(|e| e.into_inner())
}

fn raw(value: &Value) -> *mut c_char {
    // serde_json escapes NUL, so the conversion cannot fail on its output.
    CString::new(value.to_string()).map_or(std::ptr::null_mut(), CString::into_raw)
}
fn error(code: i32, message: &str) -> Value {
    json!({"schema": "whoate/v1", "error": {"code": code, "message": message}})
}
fn guarded(f: impl FnOnce() -> Value) -> *mut c_char {
    raw(&catch_unwind(AssertUnwindSafe(f)).unwrap_or_else(|_| error(1, "panic")))
}
fn host_json(sample: impl FnOnce() -> Result<whoate_core::snapshot::Host, SampleError>) -> Value {
    match sample()
        .and_then(|host| whoate_core::sample::sysctl::now_timestamp().map(|at| (host, at)))
    {
        Ok((host, at)) => json!({"schema":"whoate/host-v1", "taken_at":at, "host":host}),
        Err(e) => error(1, &e.to_string()),
    }
}

/// Independent lightweight projection; never constructs an Engine or sampler.
#[unsafe(no_mangle)]
pub extern "C" fn whoate_host_json() -> *mut c_char {
    guarded(|| host_json(whoate_core::sample::libproc::host))
}

/// Equal spacing over the series, always ending at the newest point.
pub fn downsample(points: &[HistoryPoint], max: usize) -> Vec<HistoryPoint> {
    if points.len() <= max || max < 2 {
        return points.iter().rev().take(max).rev().cloned().collect();
    }
    let last = points.len() - 1;
    (0..max)
        .map(|i| points[i * last / (max - 1)].clone())
        .collect()
}
fn tick_json(engine: &mut WhoateEngine) -> Value {
    let report = match engine.tick() {
        Ok(report) => report,
        Err(e) => return error(1, &e.to_string()),
    };
    let full: Vec<_> = report
        .owners
        .iter_mut()
        .map(|o| {
            let short = downsample(&o.history, TICK_HISTORY_POINTS);
            std::mem::replace(&mut o.history, short)
        })
        .collect();
    let value = serde_json::to_value(&*report).unwrap_or_else(|_| error(1, "serialize"));
    for (owner, history) in report.owners.iter_mut().zip(full) {
        owner.history = history;
    }
    value
}
fn why_json(engine: &mut WhoateEngine, pid: i32) -> Value {
    let report = match engine.last.as_mut() {
        Some(report) => report,
        None => match engine.tick() {
            Ok(report) => report,
            Err(e) => return error(1, &e.to_string()),
        },
    };
    // Serialize a filtered view without cloning every owner's full history.
    let owners = std::mem::take(&mut report.owners);
    let swarms = std::mem::take(&mut report.swarms);
    report.owners = owners
        .iter()
        .filter(|o| o.pids.contains(&pid))
        .cloned()
        .collect();
    report.swarms = swarms
        .iter()
        .filter(|s| {
            s.owners
                .iter()
                .any(|r| report.owners.iter().any(|o| o.id == r.id))
        })
        .cloned()
        .collect();
    let value = if report.owners.is_empty() {
        error(3, "pid_not_found")
    } else {
        serde_json::to_value(&*report).unwrap_or_else(|_| error(1, "serialize"))
    };
    report.owners = owners;
    report.swarms = swarms;
    value
}

/// Waits briefly for SIGKILL to take effect so the result reflects it.
struct Settle<'a>(&'a mut dyn Backend);
impl Backend for Settle<'_> {
    fn self_pid(&self) -> i32 {
        self.0.self_pid()
    }
    fn identity(&mut self, pid: i32) -> Result<Option<Identity>, &'static str> {
        self.0.identity(pid)
    }
    fn platform_binary(&mut self, pid: i32) -> Result<bool, &'static str> {
        self.0.platform_binary(pid)
    }
    fn signal(&mut self, pid: i32, signal: i32) -> Result<(), &'static str> {
        self.0.signal(pid, signal)?;
        let begin = Instant::now();
        // execute() re-reads the identity afterwards and decides the status.
        while signal == libc::SIGKILL
            && begin.elapsed() < KILL_SETTLE
            && matches!(self.0.identity(pid), Ok(Some(_)))
        {
            self.0.wait(Duration::from_millis(20));
        }
        Ok(())
    }
    fn wait(&mut self, duration: Duration) {
        self.0.wait(duration)
    }
    fn measure_memory(&mut self, plan: &KillPlan) -> kill::MemoryMeasurement {
        self.0.measure_memory(plan)
    }
}
fn kill_json(pids: &[i32], force: bool, backend: &mut dyn Backend) -> (i32, Value) {
    let mut unique = BTreeSet::new();
    let pids: Vec<_> = pids.iter().copied().filter(|p| unique.insert(*p)).collect();
    let (plan, mut refused) = if force {
        let state = lock();
        let (planned, still) = state
            .pending
            .as_ref()
            .map(|(plan, still)| (plan.pids.clone(), still.clone()))
            .unwrap_or_default();
        let (confirmed, refused): (Vec<_>, Vec<_>) = pids.iter().partition(|p| still.contains(p));
        let plan = KillPlan {
            pids: planned
                .into_iter()
                .filter(|p| confirmed.contains(&p.pid))
                .collect(),
            missing_targets: vec![],
        };
        let refused = refused
            .into_iter()
            .map(|pid| PidResult {
                pid,
                status: KillStatus::Refused,
                reason: Some("no_confirmed_plan".into()),
            })
            .collect();
        (plan, refused)
    } else {
        let state = lock();
        let empty = Snapshot::historical(String::new(), vec![]);
        let snapshot = state.snapshot.as_ref().unwrap_or(&empty);
        let targets: Vec<_> = pids.iter().map(i32::to_string).collect();
        // Numeric targets never read the report, so skip a full attribution under the lock.
        let plan = kill::plan(&targets, &Report::from_snapshot(&empty), snapshot);
        (plan, vec![])
    };
    // The lock is released: SIGTERM waits five seconds and must not stall ticks.
    let before = backend.measure_memory(&plan);
    let mut report = if force {
        kill::execute(&plan, true, &mut Settle(backend))
    } else {
        kill::execute(&plan, false, backend)
    };
    let after = backend.measure_memory(&plan);
    {
        let mut state = lock();
        let still: BTreeSet<_> = report
            .results
            .iter()
            .filter(|r| r.status == KillStatus::StillAlive)
            .map(|r| r.pid)
            .collect();
        if force {
            if let Some((_, pending)) = &mut state.pending {
                pending.retain(|p| !plan.pids.iter().any(|q| q.pid == *p) || still.contains(p));
            }
        } else {
            state.pending = Some((plan, still));
        }
    }
    report.results.append(&mut refused);
    report
        .results
        .sort_by_key(|r| pids.iter().position(|p| *p == r.pid));
    let memory = kill::MemoryChange::new(before, Some(after));
    (
        report.exit_code(),
        json!({"schema": "whoate/v1", "executed": true, "results": report.results,
               "missing_targets": report.missing_targets, "memory": memory}),
    )
}

/// Returns NULL when the native sampler, config or knowledge base cannot start.
#[unsafe(no_mangle)]
pub extern "C" fn whoate_engine_new() -> *mut WhoateEngine {
    catch_unwind(|| {
        Engine::new()
            .map(|engine| Box::into_raw(Box::new(WhoateEngine::live(engine, true))))
            .unwrap_or(std::ptr::null_mut())
    })
    .unwrap_or(std::ptr::null_mut())
}

/// whoate/v1 report JSON; on failure `{"schema","error":{code,message}}`.
/// # Safety
/// `engine` is NULL or a live pointer from `whoate_engine_new*`, used by one thread at a time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_engine_tick(engine: *mut WhoateEngine) -> *mut c_char {
    guarded(|| match unsafe { engine.as_mut() } {
        Some(engine) => tick_json(engine),
        None => error(2, "null_engine"),
    })
}

/// Suspend/resume owned workers without resetting report/history/kill state.
/// # Safety
/// `engine` is NULL or an exclusively used live engine pointer.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_engine_set_active(
    engine: *mut WhoateEngine,
    active: i32,
) -> *mut c_char {
    guarded(|| {
        if !matches!(active, 0 | 1) {
            return error(2, "invalid_active");
        }
        let Some(engine) = (unsafe { engine.as_mut() }) else {
            return error(2, "null_engine");
        };
        match &mut engine.source {
            Source::Live(engine) => engine.set_active(active != 0),
            #[cfg(debug_assertions)]
            Source::Report(_) => {}
        }
        json!({"schema":"whoate/control-v1","active":active != 0})
    })
}

/// The owner(s) containing `pid` with full history; code 3 when no owner has it.
/// # Safety
/// `engine` is NULL or a live pointer from `whoate_engine_new*`, used by one thread at a time.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_why(engine: *mut WhoateEngine, pid: i32) -> *mut c_char {
    guarded(|| match unsafe { engine.as_mut() } {
        Some(engine) => why_json(engine, pid),
        None => error(2, "null_engine"),
    })
}

/// Exit codes follow the CLI: 4 refused > 3 missing > 0; 2 invalid arguments.
/// force=0 sends SIGTERM to identities from the latest live tick and waits 5 s;
/// force=1 sends SIGKILL only to pids still alive after that confirmed request.
/// # Safety
/// `pids` is NULL or points to `n` readable values; `err` is NULL or writable.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_kill(
    pids: *const i32,
    n: i32,
    force: i32,
    err: *mut *mut c_char,
) -> i32 {
    let (code, value) = catch_unwind(AssertUnwindSafe(|| {
        if pids.is_null() || n <= 0 {
            return (2, error(2, "invalid_arguments"));
        }
        let pids = unsafe { std::slice::from_raw_parts(pids, n as usize) };
        kill_json(pids, force != 0, &mut MacBackend)
    }))
    .unwrap_or_else(|_| (1, error(1, "panic")));
    if let Some(err) = unsafe { err.as_mut() } {
        *err = raw(&value);
    }
    code
}

/// `{"schema":"whoate/v1","insights":[…]}`, same entries as `whoate insights --list`.
#[unsafe(no_mangle)]
pub extern "C" fn whoate_insights_json() -> *mut c_char {
    guarded(|| match whoate_core::insights::registry::load() {
        Ok(entries) => json!({"schema": "whoate/v1", "insights": entries}),
        Err(code) => error(1, code),
    })
}

/// # Safety
/// `s` is NULL or a string returned by this library and not freed yet.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_string_free(s: *mut c_char) {
    if !s.is_null() {
        drop(unsafe { CString::from_raw(s) });
    }
}

/// # Safety
/// `engine` is NULL or a live engine pointer that is not used afterwards.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_engine_free(engine: *mut WhoateEngine) {
    if !engine.is_null() {
        let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { Box::from_raw(engine) })));
    }
}

#[cfg(debug_assertions)]
mod replay;
#[cfg(debug_assertions)]
pub use replay::whoate_engine_new_from_fixture;

#[cfg(test)]
mod tests;
