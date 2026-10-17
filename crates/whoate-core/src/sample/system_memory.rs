use super::{SampleError, SampleResult};
use crate::snapshot::{Freshness, Host};
use serde::{Deserialize, Serialize};
use std::{path::Path, sync::atomic::AtomicBool, time::Duration};
pub const MAX_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_ROWS: usize = 8192;
pub const BASIS: &str = "zprint: elem size * current inuse; bytes; not additive to wired";
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KernelZone {
    pub name: String,
    pub bytes: u64,
    pub collectible: Option<bool>,
    pub pageable: Option<bool>,
}
pub fn parse_zones(text: &str) -> SampleResult<Vec<KernelZone>> {
    if text.len() > MAX_BYTES {
        return Err(SampleError::new("zprint", "output_limit"));
    }
    let mut lines = text.lines();
    let upper: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    let lower: Vec<_> = lines.next().unwrap_or("").split_whitespace().collect();
    if upper
        != [
            "elem", "cur", "max", "cur", "max", "cur", "alloc", "alloc", "total",
        ]
        || lower
            != [
                "zone", "name", "size", "size", "size", "#elts", "#elts", "inuse", "size", "count",
                "allocs",
            ]
        || !lines.next().is_some_and(|s| s.starts_with("----"))
    {
        return Err(SampleError::new("zprint", "unsupported_header"));
    }
    let mut zones = vec![];
    for line in lines {
        if line.starts_with("----") || line == "ZONE TOTALS" {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        if zones.len() >= MAX_ROWS {
            return Err(SampleError::new("zprint", "row_limit"));
        }
        let cols: Vec<_> = line.split_whitespace().collect();
        if !(10..=13).contains(&cols.len()) {
            return Err(SampleError::new("zprint", "invalid_columns"));
        }
        let size = cols[1]
            .parse::<u64>()
            .map_err(|_| SampleError::new("zprint", "invalid_element_size"))?;
        let count = cols[6]
            .parse::<u64>()
            .map_err(|_| SampleError::new("zprint", "invalid_inuse"))?;
        let bytes = size
            .checked_mul(count)
            .ok_or_else(|| SampleError::new("zprint", "resource_overflow"))?;
        let flags = cols[9..cols.len() - 1].join("");
        let known = flags.chars().all(|c| matches!(c, 'P' | 'C'));
        zones.push(KernelZone {
            name: cols[0].into(),
            bytes,
            collectible: known.then_some(flags.contains('C')),
            pageable: known.then_some(flags.contains('P')),
        });
    }
    if zones.is_empty() {
        return Err(SampleError::new("zprint", "empty_zones"));
    }
    zones.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.name.cmp(&b.name)));
    zones.truncate(5);
    Ok(zones)
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KernelZones {
    pub status: Freshness,
    pub as_of: Option<String>,
    pub basis: String,
    pub top: Vec<KernelZone>,
    pub reason: Option<String>,
}
impl KernelZones {
    pub fn missing(reason: &str) -> Self {
        Self {
            status: Freshness::Missing,
            as_of: None,
            basis: BASIS.into(),
            top: vec![],
            reason: Some(reason.into()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GpuMemory {
    pub status: Freshness,
    pub as_of: Option<String>,
    pub bytes: Option<u64>,
    pub source: Option<String>,
    pub reason: Option<String>,
}
impl GpuMemory {
    pub fn missing() -> Self {
        Self {
            status: Freshness::Missing,
            as_of: None,
            bytes: None,
            source: None,
            reason: Some("ioreg_memory_keys_have_unverified_units_and_overlap".into()),
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MemoryCache {
    pub kernel_zones: KernelZones,
    pub gpu_memory: GpuMemory,
}
impl MemoryCache {
    pub fn missing(reason: &str) -> Self {
        Self {
            kernel_zones: KernelZones::missing(reason),
            gpu_memory: GpuMemory::missing(),
        }
    }
    pub fn load(path: &Path) -> Self {
        use std::io::Read;
        match std::fs::File::open(path) {
            Ok(file) => {
                let mut bytes = vec![];
                if file
                    .take((MAX_BYTES + 1) as u64)
                    .read_to_end(&mut bytes)
                    .is_err()
                    || bytes.len() > MAX_BYTES
                {
                    return Self::missing("invalid_memory_cache");
                }
                let cache: Self = match serde_json::from_slice(&bytes) {
                    Ok(cache) => cache,
                    Err(_) => return Self::missing("invalid_memory_cache"),
                };
                let zones = &cache.kernel_zones;
                let valid_zones = zones.basis == BASIS
                    && zones.top.len() <= 5
                    && (zones.status == Freshness::Missing
                        || !zones.top.is_empty()
                            && zones.as_of.as_deref().is_some_and(|s| {
                                time::OffsetDateTime::parse(
                                    s,
                                    &time::format_description::well_known::Rfc3339,
                                )
                                .is_ok()
                            }));
                // No available GPU source has a proven unit/overlap contract.
                if !valid_zones
                    || cache.gpu_memory.status != Freshness::Missing
                    || cache.gpu_memory.bytes.is_some()
                {
                    return Self::missing("invalid_memory_cache");
                }
                cache
            }
            Err(e) => Self::missing(if e.kind() == std::io::ErrorKind::NotFound {
                "no_memory_cache"
            } else {
                "memory_cache_read_failed"
            }),
        }
    }
    pub fn at(&self, now: i64) -> Self {
        let mut view = self.clone();
        if view.kernel_zones.status != Freshness::Missing {
            let age = view
                .kernel_zones
                .as_of
                .as_deref()
                .and_then(|s| {
                    time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339)
                        .ok()
                })
                .and_then(|s| now.checked_sub(s.unix_timestamp()));
            if !age.is_some_and(|n| (0..=super::cache::FRESH_SECONDS).contains(&n)) {
                view.kernel_zones.status = Freshness::Stale;
            }
        }
        view
    }
    pub fn merge(&self, host: &mut Host, now: i64) {
        let view = self.at(now);
        host.kernel_zones = Some(view.kernel_zones);
        host.gpu_memory = Some(view.gpu_memory);
    }
    pub fn update(&mut self, mut next: Self, path: &Path) {
        if next.kernel_zones.status == Freshness::Missing && !self.kernel_zones.top.is_empty() {
            let reason = next.kernel_zones.reason.take();
            next.kernel_zones = self.kernel_zones.clone();
            next.kernel_zones.status = Freshness::Stale;
            next.kernel_zones.reason = reason;
        }
        if super::cache::write_json_atomic(path, &next).is_err() {
            next.kernel_zones.status = if next.kernel_zones.top.is_empty() {
                Freshness::Missing
            } else {
                Freshness::Stale
            };
            next.kernel_zones.reason = Some("memory_cache_write_failed".into());
        }
        *self = next;
    }
}
pub fn capture(stop: &AtomicBool) -> MemoryCache {
    // ceiling: 2 MiB/8192 rows and two seconds bound private tool output;
    // upgrade the parser/limit only when a supported OS exceeds this envelope.
    let result = super::command::read_command(
        "/usr/bin/zprint",
        &["-t"],
        stop,
        Duration::from_secs(2),
        MAX_BYTES,
    )
    .and_then(|output| {
        if !output.success {
            return Err(SampleError::new("zprint", "nonzero_exit"));
        }
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| SampleError::new("zprint", "invalid_utf8"))?;
        parse_zones(text)
    });
    let zones = match result {
        Ok(top) => KernelZones {
            status: Freshness::Fresh,
            as_of: super::sysctl::now_timestamp().ok(),
            basis: BASIS.into(),
            top,
            reason: None,
        },
        Err(e) => KernelZones::missing(&format!("{}:{}", e.stage, e.code)),
    };
    MemoryCache {
        kernel_zones: zones,
        gpu_memory: GpuMemory::missing(),
    }
}
