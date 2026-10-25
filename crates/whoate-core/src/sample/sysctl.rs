//! Only whitelisted environment values leave the kernel argument buffer.
use super::{SampleError, SampleResult};
use crate::snapshot::{Host, Pressure};

#[derive(Debug, PartialEq)]
pub struct ProcArgs {
    pub exe_path: String,
    pub argv: Vec<String>,
    pub tmux: Option<String>,
    pub tmux_pane: Option<String>,
    /// The Codex session a tool call runs in (phase 10 R7).
    pub codex_thread_id: Option<String>,
}

fn nul_string<'a>(bytes: &'a [u8], cursor: &mut usize) -> SampleResult<&'a [u8]> {
    let end = bytes
        .get(*cursor..)
        .and_then(|tail| tail.iter().position(|b| *b == 0))
        .ok_or_else(|| SampleError::new("procargs", "unterminated_field"))?
        + *cursor;
    let field = &bytes[*cursor..end];
    *cursor = end + 1;
    Ok(field)
}

fn utf8(bytes: &[u8]) -> SampleResult<String> {
    std::str::from_utf8(bytes)
        .map(str::to_owned)
        .map_err(|_| SampleError::new("procargs", "invalid_utf8"))
}

pub fn parse_procargs(bytes: &[u8]) -> SampleResult<ProcArgs> {
    let header: [u8; 4] = bytes
        .get(..4)
        .and_then(|s| s.try_into().ok())
        .ok_or_else(|| SampleError::new("procargs", "short_header"))?;
    let argc = i32::from_ne_bytes(header);
    if argc <= 0 || argc as usize > bytes.len().saturating_sub(4) {
        return Err(SampleError::new("procargs", "invalid_argc"));
    }
    let mut cursor = 4;
    let exe_path = utf8(nul_string(bytes, &mut cursor)?)?;
    while bytes.get(cursor) == Some(&0) {
        cursor += 1;
    }
    // Allocate only after finding real fields, never from an untrusted argc alone.
    let mut argv = Vec::new();
    for _ in 0..argc {
        argv.push(utf8(nul_string(bytes, &mut cursor)?)?);
    }
    let mut result = ProcArgs {
        exe_path,
        argv,
        tmux: None,
        tmux_pane: None,
        codex_thread_id: None,
    };
    while cursor < bytes.len() {
        let field = nul_string(bytes, &mut cursor)?;
        if field.is_empty() {
            continue;
        }
        if let Some(value) = field.strip_prefix(b"TMUX_PANE=") {
            result.tmux_pane = Some(utf8(value)?);
        } else if let Some(value) = field.strip_prefix(b"TMUX=") {
            result.tmux = Some(utf8(value)?);
        } else if let Some(value) = field.strip_prefix(b"CODEX_THREAD_ID=") {
            result.codex_thread_id = Some(utf8(value)?).filter(|id| !id.is_empty());
        }
    }
    Ok(result)
}

pub struct PageCounts {
    pub page_size: u64,
    pub internal: u64,
    pub purgeable: u64,
    pub wired: u64,
    pub compressed: u64,
}

pub fn host_from_pages(
    total: u64,
    pages: PageCounts,
    swap: u64,
    pressure: i32,
) -> SampleResult<Host> {
    let PageCounts {
        page_size,
        internal,
        purgeable,
        wired,
        compressed,
    } = pages;
    if page_size == 0 || purgeable > internal {
        return Err(SampleError::new("host", "invalid_page_statistics"));
    }
    let bytes = |pages: u64| {
        pages
            .checked_mul(page_size)
            .ok_or_else(|| SampleError::new("host", "overflow"))
    };
    let used = (internal - purgeable)
        .checked_add(wired)
        .and_then(|n| n.checked_add(compressed))
        .ok_or_else(|| SampleError::new("host", "overflow"))?;
    let pressure = match pressure {
        1 => Pressure::Normal,
        2 => Pressure::Warn,
        4 => Pressure::Critical,
        _ => return Err(SampleError::new("host", "unknown_pressure")),
    };
    Ok(Host {
        kernel_zones: None,
        gpu_memory: None,
        memory_diagnostics: None,
        cpu: None,
        mem_total_bytes: total,
        mem_used_bytes: bytes(used)?,
        compressed_bytes: bytes(compressed)?,
        wired_bytes: bytes(wired)?,
        swap_used_bytes: swap,
        swap_total_bytes: None,
        pressure,
    })
}

pub fn local_time(unix_seconds: i64) -> SampleResult<time::OffsetDateTime> {
    // localtime_r uses the OS timezone, and is valid even with the slow worker alive.
    let seconds = unix_seconds as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    if unsafe { libc::localtime_r(&seconds, &mut tm) }.is_null() {
        return Err(SampleError::new("clock", "localtime_unavailable"));
    }
    let offset = time::UtcOffset::from_whole_seconds(tm.tm_gmtoff as i32)
        .map_err(|_| SampleError::new("clock", "invalid_timezone"))?;
    time::OffsetDateTime::from_unix_timestamp(unix_seconds)
        .map(|date| date.to_offset(offset))
        .map_err(|_| SampleError::new("clock", "invalid_timestamp"))
}

pub fn timestamp(unix_seconds: i64) -> SampleResult<String> {
    local_time(unix_seconds)?
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| SampleError::new("clock", "format_failed"))
}

pub fn unix_now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

pub fn now_timestamp() -> SampleResult<String> {
    let now = time::OffsetDateTime::now_utc();
    let offset = local_time(now.unix_timestamp())?.offset();
    now.to_offset(offset)
        .format(&time::format_description::well_known::Rfc3339)
        .map_err(|_| SampleError::new("clock", "format_failed"))
}
