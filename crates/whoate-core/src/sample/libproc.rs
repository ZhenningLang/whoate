//! SDK-backed sampling. No Rust layout assumptions for kinfo_proc/socket unions.
use super::{
    SampleError, SampleResult, Sampler,
    cache::{self, CachedProc, SlowSource, SystemCache, Worker},
    host_cpu::CpuMeter,
    ports, sysctl, tmux, top,
};
use crate::snapshot::{Proc, SampleIssue, Snapshot};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    mem::{size_of, zeroed},
    path::PathBuf,
    ptr,
    sync::{OnceLock, atomic::AtomicBool},
};

#[repr(C)]
struct NativeIdentity {
    pid: i32,
    ppid: i32,
    uid: u32,
    reserved: u32,
    start_us: u64,
    comm: [libc::c_char; 17],
}
#[repr(C)]
#[derive(Default)]
struct NativeHost {
    total: u64,
    internal_pages: u64,
    purgeable_pages: u64,
    wired_pages: u64,
    compressed_pages: u64,
    page_size: u64,
    swap_used: u64,
    swap_total: u64,
    pressure: i32,
}

unsafe extern "C" {
    fn capture_identities(out: *mut *mut NativeIdentity, count: *mut usize) -> i32;
    fn capture_host(out: *mut NativeHost) -> i32;
}

fn char_array(bytes: &[libc::c_char]) -> String {
    let bytes: Vec<u8> = bytes
        .iter()
        .take_while(|c| **c != 0)
        .map(|c| *c as u8)
        .collect();
    String::from_utf8_lossy(&bytes).into_owned()
}

pub fn identities() -> SampleResult<Vec<Proc>> {
    let mut pointer = ptr::null_mut();
    let mut count = 0;
    let error = unsafe { capture_identities(&mut pointer, &mut count) };
    if error != 0 {
        return Err(SampleError::os("kern_proc_all", error));
    }
    let rows = unsafe { std::slice::from_raw_parts(pointer, count) };
    let mut procs: BTreeMap<_, _> = rows
        .iter()
        .filter(|row| row.pid >= 0)
        .map(|row| {
            (
                row.pid,
                Proc::identity(
                    row.pid,
                    row.ppid,
                    row.uid,
                    Some(row.start_us),
                    char_array(&row.comm),
                ),
            )
        })
        .collect();
    unsafe { libc::free(pointer.cast::<c_void>()) };
    // Both enumeration APIs are used; an identity appearing between calls is read via BSDINFO.
    let n = unsafe { libc::proc_listallpids(ptr::null_mut(), 0) };
    if n <= 0 {
        return Err(SampleError::new("proc_listallpids", "enumeration_failed"));
    }
    let mut pids = vec![0_i32; n as usize + n as usize / 4 + 1];
    let received = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            std::mem::size_of_val(pids.as_slice()) as i32,
        )
    };
    if received <= 0 || received as usize >= pids.len() {
        return Err(SampleError::new("proc_listallpids", "incomplete_result"));
    }
    for pid in &pids[..received as usize] {
        if *pid > 0
            && !procs.contains_key(pid)
            && let Some(info) = bsd_info(*pid)
        {
            procs.insert(*pid, proc_from_bsd(&info));
        }
    }
    Ok(procs.into_values().collect())
}

fn bsd_info(pid: i32) -> Option<libc::proc_bsdinfo> {
    let mut info = unsafe { zeroed::<libc::proc_bsdinfo>() };
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size_of::<libc::proc_bsdinfo>() as i32,
        )
    };
    (bytes as usize == size_of::<libc::proc_bsdinfo>()).then_some(info)
}

fn proc_from_bsd(info: &libc::proc_bsdinfo) -> Proc {
    Proc::identity(
        info.pbi_pid as i32,
        info.pbi_ppid as i32,
        info.pbi_uid,
        Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec),
        char_array(&info.pbi_comm),
    )
}

