//! Bounded, cancellable calls to read-only platform tools. Never expose stderr.
use super::{SampleError, SampleResult};
use std::{
    io::Read,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

pub struct Output {
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub fn read_command(
    program: &str,
    args: &[&str],
    stop: &AtomicBool,
    timeout: Duration,
    output_limit: usize,
) -> SampleResult<Output> {
    let mut child = Command::new(program)
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| SampleError::os("command_spawn", e.raw_os_error().unwrap_or(libc::EIO)))?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let limit = output_limit.saturating_add(1) as u64;
    let reader = |pipe: Box<dyn Read + Send>| {
        thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.take(limit).read_to_end(&mut bytes).map(|_| bytes)
        })
    };
    let out_reader = reader(Box::new(stdout));
    let err_reader = reader(Box::new(stderr));
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Ok(None) => {}
            Err(_) => break Err(SampleError::new("command", "wait_failed")),
        }
        if stop.load(Ordering::Acquire) || started.elapsed() >= timeout {
            break Err(SampleError::new(
                "command",
                if stop.load(Ordering::Acquire) {
                    "cancelled"
                } else {
                    "timeout"
                },
            ));
        }
        thread::sleep(Duration::from_millis(1));
    };
    if status.is_err() {
        // Only the child this function launched is terminated, never an existing process.
        let _ = child.kill();
        let _ = child.wait();
    }
    let stdout = out_reader
        .join()
        .map_err(|_| SampleError::new("command", "reader_panicked"))?
        .map_err(|_| SampleError::new("command", "read_failed"))?;
    let stderr = err_reader
        .join()
        .map_err(|_| SampleError::new("command", "reader_panicked"))?
        .map_err(|_| SampleError::new("command", "read_failed"))?;
    if stdout.len() > output_limit || stderr.len() > output_limit {
        return Err(SampleError::new("command", "output_limit"));
    }
    Ok(Output {
        success: status?.success(),
        stdout,
        stderr,
    })
}
