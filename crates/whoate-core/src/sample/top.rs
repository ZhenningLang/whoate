use super::{SampleError, SampleResult, command::read_command};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, sync::atomic::AtomicBool, time::Duration};

pub const TOP_ARGS: &[&str] = &["-l", "2", "-s", "1", "-stats", "pid,mem,cpu", "-n", "10000"];
pub const MAX_PROCESSES: usize = 10_000;

/// Independent top cadence; the enclosing worker still samples kernel memory
/// every six seconds. Cached results retain their original capture timestamp.
pub struct Scheduled<S> {
    source: S,
    started: std::time::Instant,
    previous: Option<(Duration, SampleResult<super::cache::SystemCache>)>,
}
impl<S: super::cache::SlowSource> Scheduled<S> {
    pub fn new(source: S) -> Self {
        Self {
            source,
            started: std::time::Instant::now(),
            previous: None,
        }
    }
    pub fn capture_at(
        &mut self,
        now: Duration,
        stop: &AtomicBool,
    ) -> SampleResult<super::cache::SystemCache> {
        if stop.load(std::sync::atomic::Ordering::Acquire) {
            return Err(SampleError::new("top", "cancelled"));
        }
        if let Some((at, result)) = &self.previous
            && now >= *at
            && now < at.saturating_add(Duration::from_secs(24))
        {
            return result.clone();
        }
        let result = self.source.capture(stop);
        if result.is_ok() && !stop.load(std::sync::atomic::Ordering::Acquire) {
            self.previous = Some((now, result.clone()));
        } else {
            // Errors retain CacheState's stale/as_of semantics, but must retry
            // at the worker's next 6s tick rather than memoizing failure for 24s.
            self.previous = None;
        }
        result
    }
}
impl<S: super::cache::SlowSource> super::cache::SlowSource for Scheduled<S> {
    fn capture(&mut self, stop: &AtomicBool) -> SampleResult<super::cache::SystemCache> {
        self.capture_at(self.started.elapsed(), stop)
    }
    fn capture_memory(&mut self, stop: &AtomicBool) -> Option<super::system_memory::MemoryCache> {
        self.source.capture_memory(stop)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TopRow {
    pub footprint_bytes: u64,
    pub cpu_pct_sampled: Option<f64>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TopSample {
    pub declared_count: usize,
    pub rows: BTreeMap<i32, TopRow>,
    pub cpu_issue: Option<String>,
}

pub fn memory_bytes(text: &str) -> SampleResult<u64> {
    // top appends + or - to indicate movement; it is not part of the value.
    let text = text.trim_end_matches(['+', '-']);
    let split = text
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(text.len());
    let number: f64 = text[..split]
        .parse()
        .map_err(|_| SampleError::new("top", "invalid_memory"))?;
    let multiplier = match &text[split..] {
        "" | "B" => 1_u64,
        "K" => 1 << 10,
        "M" => 1 << 20,
        "G" => 1 << 30,
        "T" => 1 << 40,
        _ => return Err(SampleError::new("top", "invalid_memory_unit")),
    };
    let value = number * multiplier as f64;
    if !value.is_finite() || value < 0.0 || value >= u64::MAX as f64 {
        return Err(SampleError::new("top", "memory_overflow"));
    }
    Ok(value.round() as u64)
}

pub fn parse_top(text: &str) -> SampleResult<TopSample> {
    let starts: Vec<_> = text
        .match_indices("Processes:")
        .filter(|(pos, _)| *pos == 0 || text.as_bytes()[pos - 1] == b'\n')
        .map(|(pos, _)| pos)
        .collect();
    match starts.as_slice() {
        [first, second] => match parse_frame(&text[*second..], true) {
            Ok(sample) => Ok(sample),
            Err(error) => {
                let mut sample = parse_frame(&text[*first..*second], false)?;
                sample.cpu_issue = Some(format!("second_frame_unavailable;{}", error.code));
                Ok(sample)
            }
        },
        [_] => parse_frame(text, false),
        [] => Err(SampleError::new("top", "missing_header")),
        _ => Err(SampleError::new("top", "unexpected_frame_count")),
    }
}

fn parse_frame(text: &str, interval_cpu: bool) -> SampleResult<TopSample> {
    let mut declared = None;
    let mut table = false;
    let mut rows = BTreeMap::new();
    for line in text.lines() {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.is_empty() {
            continue;
        }
        if fields[0] == "Processes:" {
            declared = Some(
                fields
                    .get(1)
                    .and_then(|s| s.parse::<usize>().ok())
                    .ok_or_else(|| SampleError::new("top", "invalid_count"))?,
            );
        } else if matches!(fields.as_slice(), ["PID", "MEM", "%CPU" | "CPU"]) {
            table = true;
        } else if table {
            if fields.len() != 3 {
                return Err(SampleError::new("top", "malformed_row"));
            }
            let pid: i32 = fields[0]
                .parse()
                .map_err(|_| SampleError::new("top", "invalid_pid"))?;
            let cpu: f64 = fields[2]
                .parse()
                .map_err(|_| SampleError::new("top", "invalid_cpu"))?;
            if pid < 0 || !cpu.is_finite() || cpu < 0.0 {
                return Err(SampleError::new("top", "invalid_row"));
            }
            let row = TopRow {
                footprint_bytes: memory_bytes(fields[1])?,
                cpu_pct_sampled: interval_cpu.then_some(cpu),
            };
            if rows.insert(pid, row).is_some() {
                return Err(SampleError::new("top", "duplicate_pid"));
            }
            if rows.len() > MAX_PROCESSES {
                return Err(SampleError::new("top", "process_limit"));
            }
        }
    }
    let declared = declared.ok_or_else(|| SampleError::new("top", "missing_header"))?;
    if declared > MAX_PROCESSES {
        return Err(SampleError::new("top", "process_limit"));
    }
    if rows.is_empty() || rows.len() != declared {
        return Err(SampleError::new("top", "incomplete_result"));
    }
    Ok(TopSample {
        declared_count: declared,
        rows,
        cpu_issue: (!interval_cpu).then(|| "second_frame_missing".into()),
    })
}

pub fn capture(stop: &AtomicBool) -> SampleResult<TopSample> {
    // No result older than the approved 30s freshness window can become fresh.
    // Budget: approved 10,000 rows, 256 bytes per numeric row plus one header row.
    let output = read_command(
        "/usr/bin/top",
        TOP_ARGS,
        stop,
        Duration::from_secs(super::cache::FRESH_SECONDS as u64),
        2 * (MAX_PROCESSES + 1) * 256,
    )?;
    if !output.success {
        return Err(SampleError::new("top", "nonzero_exit"));
    }
    let text =
        std::str::from_utf8(&output.stdout).map_err(|_| SampleError::new("top", "invalid_utf8"))?;
    parse_top(text)
}