fn matches_bsd(proc: &Proc, info: &libc::proc_bsdinfo) -> bool {
    proc.pid == info.pbi_pid as i32
        && proc.uid == info.pbi_uid
        && proc.start_time == Some(info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

/// Kernel boundary for one process window. Native and injected outcomes share
/// the same before/read/after identity checks used by MacSampler.
pub trait ProcessReader {
    fn identity(&mut self, pid: i32) -> Option<Proc>;
    fn footprint(&mut self, pid: i32) -> SampleResult<(u64, u64)>;
    fn resources(&mut self, pid: i32) -> SampleResult<(u64, u64, Option<u64>)> {
        self.footprint(pid).map(|(mem, cpu)| (mem, cpu, None))
    }
    fn exe_path(&mut self, pid: i32) -> SampleResult<String>;
    fn cwd(&mut self, pid: i32) -> SampleResult<String>;
    fn args(&mut self, pid: i32) -> SampleResult<sysctl::ProcArgs>;
    fn responsible_pid(&mut self, pid: i32) -> Option<i32>;
    /// Unix socket paths and directories the process holds open, sockets first;
    /// asked only of git fsmonitor daemons (R17).
    fn open_paths(&mut self, _pid: i32) -> Vec<String> {
        vec![]
    }
}

pub fn enrich_proc(
    proc: &mut Proc,
    account_uid: u32,
    reader: &mut impl ProcessReader,
) -> Vec<SampleError> {
    let Some(before) = reader.identity(proc.pid) else {
        return vec![SampleError::new("bsd_info", "unavailable_or_exited")];
    };
    if !proc.same_identity(&before) {
        return vec![SampleError::new("identity", "changed_before_read")];
    }
    let mut errors = Vec::new();
    let mut candidate = proc.clone();
    candidate.ppid = before.ppid;
    candidate.responsible_pid = reader.responsible_pid(proc.pid);
    if proc.uid == account_uid {
        match reader.resources(proc.pid) {
            Ok((mem, cpu, resident)) => {
                candidate.footprint_bytes = Some(mem);
                candidate.resident_bytes = resident;
                candidate.cpu_time_ns = Some(cpu);
            }
            Err(error) => errors.push(error),
        }
        match reader.exe_path(proc.pid) {
            Ok(path) => candidate.exe_path = Some(path),
            Err(error) => errors.push(error),
        }
        match reader.cwd(proc.pid) {
            Ok(path) => candidate.cwd = Some(path),
            Err(error) => errors.push(error),
        }
        match reader.args(proc.pid) {
            Ok(args) => {
                candidate.argv = args.argv;
                candidate.tmux = args.tmux;
                candidate.tmux_pane = args.tmux_pane;
                candidate.codex_thread_id = args.codex_thread_id;
            }
            Err(error) => errors.push(error),
        }
        if crate::attribute::git_fsmonitor(&candidate.argv) {
            candidate.git_dir = crate::attribute::git_dir_of(&reader.open_paths(proc.pid));
        }
    }
    if reader
        .identity(proc.pid)
        .is_some_and(|after| proc.same_identity(&after))
    {
        *proc = candidate;
    } else {
        errors.push(SampleError::new("identity", "changed_during_read"));
    }
    errors
}

struct NativeProcessReader<'a> {
    arguments: &'a mut [u8],
}
impl ProcessReader for NativeProcessReader<'_> {
    fn identity(&mut self, pid: i32) -> Option<Proc> {
        bsd_info(pid).as_ref().map(proc_from_bsd)
    }
    fn footprint(&mut self, pid: i32) -> SampleResult<(u64, u64)> {
        footprint(pid)
    }
    fn resources(&mut self, pid: i32) -> SampleResult<(u64, u64, Option<u64>)> {
        resources(pid)
    }
    fn exe_path(&mut self, pid: i32) -> SampleResult<String> {
        let mut path = [0_i8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        let size = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr().cast(), path.len() as u32) };
        if size > 0 {
            Ok(char_array(&path))
        } else {
            Err(SampleError::new("exe_path", "unavailable"))
        }
    }
    fn cwd(&mut self, pid: i32) -> SampleResult<String> {
        let mut vnode = unsafe { zeroed::<libc::proc_vnodepathinfo>() };
        let size = unsafe {
            libc::proc_pidinfo(
                pid,
                libc::PROC_PIDVNODEPATHINFO,
                0,
                (&mut vnode as *mut libc::proc_vnodepathinfo).cast(),
                size_of::<libc::proc_vnodepathinfo>() as i32,
            )
        };
        if size as usize == size_of::<libc::proc_vnodepathinfo>() {
            Ok(char_array(
                vnode.pvi_cdir.vip_path.as_slice().as_flattened(),
            ))
        } else {
            Err(SampleError::new("cwd", "unavailable"))
        }
    }
    fn args(&mut self, pid: i32) -> SampleResult<sysctl::ProcArgs> {
        read_args(pid, self.arguments)
    }
    fn responsible_pid(&mut self, pid: i32) -> Option<i32> {
        responsible_pid(pid)
    }
    fn open_paths(&mut self, pid: i32) -> Vec<String> {
        open_paths(pid)
    }
}

