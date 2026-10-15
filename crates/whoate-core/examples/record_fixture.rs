use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    sync::atomic::{AtomicBool, Ordering},
};
use whoate_core::sample::{MacSampler, SampleError, SampleResult, Sampler, fixture};

static INTERRUPTED: AtomicBool = AtomicBool::new(false);
extern "C" fn interrupt(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::Release);
}
fn check_interrupted() -> SampleResult<()> {
    if INTERRUPTED.load(Ordering::Acquire) {
        Err(SampleError::new("record", "interrupted"))
    } else {
        Ok(())
    }
}

fn run() -> SampleResult<()> {
    // Arm INT/TERM before capture or any write. Handler only sets an atomic flag.
    unsafe {
        libc::signal(libc::SIGINT, interrupt as *const () as libc::sighandler_t);
        libc::signal(libc::SIGTERM, interrupt as *const () as libc::sighandler_t);
    }
    let mut source = None;
    let mut output = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--source" => {
                source =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        SampleError::new("record", "missing_source")
                    })?))
            }
            "--output" => {
                output =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        SampleError::new("record", "missing_output")
                    })?))
            }
            "--help" => {
                println!(
                    "record-fixture [--source ARCHIVE] --output DEST\nWithout --source: read this Mac. Output is always redacted; existing DEST is never overwritten."
                );
                return Ok(());
            }
            _ => return Err(SampleError::new("record", "unknown_argument")),
        }
    }
    let output = output.ok_or_else(|| SampleError::new("record", "output_required"))?;
    eprintln!(
        "record-fixture: stage=capture source={}",
        if source.is_some() { "archive" } else { "live" }
    );
    let snapshot = if let Some(source) = source {
        let bytes =
            fs::read(source).map_err(|_| SampleError::new("record", "source_read_failed"))?;
        fixture::import_archive(&bytes)?
    } else {
        MacSampler::new()?.sample()?
    };
    let count = snapshot.procs.len();
    let snapshot = fixture::redact(snapshot);
    fixture::validate_safe(&snapshot)?;
    let bytes = serde_json::to_vec_pretty(&snapshot)
        .map_err(|_| SampleError::new("record", "serialize_failed"))?;
    check_interrupted()?;
    let parent = output
        .parent()
        .ok_or_else(|| SampleError::new("record", "invalid_destination"))?;
    fs::create_dir_all(parent).map_err(|_| SampleError::new("record", "create_parent_failed"))?;
    // Arm cleanup before the first write; never overwrite an existing fixture.
    struct Cleanup {
        path: PathBuf,
        completed: bool,
    }
    impl Drop for Cleanup {
        fn drop(&mut self) {
            if !self.completed {
                let _ = fs::remove_file(&self.path);
            }
        }
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&output)
        .map_err(|_| SampleError::new("record", "destination_exists_or_unwritable"))?;
    let mut cleanup = Cleanup {
        path: output,
        completed: false,
    };
    for chunk in bytes.chunks(65_536) {
        check_interrupted()?;
        file.write_all(chunk)
            .map_err(|_| SampleError::new("record", "write_failed"))?;
    }
    check_interrupted()?;
    file.sync_all()
        .map_err(|_| SampleError::new("record", "write_failed"))?;
    cleanup.completed = true;
    eprintln!(
        "record-fixture: stage=complete procs={count} bytes={} privacy=checked",
        bytes.len()
    );
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("record-fixture: {error}");
        std::process::exit(1);
    }
}
