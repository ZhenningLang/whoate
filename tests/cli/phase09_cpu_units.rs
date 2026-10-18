//! One-shot native calibration. Compiled by the phase verifier with the current
//! release core rlib, not auto-discovered as a repeatedly executed cargo test.
use std::{
    hint::black_box,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use whoate_core::{
    engine::{Clock, Context, Engine},
    recognizers::ConfigReader,
    sample::{SampleResult, Sampler, libproc},
    snapshot::Snapshot,
    verdict::Language,
};

fn busy() {
    unsafe {
        libc::fcntl(0, libc::F_SETFL, libc::O_NONBLOCK);
    }
    let mut n = 1_u64;
    loop {
        for _ in 0..10000 {
            n = black_box(n.wrapping_mul(3).wrapping_add(1));
        }
        let mut byte = [0_u8];
        let got = unsafe { libc::read(0, byte.as_mut_ptr().cast(), 1) };
        if got == 0 {
            break;
        }
        if got == 1 {
            let mut r: libc::rusage = unsafe { std::mem::zeroed() };
            assert_eq!(unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut r) }, 0);
            let ns = (r.ru_utime.tv_sec + r.ru_stime.tv_sec) as u64 * 1_000_000_000
                + (r.ru_utime.tv_usec + r.ru_stime.tv_usec) as u64 * 1000;
            println!("{ns}");
        }
    }
}
struct Owned(Child);
impl Drop for Owned {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
struct One {
    child: Owned,
    output: BufReader<std::process::ChildStdout>,
    references: Arc<Mutex<Vec<u64>>>,
}
impl Sampler for One {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        self.child
            .0
            .stdin
            .as_mut()
            .unwrap()
            .write_all(b"s")
            .unwrap();
        let mut line = String::new();
        self.output.read_line(&mut line).unwrap();
        self.references
            .lock()
            .unwrap()
            .push(line.trim().parse().unwrap());
        let pid = self.child.0.id() as i32;
        let mut p = libproc::identities()?
            .into_iter()
            .find(|p| p.pid == pid)
            .unwrap();
        let (mem, cpu, resident) = libproc::resources(pid)?;
        p.footprint_bytes = Some(mem);
        p.cpu_time_ns = Some(cpu);
        p.resident_bytes = resident;
        Ok(Snapshot::historical("2026-10-19T00:00:00Z".into(), vec![p]))
    }
}
struct Times {
    start: Instant,
    readings: Arc<Mutex<Vec<u128>>>,
}
impl Clock for Times {
    fn now_ns(&self) -> u128 {
        let n = self.start.elapsed().as_nanos();
        self.readings.lock().unwrap().push(n);
        n
    }
    fn wait(&mut self, d: Duration) {
        std::thread::sleep(d);
    }
}
fn main() {
    if std::env::args().any(|a| a == "--busy") {
        busy();
        return;
    }
    let home = std::path::PathBuf::from(std::env::args().nth(1).expect("isolated home required"));
    let mut child = Owned(
        Command::new(std::env::current_exe().unwrap())
            .arg("--busy")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap(),
    );
    let output = BufReader::new(child.0.stdout.take().unwrap());
    let refs = Arc::new(Mutex::new(vec![]));
    let readings = Arc::new(Mutex::new(vec![]));
    let mut engine = Engine::with_sampler(
        One {
            child,
            output,
            references: refs.clone(),
        },
        Times {
            start: Instant::now(),
            readings: readings.clone(),
        },
        Context {
            cfg: ConfigReader::isolated(home),
            uid: unsafe { libc::getuid() },
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let report = engine.sample_once().unwrap();
    let whoate = report.owners[0].cpu_pct.unwrap();
    let refs = refs.lock().unwrap();
    let times = readings.lock().unwrap();
    assert_eq!(refs.len(), 2);
    assert_eq!(times.len(), 2);
    let reference = (refs[1] - refs[0]) as f64 / (times[1] - times[0]) as f64 * 100.0;
    let ratio = whoate / reference;
    println!(
        "CPU_UNIT whoate_pct={whoate} getrusage_pct={reference} ratio={ratio} reference_ns={:?} wall_ns={:?}",
        *refs, *times
    );
    assert!(
        reference > 0.0 && (0.5..=2.0).contains(&ratio),
        "native CPU unit mismatch"
    );
}
