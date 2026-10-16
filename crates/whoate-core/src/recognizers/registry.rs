use super::ConfigReader;
use super::dev_runtimes::fact;
use super::{
    Recognizer, chromium::Chromium, coding_agents::CodingAgents, dev_runtimes::DevRuntimes,
    ide::Ide, webkit_host::WebKitHost,
};
use crate::{
    model::{FactValue, Owner},
    snapshot::{Proc, Snapshot},
};
use std::{collections::BTreeMap, path::Path};

/// Only executable/script entry positions count, never arbitrary argument text.
fn runtime_execution_role(proc: &Proc) -> bool {
    crate::attribute::is_runtime(proc)
        || (super::ide::family(proc) == Some("vscode")
            && proc.exe_path.as_deref().is_some_and(|exe| {
                exe.ends_with("/Code Helper (Plugin).app/Contents/MacOS/Code Helper (Plugin)")
            }))
}

pub(crate) fn runtime_entry(proc: &Proc) -> Option<&str> {
    if !runtime_execution_role(proc) {
        return None;
    }
    let mut args = proc.argv.iter().skip(1);
    while let Some(arg) = args.next() {
        if let Some(value) = arg.strip_prefix("--max-old-space-size=") {
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            continue;
        }
        if arg == "--" {
            return args.next().map(String::as_str);
        }
        if arg.starts_with('-') {
            return None;
        }
        return Some(arg);
    }
    None
}

pub fn runtime_family(proc: &Proc) -> Option<&'static str> {
    if !runtime_execution_role(proc) {
        return None;
    }
    let title = proc.argv.first().map(String::as_str).unwrap_or(&proc.comm);
    if title
        .strip_prefix("next-server (v")
        .and_then(|v| v.strip_suffix(')'))
        .is_some_and(|v| {
            !v.is_empty()
                && v.bytes()
                    .all(|b| b.is_ascii_digit() || matches!(b, b'.' | b'-' | b'a'..=b'z'))
        })
    {
        return Some("next");
    }
    let entry = runtime_entry(proc)?;
    let name = Path::new(entry).file_name()?.to_str()?;
    let name = name.strip_suffix(".js").unwrap_or(name);
    match name {
        "vite" => Some("vite"),
        "next" => Some("next"),
        "webpack" | "webpack-dev-server" => Some("webpack"),
        "wrangler" if proc.argv.iter().skip(2).any(|a| a == "dev") => Some("wrangler"),
        "tsserver" => Some("tsserver"),
        _ => None,
    }
}

/// A per-owner value view prevents one product's preferences leaking to another.
pub fn prepare(owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) -> ConfigReader {
    let mut view = cfg.clone();
    let mut counts: BTreeMap<&str, u64> = crate::insights::registry::COUNT_SIGNALS
        .iter()
        .copied()
        .filter(|s| !matches!(*s, "process_count" | "mcp_count"))
        .map(|s| (s, 0))
        .collect();
    for proc in snap.procs.iter().filter(|p| owner.pids.contains(&p.pid)) {
        let mut add = |key: &str| {
            *counts.get_mut(key).expect("registered count producer") += 1;
        };
        if let Some(tool) = crate::sources::tool(proc) {
            match tool {
                "claude" => add("software_claude"),
                "opencode" => add("software_opencode"),
                _ => {}
            }
        }
        if let Some(family) = runtime_family(proc) {
            add(&format!("software_{family}"));
        }
        let executable = proc
            .exe_path
            .as_deref()
            .and_then(|p| Path::new(p).file_name()?.to_str())
            .unwrap_or(&proc.comm);
        if executable == "python"
            || executable == "python3"
            || executable.strip_prefix("python3.").is_some_and(|s| {
                !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.')
            })
        {
            add("software_python");
            if proc.argv.get(1).is_some_and(|s| s == "-m")
                && proc.argv.get(2).is_some_and(|s| s == "ipykernel_launcher")
            {
                add("python_kernel");
            }
        }
        if let Some(family) = super::chromium::family(proc) {
            add(&format!("software_{family}"));
            if proc.argv.iter().any(|s| s == "--extension-process") {
                add(&format!("{family}_extensions"));
            }
        }
        if let Some(family) = super::ide::family(proc) {
            add(&format!("software_{family}"));
            if family == "vscode" {
                if super::ide::extension_host(proc) {
                    add("vscode_extension_hosts");
                }
                if proc.argv.iter().any(|s| s == "--type=fileWatcher") {
                    add("vscode_watchers");
                }
            }
        }
        if super::docker::is_docker(proc) {
            add("software_docker");
        }
    }
    for (key, count) in counts {
        fact(
            owner,
            &format!("count.{key}"),
            cfg.language
                .choose("登记的软件信号", "Registered software signal"),
            FactValue::Count(count),
        );
    }
    for key in crate::insights::registry::CONFIG_SIGNALS {
        view.values.insert((*key).into(), serde_json::Value::Null);
    }
    super::chromium::configure(owner, snap, &mut view);
    super::docker::configure(owner, snap, &mut view);
    view
}

/// Registered adapter counts are temporary matcher inputs, not display facts.
/// Keep all leaves available until every matched recognizer has attached KB IDs.
pub fn finish(owner: &mut Owner) {
    owner.facts.retain(|f| {
        !f.key
            .strip_prefix("count.")
            .is_some_and(|signal| crate::insights::registry::COUNT_SIGNALS.contains(&signal))
    });
}

pub fn builtins() -> Vec<Box<dyn Recognizer>> {
    vec![
        Box::new(Chromium),
        Box::new(WebKitHost),
        Box::new(Ide),
        Box::new(super::docker::Docker),
        Box::new(CodingAgents),
        Box::new(DevRuntimes),
    ]
}
