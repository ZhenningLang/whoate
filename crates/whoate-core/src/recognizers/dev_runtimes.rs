use super::{ConfigReader, Recognizer};
use crate::{
    attribute::is_runtime,
    model::{Fact, FactValue, Owner, Part},
    snapshot::{Proc, Snapshot},
};
use std::{
    collections::BTreeMap,
    path::{Component, Path, PathBuf},
};

pub struct DevRuntimes;

fn venv_entry(entry: &str, cwd: Option<&str>, cfg: &ConfigReader) -> Option<PathBuf> {
    let path = Path::new(entry);
    let relative = !path.is_absolute();
    // A bare activated name has no unique venv signal in the approved Snapshot.
    if path.components().count() < 2 {
        return None;
    }
    let name = path.file_name()?.to_str()?;
    let valid_python = matches!(name, "python" | "python3")
        || name
            .strip_prefix("python3.")
            .is_some_and(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit() || b == b'.'));
    if !valid_python || path.parent()?.file_name()? != "bin" {
        return None;
    }
    let path = if path.is_absolute() {
        path.to_owned()
    } else {
        Path::new(cwd?).join(path)
    };
    let root = path.parent()?.parent()?;
    let mut normalized = PathBuf::new();
    for component in root.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    if let Ok(canonical) = normalized.canonicalize() {
        if !cfg.allows(&normalized) {
            return None;
        }
        normalized = cfg
            .home
            .canonicalize()
            .ok()
            .and_then(|home| canonical.strip_prefix(home).ok().map(|p| cfg.home.join(p)))
            .unwrap_or(canonical);
    } else if relative || !normalized.starts_with(&cfg.home) {
        return None;
    }
    let named = normalized
        .file_name()
        .is_some_and(|name| name == ".venv" || name == "venv");
    (named || (cfg.allows(&normalized) && normalized.join("pyvenv.cfg").is_file()))
        .then_some(normalized)
}

