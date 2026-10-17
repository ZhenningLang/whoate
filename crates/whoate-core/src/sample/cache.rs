use super::{SampleError, SampleResult, sysctl};
use crate::snapshot::{Freshness, Proc, SystemProcs};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

pub const INTERVAL_SECONDS: i64 = 6;
pub const FRESH_SECONDS: i64 = 30;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CachedProc {
    pub start_time: u64,
    pub uid: u32,
    pub footprint_bytes: u64,
    pub cpu_pct_sampled: Option<f64>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SystemCache {
    pub as_of: i64,
    pub rows: BTreeMap<i32, CachedProc>,
    /// Additive and optional so existing caches remain readable.
    #[serde(default)]
    pub cpu_issue: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct CacheState {
    pub cache: Option<SystemCache>,
    pub error: Option<SampleError>,
}

impl CacheState {
    pub fn load(path: &Path) -> Self {
        match fs::read(path) {
            Ok(bytes) => match serde_json::from_slice::<SystemCache>(&bytes) {
                Ok(cache) if !cache.rows.is_empty() => Self {
                    cache: Some(cache),
                    error: None,
                },
                _ => Self {
                    cache: None,
                    error: Some(SampleError::new("cache", "invalid_cache")),
                },
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => Self {
                cache: None,
                error: Some(SampleError::os(
                    "cache_read",
                    e.raw_os_error().unwrap_or(libc::EIO),
                )),
            },
        }
    }
    pub fn status(&self, now: i64) -> SystemProcs {
        let Some(cache) = &self.cache else {
            return SystemProcs::missing();
        };
        let age = now.checked_sub(cache.as_of);
        let fresh = self.error.is_none()
            && !cache.rows.is_empty()
            && age.is_some_and(|age| (0..=FRESH_SECONDS).contains(&age));
        SystemProcs {
            status: if fresh {
                Freshness::Fresh
            } else {
                Freshness::Stale
            },
            as_of: sysctl::timestamp(cache.as_of).ok(),
        }
    }
    pub fn merge(&self, procs: &mut [Proc], now: i64) {
        let Some(cache) = &self.cache else {
            return;
        };
        if cache.as_of > now {
            return;
        }
        for proc in procs {
            if let Some(row) = cache
                .rows
                .get(&proc.pid)
                .filter(|row| Some(row.start_time) == proc.start_time && row.uid == proc.uid)
            {
                if proc.footprint_bytes.is_none() {
                    proc.footprint_bytes = Some(row.footprint_bytes);
                }
                if proc.cpu_pct_sampled.is_none() {
                    proc.cpu_pct_sampled = row.cpu_pct_sampled;
                }
            }
        }
    }
    pub fn update(&mut self, result: SampleResult<SystemCache>, path: &Path) {
        match result {
            Ok(cache) if !cache.rows.is_empty() => match write_atomic(path, &cache) {
                Ok(()) => {
                    self.cache = Some(cache);
                    self.error = None;
                }
                Err(error) => self.error = Some(error),
            },
            Ok(_) => self.error = Some(SampleError::new("cache", "empty_result")),
            Err(error) => self.error = Some(error),
        }
    }
}

pub fn should_refresh(now: Duration, previous: Option<Duration>) -> bool {
    previous.is_none_or(|previous| {
        now >= previous.saturating_add(Duration::from_secs(INTERVAL_SECONDS as u64))
    })
}

pub fn cache_path() -> SampleResult<PathBuf> {
    if let Some(path) = std::env::var_os("WHOATE_CACHE_DIR") {
        return Ok(PathBuf::from(path).join("system.json"));
    }
    let home =
        std::env::var_os("HOME").ok_or_else(|| SampleError::new("cache", "home_unavailable"))?;
    Ok(PathBuf::from(home).join("Library/Caches/whoate/system.json"))
}

pub fn write_atomic(path: &Path, cache: &SystemCache) -> SampleResult<()> {
    if cache.rows.is_empty() {
        return Err(SampleError::new("cache", "empty_result"));
    }
    write_json_atomic(path, cache)
}
pub(crate) fn write_json_atomic(path: &Path, cache: &impl Serialize) -> SampleResult<()> {
    let parent = path
        .parent()
        .ok_or_else(|| SampleError::new("cache", "invalid_destination"))?;
    fs::create_dir_all(parent)
        .map_err(|e| SampleError::os("cache_mkdir", e.raw_os_error().unwrap_or(libc::EIO)))?;
    // PID plus a monotonic counter prevents two independently owned samplers colliding.
    static SERIAL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let temporary = parent.join(format!(
        ".system.{}.{}.tmp",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    let cleanup = Cleanup(temporary.clone());
    let result = (|| -> SampleResult<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)
            .map_err(|e| SampleError::os("cache_open", e.raw_os_error().unwrap_or(libc::EIO)))?;
        let bytes =
            serde_json::to_vec(cache).map_err(|_| SampleError::new("cache", "serialize_failed"))?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| SampleError::os("cache_write", e.raw_os_error().unwrap_or(libc::EIO)))?;
        fs::rename(&temporary, path)
            .map_err(|e| SampleError::os("cache_replace", e.raw_os_error().unwrap_or(libc::EIO)))?;
        Ok(())
    })();
    drop(cleanup);
    result
}

pub trait SlowSource: Send + 'static {
    fn capture(&mut self, stop: &AtomicBool) -> SampleResult<SystemCache>;
    fn capture_memory(&mut self, _: &AtomicBool) -> Option<super::system_memory::MemoryCache> {
        None
    }
}

pub trait Clock: Send + Sync + 'static {
    fn now(&self) -> Duration;
}
struct MonotonicClock(std::time::Instant);
impl Clock for MonotonicClock {
    fn now(&self) -> Duration {
        self.0.elapsed()
    }
}