/// `struct proc_fileinfo` and `struct vnode_fdinfowithpath` (sys/proc_info.h);
/// the locked libc has neither, nor PROC_PIDFDVNODEPATHINFO.
#[repr(C)]
struct ProcFileInfo {
    fi_openflags: u32,
    fi_status: u32,
    fi_offset: i64,
    fi_type: i32,
    fi_guardflags: u32,
}
#[repr(C)]
struct VnodeFdInfoWithPath {
    pfi: ProcFileInfo,
    pvip: libc::vnode_info_path,
}
const PROC_PIDFDVNODEPATHINFO: libc::c_int = 2;
const VDIR: libc::c_int = 2;
/// `struct socket_fdinfo` is a deep union; only two fields are read, at the
/// offsets the macOS SDK's sys/proc_info.h gives (measured with offsetof,
/// 2026-10-25): `psi.soi_kind` and the unix socket's `sun_path` (104 bytes).
const PROC_PIDFDSOCKETINFO: libc::c_int = 3;
const PROX_FDTYPE_SOCKET: u32 = 2;
const SOCKET_FDINFO_SIZE: usize = 792;
const SOI_KIND_OFFSET: usize = 256;
const SOCKINFO_UN: i32 = 3;
const SUN_PATH_OFFSET: usize = 282;
const SUN_PATH_LEN: usize = 104;

/// The process's open descriptors; empty when the list cannot be read.
fn open_fds(pid: i32) -> Vec<libc::proc_fdinfo> {
    let item = size_of::<libc::proc_fdinfo>();
    let bytes =
        unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDLISTFDS, 0, std::ptr::null_mut(), 0) };
    if bytes <= 0 {
        return vec![];
    }
    // Room for a few descriptors opened between the two calls.
    let mut fds = vec![unsafe { zeroed::<libc::proc_fdinfo>() }; bytes as usize / item + 8];
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDLISTFDS,
            0,
            fds.as_mut_ptr().cast(),
            (fds.len() * item) as i32,
        )
    };
    fds.truncate(bytes.max(0) as usize / item);
    fds
}

