//! Two independent inputs: native Snapshot and explicitly lossy historical command text.
//! Neither input is executed. Public system/app paths and recognition signals survive.
use super::{SampleError, SampleResult, tmux};
use crate::snapshot::{Proc, SampleIssue, Snapshot, TmuxPane};
use serde::Deserialize;
use std::collections::BTreeMap;

#[derive(Deserialize)]
struct Archive {
    taken_at: String,
    procs: Vec<ArchiveProc>,
    tmux_panes: Vec<String>,
}
#[derive(Deserialize)]
struct ArchiveProc {
    pid: i32,
    ppid: i32,
    uid: u32,
    start: String,
    command: String,
    footprint_bytes: Option<u64>,
    cpu_time_ns: Option<u64>,
    tmux_pane: Option<String>,
}

/// Whitespace-delimited archive text is not recoverable argv. Quotes are retained
/// as data, not interpreted as shell escapes. Public -- markers remain exact tokens.
fn archive_argv(command: &str) -> Vec<String> {
    let command = command.trim();
    if command.is_empty() {
        return vec![];
    }
    let mut end = command.find(char::is_whitespace).unwrap_or(command.len());
    // ps has no escaping contract. Recognize a leading bundle path, never an app
    // path in later arguments. Positional arguments are NOT part of the executable.
    let head_end = [command.find(" --"), command.find(" /")]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(command.len());
    let head = &command[..head_end];
    if command.starts_with('/') && head.contains(".app/Contents/") {
        if let Some(marker) = head.find("/Contents/MacOS/") {
            let start = marker + "/Contents/MacOS/".len();
            let basename = &head[start..];
            let bundle_name = head[..marker]
                .rsplit('/')
                .next()
                .and_then(|name| name.strip_suffix(".app"));
            let name_len = PUBLIC_NAMES
                .iter()
                .copied()
                .chain(bundle_name)
                .filter(|name| {
                    basename.strip_prefix(*name).is_some_and(|tail| {
                        tail.is_empty() || tail.starts_with(char::is_whitespace)
                    })
                })
                .map(str::len)
                .max()
                .unwrap_or_else(|| basename.find(char::is_whitespace).unwrap_or(basename.len()));
            end = start + name_len;
        } else if let Some(marker) = head.find(".app/Contents/") {
            let start = marker + ".app/Contents/".len();
            // Ambiguous non-MacOS bundle paths remain explicitly lossy. Keep the
            // app marker but do not absorb unrecognized whitespace-separated data.
            end = start
                + head[start..]
                    .find(char::is_whitespace)
                    .unwrap_or(head.len() - start);
        }
    }
    std::iter::once(command[..end].to_owned())
        .chain(command[end..].split_whitespace().map(str::to_owned))
        .collect()
}

