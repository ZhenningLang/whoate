//! Debug-only fixture playback for GUI development. Never exported in release.
use super::{Source, WhoateEngine};
use std::{
    ffi::{CStr, c_char},
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use whoate_core::{
    engine::{Context, Engine, SystemClock},
    model::Report,
    recognizers::{
        ConfigReader,
        docker::{ContainerStats, DockerSource},
    },
    sample::{SampleResult, Sampler},
    snapshot::Snapshot,
    verdict::Language,
};

/// Returns the same historical snapshot on every tick.
pub(crate) struct Replay(pub Snapshot);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.clone())
    }
}
/// Historical data must not be mixed with this machine's live containers.
struct NoDocker;
impl DockerSource for NoDocker {
    fn capture(
        &mut self,
        _: &std::sync::atomic::AtomicBool,
    ) -> Result<Vec<ContainerStats>, &'static str> {
        Err("replay")
    }
}

/// A file with `owners` replays as a whoate/v1 report; otherwise it is a
/// Snapshot run through the full pipeline with an empty isolated home.
/// Replayed identities never authorize whoate_kill. NULL on any failure.
/// # Safety
/// `path` is NULL or a NUL-terminated string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn whoate_engine_new_from_fixture(path: *const c_char) -> *mut WhoateEngine {
    std::panic::catch_unwind(|| {
        open(path).map_or(std::ptr::null_mut(), |e| Box::into_raw(Box::new(e)))
    })
    .unwrap_or(std::ptr::null_mut())
}
fn open(path: *const c_char) -> Option<WhoateEngine> {
    if path.is_null() {
        return None;
    }
    let path = unsafe { CStr::from_ptr(path) }.to_str().ok()?;
    let value: serde_json::Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    if value.get("owners").is_some() {
        let report: Report = serde_json::from_value(value).ok()?;
        return Some(WhoateEngine {
            source: Source::Report(Box::new(report)),
            last: None,
            scratch: None,
        });
    }
    let snapshot: Snapshot = serde_json::from_value(value).ok()?;
    replay_engine(Replay(snapshot), Language::system())
}
pub(crate) fn replay_engine(
    sampler: impl Sampler + 'static,
    language: Language,
) -> Option<WhoateEngine> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let home: PathBuf = std::env::temp_dir().join(format!(
        "whoate-ffi-replay-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&home).ok()?;
    let context = Context {
        cfg: ConfigReader::isolated(home.clone()).with_docker_source(NoDocker),
        uid: unsafe { libc::getuid() },
        language,
        // Historical pids belong to another moment; never probe live processes.
        platform_probe: |_| Ok(false),
    };
    let mut engine = match Engine::try_with_sampler(sampler, SystemClock::default(), context) {
        Ok(engine) => WhoateEngine::live(engine, false),
        Err(_) => {
            let _ = std::fs::remove_dir_all(&home);
            return None;
        }
    };
    engine.scratch = Some(home);
    Some(engine)
}