/// R17: the unix socket paths, then the directories, a process holds open (a git
/// fsmonitor daemon listens on `<git dir>/fsmonitor--daemon.ipc`).
pub fn open_paths(pid: i32) -> Vec<String> {
    let fds = open_fds(pid);
    let sockets = fds
        .iter()
        .filter(|fd| fd.proc_fdtype == PROX_FDTYPE_SOCKET)
        .filter_map(|fd| {
            let mut buffer = [0_u64; SOCKET_FDINFO_SIZE / 8];
            let read = unsafe {
                libc::proc_pidfdinfo(
                    pid,
                    fd.proc_fd,
                    PROC_PIDFDSOCKETINFO,
                    buffer.as_mut_ptr().cast(),
                    SOCKET_FDINFO_SIZE as i32,
                )
            };
            let bytes: &[u8] =
                unsafe { std::slice::from_raw_parts(buffer.as_ptr().cast(), SOCKET_FDINFO_SIZE) };
            let kind = i32::from_ne_bytes(
                bytes[SOI_KIND_OFFSET..SOI_KIND_OFFSET + 4]
                    .try_into()
                    .ok()?,
            );
            let path = &bytes[SUN_PATH_OFFSET..SUN_PATH_OFFSET + SUN_PATH_LEN];
            let path = &path[..path.iter().position(|b| *b == 0).unwrap_or(SUN_PATH_LEN)];
            (read as usize == SOCKET_FDINFO_SIZE && kind == SOCKINFO_UN && !path.is_empty())
                .then(|| String::from_utf8_lossy(path).into_owned())
        });
    let dirs = fds
        .iter()
        .filter(|fd| fd.proc_fdtype == libc::PROX_FDTYPE_VNODE as u32)
        .filter_map(|fd| {
            let mut info = unsafe { zeroed::<VnodeFdInfoWithPath>() };
            let size = size_of::<VnodeFdInfoWithPath>();
            let read = unsafe {
                libc::proc_pidfdinfo(
                    pid,
                    fd.proc_fd,
                    PROC_PIDFDVNODEPATHINFO,
                    (&mut info as *mut VnodeFdInfoWithPath).cast(),
                    size as i32,
                )
            };
            (read as usize == size && info.pvip.vip_vi.vi_type == VDIR)
                .then(|| char_array(info.pvip.vip_path.as_slice().as_flattened()))
        });
    sockets
        .collect::<Vec<_>>()
        .into_iter()
        .chain(dirs)
        .collect()
}

pub fn footprint(pid: i32) -> SampleResult<(u64, u64)> {
    resources(pid).map(|(mem, cpu, _)| (mem, cpu))
}
fn cpu_nanoseconds(ticks: u64, numer: u32, denom: u32) -> SampleResult<u64> {
    if numer == 0 || denom == 0 {
        return Err(SampleError::new("rusage_v4", "timebase_unavailable"));
    }
    u64::try_from(u128::from(ticks) * u128::from(numer) / u128::from(denom))
        .map_err(|_| SampleError::new("rusage_v4", "cpu_time_overflow"))
}
// The locked libc exposes this SDK ABI but deprecates it in favour of a new
// dependency. Keep the existing dependency contract; no alternate conversion.
#[allow(deprecated)]
fn timebase() -> SampleResult<(u32, u32)> {
    static TIMEBASE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    TIMEBASE
        .get_or_init(|| {
            let mut info = unsafe { zeroed::<libc::mach_timebase_info>() };
            (unsafe { libc::mach_timebase_info(&mut info) } == 0
                && info.numer > 0
                && info.denom > 0)
                .then_some((info.numer, info.denom))
        })
        .ok_or_else(|| SampleError::new("rusage_v4", "timebase_unavailable"))
}
pub fn resources(pid: i32) -> SampleResult<(u64, u64, Option<u64>)> {
    let mut usage = unsafe { zeroed::<libc::rusage_info_v4>() };
    let result = unsafe {
        libc::proc_pid_rusage(
            pid,
            libc::RUSAGE_INFO_V4,
            (&mut usage as *mut libc::rusage_info_v4).cast(),
        )
    };
    if result != 0 {
        return Err(SampleError::os(
            "rusage_v4",
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO),
        ));
    }
    let cpu = usage
        .ri_user_time
        .checked_add(usage.ri_system_time)
        .ok_or_else(|| SampleError::new("rusage_v4", "cpu_time_overflow"))?;
    // rusage_info CPU counters are Mach ticks, not ns on Apple Silicon.
    // Convert only here; injected samplers and fixtures already supply ns.
    let (numer, denom) = timebase()?;
    Ok((
        usage.ri_phys_footprint,
        cpu_nanoseconds(cpu, numer, denom)?,
        Some(usage.ri_resident_size),
    ))
}

fn read_args(pid: i32, buffer: &mut [u8]) -> SampleResult<sysctl::ProcArgs> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = buffer.len();
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            3,
            buffer.as_mut_ptr().cast(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 {
        return Err(SampleError::os(
            "procargs",
            std::io::Error::last_os_error()
                .raw_os_error()
                .unwrap_or(libc::EIO),
        ));
    }
    if size > buffer.len() {
        return Err(SampleError::new("procargs", "oversized_result"));
    }
    sysctl::parse_procargs(&buffer[..size])
}

