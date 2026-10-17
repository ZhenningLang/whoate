use std::{
    fs,
    os::unix::fs::DirBuilderExt,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use whoate_core::{
    sample::{MacSampler, SampleError, SampleResult, Sampler},
    snapshot::{SampleIssue, Snapshot, SystemProcs},
};

unsafe extern "C" {
    fn getuid() -> u32;
}
pub fn uid() -> u32 {
    unsafe { getuid() }
}

/// The worker is dropped before its private cache directory is removed.
pub struct NativeSampler {
    inner: MacSampler,
    _scratch: Option<Scratch>,
    fresh: bool,
    uid: u32,
}
pub struct Scratch(pub PathBuf);
impl Scratch {
    pub fn new() -> SampleResult<Self> {
        static SERIAL: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "whoate-cli-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .map_err(|_| SampleError::new("temporary_cache", "create_failed"))?;
        Ok(Self(root))
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            // Cleanup is best effort at process teardown; no paths/raw content in logs.
            eprintln!(
                "whoate: temporary_cache cleanup_failed ({:?})",
                error.kind()
            );
        }
    }
}
impl NativeSampler {
    pub fn new(fresh: bool) -> SampleResult<Self> {
        let scratch = if fresh { Some(Scratch::new()?) } else { None };
        let inner = match &scratch {
            Some(root) => MacSampler::with_cache_path(root.0.join("system.json"))?,
            None => MacSampler::new()?,
        };
        Ok(Self {
            inner,
            _scratch: scratch,
            fresh,
            uid: uid(),
        })
    }
}
pub fn without_slow_cache(snapshot: &mut Snapshot, uid: u32) {
    if let Some(host) = &mut snapshot.host {
        whoate_core::sample::system_memory::MemoryCache::missing("fresh_requested").merge(host, 0);
    }
    for p in &mut snapshot.procs {
        if p.uid != uid {
            p.footprint_bytes = None;
            p.cpu_pct_sampled = None;
        }
    }
    snapshot.system_procs = SystemProcs::missing();
    snapshot.issues.push(SampleIssue {
        stage: "cli".into(),
        pid: None,
        code: "fresh_requested".into(),
    });
}
impl Sampler for NativeSampler {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        let mut snapshot = self.inner.sample()?;
        if self.fresh {
            without_slow_cache(&mut snapshot, self.uid);
        }
        Ok(snapshot)
    }
}
