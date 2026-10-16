use super::{
    ConfigReader, Recognizer,
    dev_runtimes::{fact, parts},
};
use crate::{
    model::{FactValue, Owner},
    snapshot::{Proc, Snapshot},
};
use std::collections::BTreeMap;

pub struct Chromium;

pub fn family(proc: &Proc) -> Option<&'static str> {
    let path = proc
        .exe_path
        .as_deref()
        .or_else(|| proc.argv.first().map(String::as_str))?;
    let (app, _) = path.split_once(".app/Contents/")?;
    match app.rsplit('/').next()? {
        "Google Chrome" => Some("chrome"),
        "Brave Browser" => Some("brave"),
        _ => None,
    }
}
pub(crate) fn configure(owner: &mut Owner, snap: &Snapshot, cfg: &mut ConfigReader) {
    for (family_name, relative) in [
        ("chrome", "Google/Chrome"),
        ("brave", "BraveSoftware/Brave-Browser"),
    ] {
        let key = format!("{family_name}.memory_saver_state");
        cfg.values.insert(key.clone(), serde_json::Value::Null);
        if !snap
            .procs
            .iter()
            .any(|p| owner.pids.contains(&p.pid) && family(p) == Some(family_name))
        {
            continue;
        }
        fact(
            owner,
            "browser_policy_unknown",
            cfg.language.choose(
                "用户偏好，企业策略可能覆盖",
                "User preference; enterprise policy may override",
            ),
            FactValue::Bool(true),
        );
        let path = cfg
            .home
            .join("Library/Application Support")
            .join(relative)
            .join("Local State");
        let state = cfg.read_json(&path).and_then(|value| {
            if !value.is_object() {
                return Err("invalid_browser_config");
            }
            let full_key = "performance_tuning.high_efficiency_mode.state";
            let value = value
                .get(full_key)
                .or_else(|| value.pointer("/performance_tuning/high_efficiency_mode/state"));
            match value {
                None => Ok(0),
                Some(v) => v
                    .as_u64()
                    .filter(|n| *n <= 2)
                    .ok_or("invalid_browser_state"),
            }
        });
        match state {
            Ok(state) => {
                cfg.values.insert(key.clone(), state.into());
                fact(
                    owner,
                    &key,
                    cfg.language
                        .choose("省内存模式用户偏好", "Memory Saver user preference"),
                    FactValue::Count(state),
                );
            }
            Err(code) => fact(
                owner,
                &format!("config_issue.{family_name}"),
                cfg.language
                    .choose("配置不可读", "Unreadable configuration"),
                FactValue::Text(code.into()),
            ),
        }
    }
}
fn classify(proc: &Proc, cfg: &ConfigReader) -> &'static str {
    let has = |s: &str| proc.argv.iter().any(|a| a == s);
    if has("--extension-process") {
        cfg.language.choose("扩展进程", "Extensions")
    } else if has("--type=renderer") {
        cfg.language.choose("标签页进程", "Tab processes")
    } else if has("--type=gpu-process") {
        "GPU"
    } else if has("--type=utility") {
        if has("--utility-sub-type=network.mojom.NetworkService") {
            cfg.language.choose("网络服务", "Network service")
        } else if has("--utility-sub-type=storage.mojom.StorageService") {
            cfg.language.choose("存储服务", "Storage service")
        } else if has("--utility-sub-type=audio.mojom.AudioService") {
            cfg.language.choose("音频服务", "Audio service")
        } else if has("--utility-sub-type=brave_rewards.mojom.RewardsEngineFactory") {
            cfg.language
                .choose("Brave 奖励服务", "Brave rewards service")
        } else {
            cfg.language.choose("工具服务", "Utility service")
        }
    } else if proc.argv.iter().any(|a| a.starts_with("--type=")) {
        cfg.language.choose("浏览器辅助进程", "Browser helper")
    } else {
        cfg.language.choose("浏览器", "Browser")
    }
}
impl Recognizer for Chromium {
    fn id(&self) -> &'static str {
        "chromium"
    }
    fn matches(&self, proc: &Proc) -> bool {
        family(proc).is_some()
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        let mut groups: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        let mcp = super::coding_agents::mcp_members(owner, snap);
        for proc in &snap.procs {
            if owner.pids.contains(&proc.pid) && !mcp.contains_key(&proc.pid) && self.matches(proc)
            {
                groups
                    .entry(classify(proc, cfg).into())
                    .or_default()
                    .push(proc);
            }
        }
        parts(owner, groups);
    }
}
