use super::{
    ConfigReader, Recognizer,
    dev_runtimes::{fact, parts},
};
use crate::{
    model::{FactValue, Owner},
    snapshot::{Proc, Snapshot},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

pub struct Ide;
fn jetbrains_issue(owner: &mut Owner, cfg: &ConfigReader, code: &str) {
    fact(
        owner,
        "config_issue.jetbrains",
        cfg.language
            .choose("运行版本配置未知", "Running-version configuration unknown"),
        FactValue::Text(code.into()),
    );
}
pub fn parse_xmx(arg: &str) -> Option<u64> {
    let value = arg.strip_prefix("-Xmx")?;
    let (number, multiplier) = match value.as_bytes().last()? {
        b'k' | b'K' => (&value[..value.len() - 1], 1024_u64),
        b'm' | b'M' => (&value[..value.len() - 1], 1024_u64.pow(2)),
        b'g' | b'G' => (&value[..value.len() - 1], 1024_u64.pow(3)),
        _ => (value, 1),
    };
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    number
        .parse::<u64>()
        .ok()?
        .checked_mul(multiplier)
        .filter(|n| *n > 0)
}
fn jetbrains_facts(owner: &mut Owner, proc: &Proc, cfg: &ConfigReader) {
    if let Some(arg) = proc.argv.iter().rev().find(|a| a.starts_with("-Xmx")) {
        if let Some(bytes) = parse_xmx(arg) {
            fact(
                owner,
                "jvm_xmx_bytes",
                cfg.language.choose(
                    "运行参数的堆上限（非已用堆）",
                    "Heap limit from runtime arguments (not heap usage)",
                ),
                FactValue::Count(bytes),
            );
        } else {
            fact(
                owner,
                "config_issue.jvm_args",
                cfg.language.choose("JVM 参数无效", "Invalid JVM argument"),
                FactValue::Text("invalid_xmx".into()),
            );
        }
    }
    let Some((root, _)) = proc
        .exe_path
        .as_deref()
        .and_then(|s| s.split_once(".app/Contents/"))
    else {
        return;
    };
    let meta = match cfg.read_plist(&Path::new(&format!("{root}.app")).join("Contents/Info.plist"))
    {
        Ok(value) => value,
        Err(code) => {
            fact(
                owner,
                "config_issue.jetbrains",
                cfg.language
                    .choose("配置版本未知", "Configuration version unknown"),
                FactValue::Text(code.into()),
            );
            return;
        }
    };
    let Some(dict) = meta.as_dictionary() else {
        jetbrains_issue(owner, cfg, "invalid_app_metadata");
        return;
    };
    let bundle = dict
        .get("CFBundleIdentifier")
        .and_then(plist::Value::as_string);
    let (product, file) = match bundle {
        Some("com.jetbrains.pycharm.ce") => ("PyCharmCE", "pycharm.vmoptions"),
        Some("com.jetbrains.pycharm") => ("PyCharm", "pycharm.vmoptions"),
        Some("com.jetbrains.intellij.ce") => ("IdeaIC", "idea.vmoptions"),
        Some("com.jetbrains.intellij") => ("IntelliJIdea", "idea.vmoptions"),
        _ => {
            jetbrains_issue(owner, cfg, "unknown_jetbrains_product");
            return;
        }
    };
    let explicit = proc
        .argv
        .iter()
        .find_map(|a| a.strip_prefix("-Didea.paths.selector="))
        .or_else(|| {
            dict.get("JVMOptions")?
                .as_dictionary()?
                .get("Properties")?
                .as_dictionary()?
                .get("idea.paths.selector")?
                .as_string()
        });
    let selector = if let Some(selector) = explicit {
        selector.to_owned()
    } else {
        let Some(version) = dict
            .get("CFBundleShortVersionString")
            .and_then(plist::Value::as_string)
        else {
            jetbrains_issue(owner, cfg, "running_version_unavailable");
            return;
        };
        let mut components = version.split('.');
        let Some((major, minor)) = components.next().zip(components.next()) else {
            jetbrains_issue(owner, cfg, "invalid_running_version");
            return;
        };
        if major.is_empty()
            || minor.is_empty()
            || !major
                .bytes()
                .chain(minor.bytes())
                .all(|b| b.is_ascii_digit())
        {
            jetbrains_issue(owner, cfg, "invalid_running_version");
            return;
        }
        format!("{product}{major}.{minor}")
    };
    if selector.is_empty()
        || !selector
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
        || selector == "."
        || selector == ".."
    {
        fact(
            owner,
            "config_issue.jetbrains",
            cfg.language
                .choose("配置版本无效", "Invalid configuration selector"),
            FactValue::Text("invalid_jetbrains_selector".into()),
        );
        return;
    }
    fact(
        owner,
        "jetbrains_selector",
        cfg.language.choose(
            "运行版本的配置目录",
            "Configuration selector of the running version",
        ),
        FactValue::Text(selector.clone()),
    );
    let path = cfg
        .home
        .join("Library/Application Support/JetBrains")
        .join(selector)
        .join(file);
    match cfg.read_text(&path) {
        Ok(text) => {
            if let Some(arg) = text.lines().map(str::trim).rfind(|s| s.starts_with("-Xmx")) {
                if let Some(bytes) = parse_xmx(arg) {
                    fact(
                        owner,
                        "jvm_config_xmx_bytes",
                        cfg.language.choose(
                            "配置的堆上限（非运行实测）",
                            "Configured heap limit (not a runtime measurement)",
                        ),
                        FactValue::Count(bytes),
                    );
                } else {
                    fact(
                        owner,
                        "config_issue.jetbrains",
                        cfg.language.choose("配置无效", "Invalid configuration"),
                        FactValue::Text("invalid_xmx".into()),
                    );
                }
            }
        }
        Err(code) => fact(
            owner,
            "config_issue.jetbrains",
            cfg.language
                .choose("配置不可读", "Unreadable configuration"),
            FactValue::Text(code.into()),
        ),
    }
}
pub fn family(proc: &Proc) -> Option<&'static str> {
    let exe = proc.exe_path.as_deref()?;
    let (root, _) = exe.split_once(".app/Contents/")?;
    match root.rsplit('/').next()? {
        "Visual Studio Code" | "Visual Studio Code - Insiders" => Some("vscode"),
        "PyCharm" | "PyCharm CE" | "IntelliJ IDEA" | "IntelliJ IDEA CE" => Some("jetbrains"),
        _ => None,
    }
}
fn in_vscode_tree(proc: &Proc, owner: &Owner, index: &BTreeMap<i32, &Proc>) -> bool {
    let mut current = proc;
    let mut seen = BTreeSet::new();
    while seen.insert(current.pid) && owner.pids.contains(&current.pid) {
        if family(current) == Some("vscode") {
            return true;
        }
        let Some(parent) = index.get(&current.ppid) else {
            break;
        };
        current = parent;
    }
    false
}
pub(crate) fn extension_host(proc: &Proc) -> bool {
    let has = |s: &str| proc.argv.iter().any(|a| a == s);
    has("--type=extensionHost")
        || (proc
            .exe_path
            .as_deref()
            .is_some_and(|s| s.contains("/Code Helper (Plugin).app/Contents/"))
            && has("--type=utility")
            && has("--utility-sub-type=node.mojom.NodeService")
            && has("--skipWorkspaceStorageLock"))
}
fn vscode_part(proc: &Proc, cfg: &ConfigReader) -> &'static str {
    let has = |s: &str| proc.argv.iter().any(|a| a == s);
    if extension_host(proc) {
        cfg.language.choose("扩展宿主", "Extension hosts")
    } else if has("--type=fileWatcher") {
        cfg.language.choose("文件监视", "File watchers")
    } else if super::registry::runtime_entry(proc)
        .is_some_and(|s| Path::new(s).file_name().is_some_and(|s| s == "tsserver.js"))
    {
        cfg.language
            .choose("TypeScript 语言服务器", "TypeScript language server")
    } else if super::registry::runtime_entry(proc).is_some_and(|s| {
        s.contains("/ms-python.vscode-pylance-") && s.ends_with("/dist/server.bundle.js")
    }) {
        "Pylance"
    } else if proc.exe_path.as_deref().is_some_and(|s| {
        Path::new(s)
            .file_name()
            .is_some_and(|n| n == "rust-analyzer")
    }) {
        "rust-analyzer"
    } else {
        cfg.language.choose("IDE 辅助进程", "IDE helpers")
    }
}