pub fn import_archive(bytes: &[u8]) -> SampleResult<Snapshot> {
    let archive: Archive = serde_json::from_slice(bytes)
        .map_err(|_| SampleError::new("archive", "invalid_json_or_fields"))?;
    let taken = time::OffsetDateTime::parse(
        &archive.taken_at,
        &time::format_description::well_known::Rfc3339,
    )
    .map_err(|_| SampleError::new("archive", "invalid_taken_at"))?;
    let mut procs = Vec::with_capacity(archive.procs.len());
    for raw in archive.procs {
        // ps start has second precision; retain that precision, never add current data.
        let start = archive_start(&raw.start, taken.offset());
        let args = archive_argv(&raw.command);
        let executable = args.first().cloned();
        let comm = executable
            .as_deref()
            .unwrap_or("")
            .rsplit('/')
            .next()
            .unwrap_or("")
            .to_owned();
        let mut proc = Proc::identity(raw.pid, raw.ppid, raw.uid, start, comm);
        proc.exe_path = executable.filter(|exe| exe.starts_with('/'));
        proc.argv = args;
        proc.argv_lossy = true;
        proc.footprint_bytes = raw.footprint_bytes;
        proc.cpu_time_ns = raw.cpu_time_ns;
        proc.tmux_pane = raw.tmux_pane;
        procs.push(proc);
    }
    let mut snap = Snapshot::historical(archive.taken_at, procs);
    for line in archive.tmux_panes {
        // The 19:40 source uses four fields: id, target, window label, pid. No cwd.
        let fields: Vec<_> = line.split('\t').collect();
        if fields.len() != 4 || !tmux::valid_pane(fields[0]) {
            return Err(SampleError::new("archive", "invalid_pane_columns"));
        }
        let pane_pid = fields[3]
            .parse::<i32>()
            .ok()
            .filter(|pid| *pid > 0)
            .ok_or_else(|| SampleError::new("archive", "invalid_pane_pid"))?;
        snap.tmux_panes.push(TmuxPane {
            pane_title: None,
            pane_id: fields[0].into(),
            target: fields[1].into(),
            window_name: Some(fields[2].into()),
            pane_pid,
            cwd: None,
        });
    }
    snap.issues.push(SampleIssue {
        stage: "archive".into(),
        pid: None,
        code: "lossy_argv;second_precision_start;host_cwd_responsible_sockets_uncaptured".into(),
    });
    Ok(snap)
}

fn archive_start(value: &str, offset: time::UtcOffset) -> Option<u64> {
    let fields: Vec<_> = value.split_whitespace().collect();
    if fields.len() != 5 {
        return None;
    }
    let month = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ]
    .iter()
    .position(|month| *month == fields[1])?
        + 1;
    let date = time::Date::from_calendar_date(
        fields[4].parse().ok()?,
        time::Month::try_from(month as u8).ok()?,
        fields[2].parse().ok()?,
    )
    .ok()?;
    let clock = time::Time::parse(
        fields[3],
        &time::macros::format_description!("[hour]:[minute]:[second]"),
    )
    .ok()?;
    let seconds = date.with_time(clock).assume_offset(offset).unix_timestamp();
    u64::try_from(seconds).ok()?.checked_mul(1_000_000)
}

const PUBLIC_NAMES: &[&str] = &[
    "node",
    "bun",
    "deno",
    "python",
    "python3",
    "ruby",
    "java",
    "claude",
    "opencode",
    "opencode.exe",
    "kilo",
    "npm",
    "npx",
    "uv",
    "uvx",
    "sleep",
    "zsh",
    "bash",
    "fish",
    "tmux",
    "launchd",
    "kernel_task",
    "chrome-reading-list-mcp",
    "mcp-server",
    "mcp_server",
    "vite",
    "vite.js",
    "next",
    "next.js",
    "wrangler",
    "wrangler.js",
    "tsserver",
    "tsserver.js",
    "jupyter",
    "kernel",
    "Google Chrome",
    "Google Chrome Helper",
    "Google Chrome Helper (Renderer)",
    "Google Chrome Helper (GPU)",
    "Brave Browser",
    "Brave Browser Helper",
    "Brave Browser Helper (Renderer)",
    "Brave Browser Helper (GPU)",
    "Code",
    "Code Helper",
    "Code Helper (Plugin)",
    "Electron",
    "com.apple.WebKit.WebContent",
    "com.apple.WebKit.GPU",
    "com.apple.WebKit.Networking",
    "com.apple.Virtualization.VirtualMachine",
];

fn safe_name(value: &str) -> String {
    if PUBLIC_NAMES.contains(&value)
        || docker_identifier(value)
        || (value.starts_with("python3.")
            && value[8..].bytes().all(|c| c.is_ascii_digit() || c == b'.'))
        || (value.contains('.') && value.bytes().all(|c| c.is_ascii_digit() || c == b'.'))
    {
        value.into()
    } else {
        "process".into()
    }
}