pub struct Worker {
    state: Arc<Mutex<CacheState>>,
    memory: Arc<Mutex<super::system_memory::MemoryCache>>,
    signal: Arc<(Mutex<bool>, Condvar)>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Worker {
    pub fn start(path: PathBuf, source: impl SlowSource) -> Self {
        Self::start_with_clock(
            path,
            source,
            Arc::new(MonotonicClock(std::time::Instant::now())),
        )
    }
    pub fn start_with_clock(
        path: PathBuf,
        mut source: impl SlowSource,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let state = Arc::new(Mutex::new(CacheState::load(&path)));
        let memory_path = path.with_file_name("system-memory.json");
        let memory = Arc::new(Mutex::new(super::system_memory::MemoryCache::load(
            &memory_path,
        )));
        let memory_copy = memory.clone();
        let signal = Arc::new((Mutex::new(false), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let (state_copy, signal_copy, stop_copy) = (state.clone(), signal.clone(), stop.clone());
        let thread = thread::spawn(move || {
            let mut previous = None;
            while !stop_copy.load(Ordering::Acquire) {
                let now = clock.now();
                if should_refresh(now, previous) {
                    previous = Some(now);
                    let result = source.capture(&stop_copy);
                    if stop_copy.load(Ordering::Acquire) {
                        break;
                    }
                    // Disk serialization stays off the fast-reader mutex.
                    let mut next = state_copy.lock().expect("cache state").clone();
                    next.update(result, &path);
                    *state_copy.lock().expect("cache state") = next;
                    if let Some(result) = source.capture_memory(&stop_copy) {
                        if stop_copy.load(Ordering::Acquire) {
                            break;
                        }
                        let mut next = memory_copy.lock().expect("memory state").clone();
                        next.update(result, &memory_path);
                        *memory_copy.lock().expect("memory state") = next;
                    }
                }
                let (lock, condition) = &*signal_copy;
                let guard = lock.lock().expect("worker signal");
                if *guard {
                    break;
                }
                let remaining = previous
                    .unwrap()
                    .saturating_add(Duration::from_secs(INTERVAL_SECONDS as u64))
                    .saturating_sub(clock.now());
                // Spurious wakeups re-enter the same clock gate. An injected clock
                // can notify this wait without performing a real six-second sleep.
                let _ = condition
                    .wait_timeout(guard, remaining)
                    .expect("worker wait");
            }
        });
        Self {
            state,
            memory,
            signal,
            stop,
            thread: Some(thread),
        }
    }
    pub fn state(&self) -> CacheState {
        self.state.lock().expect("cache state").clone()
    }
    pub fn memory(&self) -> super::system_memory::MemoryCache {
        self.memory.lock().expect("memory state").clone()
    }
    pub fn clock_changed(&self) {
        let (lock, condition) = &*self.signal;
        let _guard = lock.lock().expect("worker signal");
        condition.notify_all();
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        let (lock, condition) = &*self.signal;
        *lock.lock().expect("worker signal") = true;
        condition.notify_all();
        if let Some(thread) = self.thread.take() {
            // Source implementations must honour stop; the platform runner kills its own child.
            if thread.join().is_err() {
                eprintln!("whoate sample: slow_worker_panicked");
            }
        }
    }
}