fn vscode_settings(owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
    let mut files = BTreeSet::new();
    for p in snap
        .procs
        .iter()
        .filter(|p| owner.pids.contains(&p.pid) && family(p) == Some("vscode"))
    {
        let insiders = p
            .exe_path
            .as_deref()
            .is_some_and(|s| s.contains("Visual Studio Code - Insiders.app/Contents/"));
        let (namespace, folder) = if insiders {
            ("vscode-insiders", "Code - Insiders")
        } else {
            ("vscode", "Code")
        };
        files.insert((
            namespace,
            "user",
            cfg.home
                .join("Library/Application Support")
                .join(folder)
                .join("User/settings.json"),
        ));
        if let Some(cwd) = p.cwd.as_deref()
            && let Some(path) = cfg.project_file(Path::new(cwd), ".vscode/settings.json")
        {
            files.insert((namespace, "workspace", path));
        }
    }
    for (namespace, scope, path) in files {
        let result = cfg.read_jsonc(&path).and_then(|value| {
            if !value.is_object() {
                return Err("invalid_vscode_settings");
            }
            if let Some(mode) = value.get("python.analysis.languageServerMode") {
                let mode = mode
                    .as_str()
                    .filter(|s| matches!(*s, "light" | "default" | "full"))
                    .ok_or("invalid_vscode_mode")?;
                fact(
                    owner,
                    &format!("{namespace}.{scope}.language_server_mode"),
                    cfg.language
                        .choose("配置的语言服务模式", "Configured language server mode"),
                    FactValue::Text(mode.into()),
                );
            }
            if let Some(excludes) = value.get("files.watcherExclude") {
                let excludes = excludes
                    .as_object()
                    .filter(|v| v.values().all(|v| v.is_boolean()))
                    .ok_or("invalid_vscode_exclusions")?;
                // Expose a count, never private directory globs or arbitrary settings.
                fact(
                    owner,
                    &format!("{namespace}.{scope}.watcher_exclusion_count"),
                    cfg.language
                        .choose("配置的监视排除项数", "Configured watcher exclusion count"),
                    FactValue::Count(
                        excludes
                            .values()
                            .filter(|v| v.as_bool() == Some(true))
                            .count() as u64,
                    ),
                );
            }
            Ok(())
        });
        if let Err(code) = result {
            fact(
                owner,
                &format!("config_issue.{namespace}.{scope}"),
                cfg.language
                    .choose("配置不可读", "Unreadable configuration"),
                FactValue::Text(code.into()),
            );
        }
    }
}
impl Recognizer for Ide {
    fn id(&self) -> &'static str {
        "ide"
    }
    fn matches(&self, proc: &Proc) -> bool {
        family(proc).is_some()
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        vscode_settings(owner, snap, cfg);
        let index = snap.procs.iter().map(|p| (p.pid, p)).collect();
        let mut groups: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        let mcp = super::coding_agents::mcp_members(owner, snap);
        for proc in &snap.procs {
            if mcp.contains_key(&proc.pid) {
                continue;
            }
            if owner.pids.contains(&proc.pid) && in_vscode_tree(proc, owner, &index) {
                groups
                    .entry(vscode_part(proc, cfg).into())
                    .or_default()
                    .push(proc);
            } else if owner.pids.contains(&proc.pid) && family(proc) == Some("jetbrains") {
                jetbrains_facts(owner, proc, cfg);
                groups.entry("JVM".into()).or_default().push(proc);
            }
        }
        parts(owner, groups);
    }
}