const APP_STRUCTURE: &[&str] = &[
    "Contents",
    "MacOS",
    "Frameworks",
    "Versions",
    "Current",
    "Helpers",
    "Resources",
    ".venv",
    "venv",
    "node_modules",
    "bin",
    "lib",
    "share",
    "site-packages",
];
const PUBLIC_ARGS: &[&str] = &[
    "-m",
    "-e",
    "http.server",
    "serve",
    "--stdio",
    "mcp_server",
    "chrome-reading-list-mcp",
    "vite",
    "vite.js",
    "next",
    "next.js",
    "wrangler",
    "wrangler.js",
    "tsserver",
    "tsserver.js",
    "jupyter",
    "kernel",
];

fn docker_identifier(value: &str) -> bool {
    value.strip_prefix("com.docker.").is_some_and(|suffix| {
        !suffix.is_empty()
            && suffix
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
    })
}

fn public_argument(value: &str) -> bool {
    PUBLIC_ARGS.contains(&value) || docker_identifier(value)
}

fn safe_comm(value: &str) -> bool {
    !value.is_empty()
        && !value.contains('/')
        && !value.contains('=')
        && !value.chars().any(char::is_control)
}

fn user_root(value: &str) -> Option<String> {
    if value == "~" || value.starts_with("~/") {
        return Some("~".into());
    }
    for prefix in ["/Users/", "/home/"] {
        if let Some(tail) = value.strip_prefix(prefix) {
            let name = tail.split('/').next()?;
            if !name.is_empty() {
                return Some(format!("{prefix}{name}"));
            }
        }
    }
    None
}

fn public_path(value: &str) -> bool {
    // These roots are public system/application locations, not a HOME substitute.
    !value.contains("/Users/")
        && !value.contains("/home/")
        && !value.contains("://")
        && !value.contains('=')
        && !value.chars().any(char::is_control)
        && (value == "/"
            || [
                "/System",
                "/usr",
                "/bin",
                "/sbin",
                "/Library",
                "/Applications",
                "/opt/homebrew",
            ]
            .iter()
            .any(|root| {
                value
                    .strip_prefix(root)
                    .is_some_and(|tail| tail.is_empty() || tail.starts_with('/'))
            }))
}

fn safe_path(value: &str) -> bool {
    if public_path(value) || value == "~" {
        return true;
    }
    let Some(path) = value
        .strip_prefix("~/redacted/")
        .or_else(|| value.strip_prefix("/redacted/"))
    else {
        return false;
    };
    path.split('/').all(|part| {
        part.strip_prefix("path-")
            .is_some_and(|number| !number.is_empty() && number.bytes().all(|c| c.is_ascii_digit()))
            || safe_name(part) != "process"
            || (part.ends_with(".app") && safe_comm(part))
            || APP_STRUCTURE.contains(&part)
    })
}

