//! Whole-machine CPU: usage from two HOST_CPU_LOAD_INFO readings, plus the
//! 1-minute load average and logical core count. A missing difference is
//! reported as missing with a reason, never as 0%.
use crate::snapshot::Freshness;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HostCpu {
    /// fresh or missing; never stale (there is no cached value).
    pub status: Freshness,
    pub usage_pct: Option<f64>,
    pub load_1m: Option<f64>,
    pub logical_cores: Option<u32>,
    pub reason: Option<String>,
}

/// Cumulative ticks since boot (CPU_STATE_USER, SYSTEM, IDLE, NICE).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CpuTicks {
    pub user: u64,
    pub system: u64,
    pub idle: u64,
    pub nice: u64,
}
impl CpuTicks {
    fn total(&self) -> Option<u64> {
        self.user
            .checked_add(self.system)?
            .checked_add(self.idle)?
            .checked_add(self.nice)
    }
}

pub trait CpuSource {
    fn ticks(&mut self) -> Result<CpuTicks, &'static str>;
    fn load_1m(&mut self) -> Option<f64>;
    fn logical_cores(&mut self) -> Option<u32>;
}

/// Keeps the previous reading; each `sample` is the difference since the last one.
#[derive(Default)]
pub struct CpuMeter<S = PlatformCpu> {
    source: S,
    previous: Option<CpuTicks>,
}
impl<S: CpuSource> CpuMeter<S> {
    pub const fn new(source: S) -> Self {
        Self {
            source,
            previous: None,
        }
    }
    /// The next sample starts over (first_sample) instead of averaging a pause.
    pub fn reset(&mut self) {
        self.previous = None;
    }
    pub fn sample(&mut self) -> HostCpu {
        let load_1m = self.source.load_1m();
        let logical_cores = self.source.logical_cores();
        let (usage_pct, reason) = match self.source.ticks() {
            Err(code) => {
                self.previous = None;
                (None, Some(code.to_string()))
            }
            Ok(current) => match self.previous.replace(current) {
                None => (None, Some("first_sample".into())),
                Some(previous) => match usage(previous, current) {
                    Ok(pct) => (Some(pct), None),
                    Err(code) => (None, Some(code.into())),
                },
            },
        };
        HostCpu {
            status: if usage_pct.is_some() {
                Freshness::Fresh
            } else {
                Freshness::Missing
            },
            usage_pct,
            load_1m,
            logical_cores,
            reason,
        }
    }
}

/// Busy share of all ticks between two readings, 0–100.
pub fn usage(previous: CpuTicks, current: CpuTicks) -> Result<f64, &'static str> {
    let fields = [
        (previous.user, current.user),
        (previous.system, current.system),
        (previous.idle, current.idle),
        (previous.nice, current.nice),
    ];
    if fields.iter().any(|(old, new)| new < old) {
        return Err("counter_reset");
    }
    let (Some(old), Some(new)) = (previous.total(), current.total()) else {
        return Err("counter_overflow");
    };
    let total = new - old;
    if total == 0 {
        return Err("no_elapsed_ticks");
    }
    let idle = current.idle - previous.idle;
    Ok((total - idle) as f64 / total as f64 * 100.0)
}

unsafe extern "C" {
    fn capture_cpu_ticks(out: *mut u64) -> i32;
}

#[derive(Default)]
pub struct PlatformCpu;
impl CpuSource for PlatformCpu {
    fn ticks(&mut self) -> Result<CpuTicks, &'static str> {
        let mut t = [0_u64; 4];
        if unsafe { capture_cpu_ticks(t.as_mut_ptr()) } != 0 {
            return Err("host_statistics_failed");
        }
        Ok(CpuTicks {
            user: t[0],
            system: t[1],
            idle: t[2],
            nice: t[3],
        })
    }
    fn load_1m(&mut self) -> Option<f64> {
        let mut load = [0.0_f64; 1];
        let n = unsafe { libc::getloadavg(load.as_mut_ptr(), 1) };
        (n == 1 && load[0].is_finite() && load[0] >= 0.0).then_some(load[0])
    }
    fn logical_cores(&mut self) -> Option<u32> {
        let mut value = 0_i32;
        let mut size = std::mem::size_of::<i32>();
        let result = unsafe {
            libc::sysctlbyname(
                c"hw.logicalcpu".as_ptr(),
                (&mut value as *mut i32).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        (result == 0 && size == std::mem::size_of::<i32>() && value > 0).then_some(value as u32)
    }
}