fn argmax() -> SampleResult<usize> {
    let mut mib = [libc::CTL_KERN, libc::KERN_ARGMAX];
    let mut value = 0_i32;
    let mut size = size_of::<i32>();
    let result = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            2,
            (&mut value as *mut i32).cast(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    if result != 0 || value <= 0 || size != size_of::<i32>() {
        return Err(SampleError::new("argmax", "unavailable"));
    }
    Ok(value as usize)
}

type Responsible = unsafe extern "C" fn(i32) -> i32;
fn responsible_function() -> Option<Responsible> {
    static SYMBOL: OnceLock<Option<Responsible>> = OnceLock::new();
    *SYMBOL.get_or_init(|| {
        let symbol = unsafe {
            libc::dlsym(
                libc::RTLD_DEFAULT,
                c"responsibility_get_pid_responsible_for_pid".as_ptr(),
            )
        };
        if symbol.is_null() {
            None
        } else {
            Some(unsafe { std::mem::transmute::<*mut c_void, Responsible>(symbol) })
        }
    })
}
pub fn responsible_value(value: Option<i32>) -> Option<i32> {
    value.filter(|pid| *pid > 1)
}
pub fn responsible_pid(pid: i32) -> Option<i32> {
    responsible_value(responsible_function().map(|function| unsafe { function(pid) }))
}

pub fn host() -> SampleResult<crate::snapshot::Host> {
    let mut raw = NativeHost::default();
    let result = unsafe { capture_host(&mut raw) };
    if result != 0 {
        return Err(SampleError::os("host", result));
    }
    let mut host = sysctl::host_from_pages(
        raw.total,
        sysctl::PageCounts {
            page_size: raw.page_size,
            internal: raw.internal_pages,
            purgeable: raw.purgeable_pages,
            wired: raw.wired_pages,
            compressed: raw.compressed_pages,
        },
        raw.swap_used,
        raw.pressure,
    )?;
    // R21: the swap files' current total (macOS grows them on demand).
    host.swap_total_bytes = Some(raw.swap_total);
    Ok(host)
}

fn issue(issues: &mut BTreeMap<(&'static str, String), usize>, error: SampleError) {
    *issues.entry((error.stage, error.code)).or_default() += 1;
}

pub struct MacSampler {
    worker: Worker,
    arguments: Vec<u8>,
    cpu: CpuMeter,
}
impl MacSampler {
    pub fn new() -> SampleResult<Self> {
        Self::with_source(top::Scheduled::new(PlatformSlowSource))
    }
    pub fn with_source(source: impl SlowSource) -> SampleResult<Self> {
        Self::with_source_at(cache::cache_path()?, source)
    }
    pub fn with_cache_path(path: PathBuf) -> SampleResult<Self> {
        Self::with_source_at(path, top::Scheduled::new(PlatformSlowSource))
    }
    pub fn with_source_at(path: PathBuf, source: impl SlowSource) -> SampleResult<Self> {
        let arguments = vec![0; argmax()?];
        Ok(Self {
            worker: Worker::start(path, source),
            arguments,
            cpu: CpuMeter::default(),
        })
    }
    pub fn cache_state(&self) -> cache::CacheState {
        self.worker.state()
    }
}

impl Sampler for MacSampler {
    fn set_active(&mut self, active: bool) {
        self.worker.set_active(active);
        if !active {
            self.cpu.reset();
        }
    }
    fn sample(&mut self) -> SampleResult<Snapshot> {
        let taken_at = sysctl::now_timestamp()?;
        let now = sysctl::unix_now();
        let mut issues = BTreeMap::new();
        let mut procs = identities()?;
        let uid = unsafe { libc::getuid() };
        let mut reader = NativeProcessReader {
            arguments: &mut self.arguments,
        };
        for proc in &mut procs {
            for error in enrich_proc(proc, uid, &mut reader) {
                issue(&mut issues, error);
            }
        }
        let tmux_panes = match tmux::capture() {
            Ok(panes) => panes,
            Err(e) => {
                issue(&mut issues, e);
                vec![]
            }
        };
        if tmux_panes
            .iter()
            .any(|p| p.pane_title.is_none() || p.window_name.is_none() || p.cwd.is_none())
        {
            issue(
                &mut issues,
                SampleError::new("tmux", "invalid_pane_metadata"),
            );
        }
        let socket_candidates = ports::candidate_pids(&procs, &tmux_panes, uid);
        for proc in procs
            .iter_mut()
            .filter(|proc| socket_candidates.contains(&proc.pid))
        {
            if !bsd_info(proc.pid).is_some_and(|info| matches_bsd(proc, &info)) {
                continue;
            }
            match ports::sockets(proc.pid) {
                Ok(result) if bsd_info(proc.pid).is_some_and(|info| matches_bsd(proc, &info)) => {
                    ports::apply_socket_result(proc, Ok(result))?;
                }
                Ok(_) => issue(&mut issues, SampleError::new("sockets", "identity_changed")),
                Err(error) => {
                    if let Err(error) = ports::apply_socket_result(proc, Err(error)) {
                        issue(&mut issues, error);
                    }
                }
            }
        }
        let mut host = host()?;
        host.cpu = Some(self.cpu.sample());
        self.worker.memory().merge(&mut host, now);
        let cache = self.worker.state();
        cache.merge(&mut procs, now);
        if let Some(error) = &cache.error {
            issue(&mut issues, error.clone());
        }
        if let Some(code) = cache
            .cache
            .as_ref()
            .and_then(|cache| cache.cpu_issue.as_ref())
        {
            issue(&mut issues, SampleError::new("top", code.clone()));
        }
        if responsible_function().is_none() {
            issue(
                &mut issues,
                SampleError::new("responsible", "symbol_unavailable"),
            );
        }
        let issues = issues
            .into_iter()
            .map(|((stage, code), count)| SampleIssue {
                stage: stage.into(),
                pid: None,
                code: format!("{code};count={count}"),
            })
            .collect();
        Ok(Snapshot {
            taken_at,
            host: Some(host),
            procs,
            tmux_panes,
            system_procs: cache.status(now),
            issues,
        })
    }
}

pub struct PlatformSlowSource;
impl SlowSource for PlatformSlowSource {
    fn capture_memory(&mut self, stop: &AtomicBool) -> Option<super::system_memory::MemoryCache> {
        Some(super::system_memory::capture(stop))
    }
    fn capture(&mut self, stop: &AtomicBool) -> SampleResult<SystemCache> {
        let as_of = sysctl::unix_now();
        let before: BTreeMap<_, _> = identities()?
            .into_iter()
            .map(|proc| (proc.pid, proc))
            .collect();
        let data = top::capture(stop)?;
        let after = identities()?;
        let valid: BTreeSet<_> = after
            .iter()
            .filter(|proc| {
                before
                    .get(&proc.pid)
                    .is_some_and(|old| old.same_identity(proc))
            })
            .map(|proc| proc.pid)
            .collect();
        let rows = data
            .rows
            .into_iter()
            .filter_map(|(pid, row)| {
                if !valid.contains(&pid) {
                    return None;
                }
                let proc = before.get(&pid)?;
                Some((
                    pid,
                    CachedProc {
                        start_time: proc.start_time?,
                        uid: proc.uid,
                        footprint_bytes: row.footprint_bytes,
                        cpu_pct_sampled: row.cpu_pct_sampled,
                    },
                ))
            })
            .collect();
        Ok(SystemCache {
            as_of,
            rows,
            cpu_issue: data.cpu_issue,
        })
    }
}

#[cfg(test)]
mod phase09_tests {
    #[test]
    fn phase09_cpu_timebase_converts_once_without_intermediate_overflow() {
        assert_eq!(super::cpu_nanoseconds(3, 125, 3).unwrap(), 125);
        assert_eq!(super::cpu_nanoseconds(17, 1, 1).unwrap(), 17);
        assert_eq!(super::cpu_nanoseconds(u64::MAX, 2, 2).unwrap(), u64::MAX);
        assert!(super::cpu_nanoseconds(1, 1, 0).is_err());
        assert!(super::cpu_nanoseconds(1, 0, 1).is_err());
        assert!(super::cpu_nanoseconds(u64::MAX, 2, 1).is_err());
    }
}
