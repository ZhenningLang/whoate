use super::{SampleError, SampleResult};
use crate::snapshot::{Proc, TmuxPane};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
};

pub fn is_candidate(proc: &Proc) -> bool {
    if proc.ppid != 1 || proc.uid == 0 {
        return false;
    }
    terminal_or_runtime(proc)
}

fn terminal_or_runtime(proc: &Proc) -> bool {
    let exe = proc
        .exe_path
        .as_deref()
        .unwrap_or(&proc.comm)
        .rsplit('/')
        .next()
        .unwrap_or("");
    proc.tmux_pane.is_some()
        || ["node", "bun", "deno", "ruby", "java"].contains(&exe)
        || exe.starts_with("python")
}

/// Budget filter, not attribution: a runtime descendant of a launchd-adopted
/// terminal tree may own its listener. Stop at live pane shells or app ancestors.
pub fn candidate_pids(procs: &[Proc], panes: &[TmuxPane], account_uid: u32) -> BTreeSet<i32> {
    let by_pid: BTreeMap<_, _> = procs.iter().map(|proc| (proc.pid, proc)).collect();
    let live_shells: BTreeSet<_> = panes.iter().map(|pane| pane.pane_pid).collect();
    let live_panes: BTreeSet<_> = panes.iter().map(|pane| pane.pane_id.as_str()).collect();
    procs
        .iter()
        .filter(|proc| {
            if proc.uid == 0
                || proc.uid != account_uid
                || !terminal_or_runtime(proc)
                || proc
                    .tmux_pane
                    .as_deref()
                    .is_some_and(|pane| live_panes.contains(pane))
            {
                return false;
            }
            if is_candidate(proc) {
                return true;
            }
            let mut parent = proc.ppid;
            let mut seen = BTreeSet::new();
            // ceiling: one bounded ancestor walk per candidate; replace with memoized
            // roots if candidate counts grow enough to threaten the 50ms fast budget.
            while parent > 1 && seen.len() < procs.len() {
                if !seen.insert(parent) || live_shells.contains(&parent) {
                    return false;
                }
                let Some(ancestor) = by_pid.get(&parent) else {
                    return false;
                };
                if ancestor
                    .exe_path
                    .as_deref()
                    .is_some_and(|path| path.contains(".app/Contents/"))
                {
                    return false;
                }
                parent = ancestor.ppid;
            }
            parent == 1
        })
        .map(|proc| proc.pid)
        .collect()
}

#[repr(C)]
#[derive(Clone, Copy)]
struct NativeSocket {
    socket_id: u64,
    port: u16,
    state: u16,
}

unsafe extern "C" {
    fn capture_sockets(pid: i32, out: *mut *mut NativeSocket, count: *mut usize) -> i32;
}

pub fn apply_socket_result(
    proc: &mut Proc,
    result: SampleResult<(Vec<u16>, u32)>,
) -> SampleResult<()> {
    match result {
        Ok((ports, established)) => {
            proc.listen_ports = Some(ports);
            proc.established_count = Some(established);
            Ok(())
        }
        Err(error) => {
            proc.listen_ports = None;
            proc.established_count = None;
            Err(error)
        }
    }
}

pub fn sockets(pid: i32) -> SampleResult<(Vec<u16>, u32)> {
    let mut pointer = std::ptr::null_mut();
    let mut count = 0;
    let code = unsafe { capture_sockets(pid, &mut pointer, &mut count) };
    if code != 0 {
        return Err(SampleError::os("sockets", code));
    }
    // C allocated count records using the SDK socket ABI; ownership is released here.
    let rows = unsafe { std::slice::from_raw_parts(pointer, count) };
    let mut seen = BTreeSet::new();
    let mut ports = BTreeSet::new();
    let mut established = 0;
    for row in rows {
        if !seen.insert(row.socket_id) {
            continue;
        }
        match row.state {
            1 => {
                ports.insert(row.port);
            }
            4 => established += 1,
            _ => {}
        }
    }
    unsafe { libc::free(pointer.cast::<c_void>()) };
    Ok((ports.into_iter().collect(), established))
}