pub fn script(proc: &Proc) -> Option<&'static str> {
    for arg in proc.argv.iter().skip(1) {
        let name = Path::new(arg)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or(arg);
        for (candidate, result) in [
            ("vite", "vite"),
            ("next", "next"),
            ("wrangler", "wrangler"),
            ("tsserver", "tsserver"),
            ("jupyter", "jupyter kernel"),
            ("ipykernel_launcher", "jupyter kernel"),
        ] {
            if name == candidate
                || name.strip_suffix(".js") == Some(candidate)
                || name.strip_suffix(".py") == Some(candidate)
            {
                return Some(result);
            }
        }
    }
    None
}
pub(crate) fn fact(owner: &mut Owner, key: &str, label: &str, value: FactValue) {
    let f = Fact {
        key: key.into(),
        label: label.into(),
        value,
    };
    if !owner.facts.contains(&f) {
        owner.facts.push(f);
    }
}
pub(crate) fn parts(owner: &mut Owner, groups: BTreeMap<String, Vec<&Proc>>) {
    let mut claimed: std::collections::BTreeSet<_> = owner
        .parts
        .iter()
        .flat_map(|p| p.pids.iter().copied())
        .collect();
    for (name, procs) in groups {
        let procs: Vec<_> = procs
            .into_iter()
            .filter(|p| claimed.insert(p.pid))
            .collect();
        if procs.is_empty() {
            continue;
        }
        let mut pids: Vec<_> = procs.iter().map(|p| p.pid).collect();
        pids.sort_unstable();
        pids.dedup();
        let part = Part {
            name,
            mem_bytes: procs.iter().filter_map(|p| p.footprint_bytes).sum(),
            process_count: pids.len(),
            pids,
            unknown_mem_count: procs.iter().filter(|p| p.footprint_bytes.is_none()).count(),
        };
        if !owner.parts.contains(&part) {
            owner.parts.push(part);
        }
    }
}
impl Recognizer for DevRuntimes {
    fn id(&self) -> &'static str {
        "dev-runtimes"
    }
    fn matches(&self, proc: &Proc) -> bool {
        is_runtime(proc)
            || (super::ide::family(proc) == Some("vscode")
                && super::registry::runtime_family(proc).is_some())
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        let mut groups: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        for proc in &snap.procs {
            if !owner.pids.contains(&proc.pid) || !self.matches(proc) {
                continue;
            }
            let name = script(proc).unwrap_or(&proc.comm);
            groups.entry(name.into()).or_default().push(proc);
            if let Some(script) = script(proc) {
                fact(
                    owner,
                    "script",
                    cfg.language.choose("脚本", "Script"),
                    FactValue::Text(script.into()),
                );
            }
            if let Some(cwd) = proc.cwd.as_deref() {
                if let Some(path) = cfg.project_file(Path::new(cwd), "package.json") {
                    match cfg.read_json(&path) {
                        Ok(v) => {
                            if let Some(name) = v["name"].as_str() {
                                fact(
                                    owner,
                                    "project",
                                    cfg.language.choose("项目", "Project"),
                                    FactValue::Text(name.into()),
                                );
                            }
                        }
                        Err(code) => fact(
                            owner,
                            "config_issue",
                            cfg.language
                                .choose("配置不可读", "Unreadable configuration"),
                            FactValue::Text(code.into()),
                        ),
                    }
                }
                if let Some(path) = cfg.project_file(Path::new(cwd), "pyproject.toml") {
                    match std::fs::read_to_string(path)
                        .ok()
                        .and_then(|s| toml::from_str::<toml::Value>(&s).ok())
                    {
                        Some(v) => {
                            if let Some(name) = v
                                .get("project")
                                .and_then(|v| v.get("name"))
                                .and_then(toml::Value::as_str)
                            {
                                fact(
                                    owner,
                                    "project",
                                    cfg.language.choose("项目", "Project"),
                                    FactValue::Text(name.into()),
                                );
                            }
                        }
                        None => fact(
                            owner,
                            "config_issue",
                            cfg.language
                                .choose("配置不可读", "Unreadable configuration"),
                            FactValue::Text("invalid_pyproject".into()),
                        ),
                    }
                }
            }
            for exe in proc
                .exe_path
                .as_deref()
                .into_iter()
                .chain(proc.argv.first().map(String::as_str))
            {
                if let Some(venv) = venv_entry(exe, proc.cwd.as_deref(), cfg) {
                    fact(
                        owner,
                        "venv",
                        cfg.language.choose("虚拟环境", "Virtual environment"),
                        FactValue::Text(venv.to_string_lossy().into()),
                    );
                }
            }
            if let Some(ports) = &proc.listen_ports {
                fact(
                    owner,
                    "listen_ports",
                    cfg.language.choose("监听端口", "Listening ports"),
                    FactValue::Strings(ports.iter().map(u16::to_string).collect()),
                );
            }
        }
        parts(owner, groups);
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        model::*,
        recognizers::{ConfigReader, Recognizer},
        snapshot::*,
        sources::testing::Temp,
    };
    #[test]
    fn phase02_runtimes_project_script_venv_and_ports() {
        let temp = Temp::new();
        temp.write("project/package.json", r#"{"name":"test-project"}"#);
        let mut node = Proc::identity(999001, 1, 501, Some(1), "node".into());
        node.cwd = Some(temp.0.join("project/sub").to_string_lossy().into());
        node.argv = vec!["node".into(), "node_modules/vite/bin/vite.js".into()];
        node.footprint_bytes = Some(100);
        node.listen_ports = Some(vec![5173]);
        let mut owner = Owner::new("orphan:999001".into(), OwnerKind::Orphan, "node".into());
        owner.pids = vec![999001];
        owner.mem_bytes = 100;
        let snap = Snapshot::historical("2026-10-14T19:40:00+08:00".into(), vec![node]);
        super::DevRuntimes.explain(&mut owner, &snap, &ConfigReader::isolated(temp.0.clone()));
        assert!(
            owner
                .facts
                .iter()
                .any(|f| f.key == "project" && f.value == FactValue::Text("test-project".into()))
        );
        assert!(
            owner
                .facts
                .iter()
                .any(|f| f.key == "script" && f.value == FactValue::Text("vite".into()))
        );
        assert_eq!(owner.parts[0].mem_bytes, 100);
        assert_eq!(owner.parts[0].process_count, 1);
        assert_eq!(owner.insights, Vec::<String>::new());
    }
}
