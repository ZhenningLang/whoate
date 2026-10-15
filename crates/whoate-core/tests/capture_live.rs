mod common;
use common::Temp;
use std::{
    sync::atomic::AtomicBool,
    time::{Duration, Instant},
};
use whoate_core::{
    sample::{MacSampler, Sampler, libproc, sysctl, top},
    snapshot::Freshness,
};

fn live_enabled() {
    assert_eq!(
        std::env::var("WHOATE_CAPTURE_LIVE").as_deref(),
        Ok("1"),
        "live acceptance must be explicitly enabled, never silently skipped"
    );
    if cfg!(debug_assertions) {
        panic!("live acceptance must measure release code");
    }
    assert!(
        std::env::var_os("WHOATE_CACHE_DIR").is_some(),
        "live acceptance requires isolated cache"
    );
}

#[test]
fn footprint_ten_preselected_oldest_comparable_account_pids_match_top() {
    live_enabled();
    let uid = unsafe { libc::getuid() };
    let now = sysctl::unix_now();
    let mut sampler = MacSampler::new().unwrap();
    let fast = sampler.sample().unwrap();
    // Freeze selection BEFORE invoking top: >=128MiB avoids top's MiB quantization
    // dominating a 1% threshold; oldest processes reduce startup churn, not failures.
    let mut candidates: Vec<_> = fast
        .procs
        .iter()
        .filter(|proc| {
            proc.uid == uid
                && proc
                    .footprint_bytes
                    .is_some_and(|bytes| bytes >= 128 * 1024 * 1024)
                && proc
                    .start_time
                    .is_some_and(|start| start / 1_000_000 + 60 < now as u64)
        })
        .collect();
    candidates.sort_by_key(|proc| (proc.start_time, proc.pid));
    let selected: Vec<_> = candidates.into_iter().take(10).cloned().collect();
    assert_eq!(
        selected.len(),
        10,
        "QA12: fewer than 10 comparable account pids before top"
    );
    println!(
        "QA12 selection=oldest-start,pid uid={uid} min_footprint=134217728 min_age_s=60 frozen_pids={:?}",
        selected.iter().map(|proc| proc.pid).collect::<Vec<_>>()
    );
    let window = Instant::now();
    let sample = top::capture(&AtomicBool::new(false)).unwrap();
    let after = libproc::identities().unwrap();
    let mut all_pass = true;
    for proc in &selected {
        let stable = after.iter().find(|after| proc.same_identity(after));
        let Some(row) = sample
            .rows
            .get(&proc.pid)
            .filter(|row| row.footprint_bytes > 0)
        else {
            println!("QA12 pid={} invalid=missing_or_zero_top", proc.pid);
            all_pass = false;
            continue;
        };
        if stable.is_none() {
            println!("QA12 pid={} invalid=exited_or_reused", proc.pid);
            all_pass = false;
            continue;
        }
        let bytes = proc.footprint_bytes.unwrap();
        let error = bytes.abs_diff(row.footprint_bytes) as f64 / row.footprint_bytes as f64;
        let current = libproc::footprint(proc.pid).unwrap().0;
        let movement = bytes.abs_diff(current) as f64 / bytes as f64;
        println!(
            "QA12 pid={} start_us={} fast_bytes={} top_bytes={} after_bytes={} window_ms={:.3} error={:.6} movement={:.6}",
            proc.pid,
            proc.start_time.unwrap(),
            bytes,
            row.footprint_bytes,
            current,
            window.elapsed().as_secs_f64() * 1000.0,
            error,
            movement
        );
        if error >= 0.01 || movement >= 0.01 {
            all_pass = false;
        }
    }
    assert!(
        all_pass,
        "QA12: every frozen pid must be a stable identity and have <1% error; no replacement or averaging"
    );
}

#[test]
fn sampling_entire_fast_round_under_50ms_then_system_cache_becomes_fresh() {
    live_enabled();
    let dir = Temp::new();
    // Parent verify supplies an isolated cache; this test checks its initial missing
    // case by creating a source worker with its own previously unused destination.
    let path =
        std::path::PathBuf::from(std::env::var_os("WHOATE_CACHE_DIR").unwrap()).join("system.json");
    // A previous acceptance test may have populated it. Do not delete shared state:
    // use an explicitly isolated constructor for this test.
    let mut sampler = MacSampler::with_cache_path(dir.0.join("system.json")).unwrap();
    assert!(!dir.0.join("system.json").exists());
    let start = Instant::now();
    let snapshot = sampler.sample().unwrap();
    let elapsed_ms = start.elapsed().as_secs_f64() * 1000.0;
    println!(
        "QA13 elapsed_ms={elapsed_ms:.3} procs={} panes={} pressure={:?} includes=identity,args,cwd,host,tmux,candidate_sockets,cache",
        snapshot.procs.len(),
        snapshot.tmux_panes.len(),
        snapshot.host.as_ref().unwrap().pressure
    );
    assert!(
        elapsed_ms < 50.0,
        "QA13 whole fast round must be <50ms; no best-of filtering"
    );
    assert_eq!(snapshot.system_procs.status, Freshness::Missing);
    let wait_start = Instant::now();
    loop {
        let state = sampler.cache_state();
        if state.status(sysctl::unix_now()).status == Freshness::Fresh {
            assert!(!state.cache.as_ref().unwrap().rows.is_empty());
            assert!(
                state
                    .cache
                    .as_ref()
                    .unwrap()
                    .rows
                    .values()
                    .any(|row| row.uid == 0 && row.footprint_bytes > 0)
            );
            assert!(
                state
                    .cache
                    .as_ref()
                    .unwrap()
                    .rows
                    .values()
                    .any(|row| row.uid == 0 && row.cpu_pct_sampled.is_some())
            );
            assert!(state.cache.as_ref().unwrap().cpu_issue.is_none());
            break;
        }
        assert!(
            wait_start.elapsed() < Duration::from_secs(30),
            "QA13 background cache never became fresh: {:?}",
            state.error
        );
        std::thread::park_timeout(Duration::from_millis(10));
    }
    let filled = sampler.sample().unwrap();
    assert_eq!(filled.system_procs.status, Freshness::Fresh);
    assert!(filled.system_procs.as_of.is_some());
    assert!(
        filled
            .procs
            .iter()
            .any(|proc| proc.uid == 0 && proc.cpu_pct_sampled.is_some())
    );
    assert!(
        filled
            .procs
            .iter()
            .any(|proc| proc.uid == 0 && proc.footprint_bytes.is_some())
    );
    let output = top::capture(&AtomicBool::new(false)).unwrap();
    assert_eq!(output.rows.len(), output.declared_count);
    assert!(output.cpu_issue.is_none());
    assert!(
        output
            .rows
            .values()
            .any(|row| row.cpu_pct_sampled.is_some())
    );
    println!(
        "QA13 top declared={} unique={} system_cache=fresh cpu_source=second_frame isolated_cache=true parent_cache_present={}",
        output.declared_count,
        output.rows.len(),
        path.exists()
    );
}