fn switch_name(value: &str) -> Option<&str> {
    let name = value.split('=').next()?;
    (name.starts_with("--")
        && name.len() > 2
        && name[2..]
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_'))
    .then_some(name)
}

fn public_value(name: &str, value: &str) -> Option<String> {
    match name {
        "--type"
            if [
                "renderer",
                "gpu-process",
                "utility",
                "zygote",
                "browser",
                "extensionHost",
            ]
            .contains(&value) =>
        {
            Some(value.into())
        }
        "--utility-sub-type"
            if [
                "network.mojom.NetworkService",
                "storage.mojom.StorageService",
                "audio.mojom.AudioService",
                "video_capture.mojom.VideoCaptureService",
                "data_decoder.mojom.DataDecoderService",
                "on_device_model.mojom.OnDeviceModelService",
                "tracing.mojom.TracingService",
                "unzip.mojom.Unzipper",
                "proxy_resolver.mojom.ProxyResolverFactory",
                "shape_detection.mojom.ShapeDetectionService",
                "screen_ai.mojom.ScreenAIService",
                "printing.mojom.PrintCompositor",
                "printing.mojom.PrintBackendService",
            ]
            .contains(&value) =>
        {
            Some(value.into())
        }
        _ => None,
    }
}

#[derive(Default)]
pub struct Redactor {
    paths: BTreeMap<String, String>,
}

impl Redactor {
    pub fn path(&mut self, value: &str) -> String {
        self.path_kind(value, true)
    }

    fn directory(&mut self, value: &str) -> String {
        self.path_kind(value, false)
    }

    fn path_kind(&mut self, value: &str, executable: bool) -> String {
        if public_path(value) {
            return value.into();
        }
        if let Some(path) = self.paths.get(value) {
            return path.clone();
        }
        // Map every directory prefix, not only the whole path: cwd and a script
        // inside cwd must still share a directory after redaction.
        let root = user_root(value);
        if root.as_deref() == Some(value) {
            return "~".into();
        }
        let mut original = root.clone().unwrap_or_default();
        let mut mapped = if root.is_some() {
            "~/redacted/path-0"
        } else {
            "/redacted/path-0"
        }
        .to_owned();
        let tail = root.as_ref().map_or(value, |root| &value[root.len()..]);
        let mut components = tail.split('/').filter(|part| !part.is_empty()).peekable();
        while let Some(component) = components.next() {
            original.push('/');
            original.push_str(component);
            if let Some(existing) = self.paths.get(&original) {
                mapped = existing.clone();
                continue;
            }
            let public_app = component.ends_with(".app") && safe_comm(component);
            let structural = APP_STRUCTURE.contains(&component);
            let public_executable =
                executable && components.peek().is_none() && safe_name(component) != "process";
            let name = if public_app
                || structural
                || public_executable
                || component == "chrome-reading-list-mcp"
                || public_argument(component)
            {
                component.to_owned()
            } else {
                format!("path-{}", self.paths.len() + 1)
            };
            mapped.push('/');
            mapped.push_str(&name);
            self.paths.insert(original.clone(), mapped.clone());
        }
        self.paths.insert(value.into(), mapped.clone());
        mapped
    }

    fn argument(&mut self, value: &str) -> String {
        if value.contains("://") || value.chars().any(char::is_control) {
            "<redacted>".into()
        } else if value.contains('/') || value == "~" {
            self.path(value)
        } else if value.contains("chrome-reading-list-mcp") {
            "chrome-reading-list-mcp".into()
        } else if public_argument(value) {
            value.into()
        } else {
            "<redacted>".into()
        }
    }

    pub fn snapshot(&mut self, mut snapshot: Snapshot) -> Snapshot {
        for issue in &mut snapshot.issues {
            issue.stage = "capture".into();
            issue.code = "unavailable_or_source_uncaptured".into();
        }
        for proc in &mut snapshot.procs {
            // Owner ruling: preserve ordinary names from non-user executables.
            // User-directory executables still scrub unknown private names.
            let user_executable = proc.exe_path.as_deref().and_then(user_root).is_some();
            proc.comm = if !user_executable && safe_comm(&proc.comm) {
                proc.comm.clone()
            } else {
                safe_name(&proc.comm)
            };
            if let Some(path) = &mut proc.exe_path {
                *path = self.path(path);
            }
            if let Some(path) = &mut proc.cwd {
                *path = self.directory(path);
            }
            if let Some(pane) = &mut proc.tmux_pane
                && !tmux::valid_pane(pane)
            {
                *pane = "<redacted>".into();
            }
            if let Some(socket) = &mut proc.tmux {
                *socket = "<redacted>".into();
            }
            let mut previous_switch: Option<String> = None;
            for (index, argument) in proc.argv.iter_mut().enumerate() {
                if index == 0 {
                    *argument = if argument.contains('/') {
                        self.path(argument)
                    } else {
                        if safe_comm(argument) {
                            argument.clone()
                        } else {
                            safe_name(argument)
                        }
                    };
                    previous_switch = None;
                } else if let Some(name) = switch_name(argument).map(str::to_owned) {
                    if let Some((_, value)) = argument.split_once('=') {
                        let value =
                            public_value(&name, value).unwrap_or_else(|| self.argument(value));
                        *argument = format!("{name}={value}");
                        previous_switch = None;
                    } else {
                        *argument = name.clone();
                        previous_switch = Some(name);
                    }
                } else {
                    *argument = previous_switch
                        .take()
                        .and_then(|name| public_value(&name, argument))
                        .unwrap_or_else(|| self.argument(argument));
                }
            }
        }
        for pane in &mut snapshot.tmux_panes {
            pane.target = format!("session:{}", pane.pane_id);
            pane.window_name = pane.window_name.as_ref().map(|_| "window".into());
            pane.pane_title = pane.pane_title.as_ref().map(|_| "<redacted>".into());
            if let Some(path) = &mut pane.cwd {
                *path = self.directory(path);
            }
        }
        snapshot
    }
}

pub fn redact(snapshot: Snapshot) -> Snapshot {
    Redactor::default().snapshot(snapshot)
}

pub fn validate_safe(snapshot: &Snapshot) -> SampleResult<()> {
    for timestamp in std::iter::once(&snapshot.taken_at).chain(snapshot.system_procs.as_of.iter()) {
        time::OffsetDateTime::parse(timestamp, &time::format_description::well_known::Rfc3339)
            .map_err(|_| SampleError::new("privacy", "unsafe_timestamp"))?;
    }
    if snapshot
        .issues
        .iter()
        .any(|issue| issue.stage != "capture" || issue.code != "unavailable_or_source_uncaptured")
    {
        return Err(SampleError::new("privacy", "unsafe_issue"));
    }
    let valid_path = safe_path;
    for proc in &snapshot.procs {
        if !safe_comm(&proc.comm) {
            return Err(SampleError::new("privacy", "unsafe_comm"));
        }
        for path in [&proc.exe_path, &proc.cwd].into_iter().flatten() {
            if !valid_path(path) {
                return Err(SampleError::new("privacy", "unsafe_path"));
            }
        }
        let mut previous_switch = None;
        for (i, argument) in proc.argv.iter().enumerate() {
            if argument.contains("://")
                || argument.contains("/Users/")
                || argument.contains("/home/")
            {
                return Err(SampleError::new("privacy", "unsafe_argument"));
            }
            if i == 0 && !(valid_path(argument) || safe_comm(argument)) {
                return Err(SampleError::new("privacy", "unsafe_executable"));
            }
            if i == 0 {
                continue;
            }
            let value_safe =
                |value: &str| value == "<redacted>" || valid_path(value) || public_argument(value);
            if let Some(name) = switch_name(argument) {
                if let Some((_, value)) = argument.split_once('=') {
                    if !value_safe(value) && public_value(name, value).as_deref() != Some(value) {
                        return Err(SampleError::new("privacy", "unsafe_switch_value"));
                    }
                    previous_switch = None;
                } else {
                    previous_switch = Some(name);
                }
            } else {
                let public = previous_switch
                    .take()
                    .and_then(|name| public_value(name, argument));
                if !value_safe(argument) && public.as_deref() != Some(argument.as_str()) {
                    return Err(SampleError::new("privacy", "unsafe_positional_value"));
                }
            }
        }
        if proc
            .tmux
            .as_deref()
            .is_some_and(|value| value != "<redacted>")
            || proc
                .tmux_pane
                .as_deref()
                .is_some_and(|value| !tmux::valid_pane(value) && value != "<redacted>")
        {
            return Err(SampleError::new("privacy", "unsafe_tmux"));
        }
    }
    for pane in &snapshot.tmux_panes {
        if !tmux::valid_pane(&pane.pane_id)
            || pane.target != format!("session:{}", pane.pane_id)
            || pane
                .window_name
                .as_deref()
                .is_some_and(|value| value != "window")
            || pane
                .pane_title
                .as_deref()
                .is_some_and(|value| value != "<redacted>")
            || pane.cwd.as_deref().is_some_and(|path| !valid_path(path))
        {
            return Err(SampleError::new("privacy", "unsafe_pane"));
        }
    }
    Ok(())
}
