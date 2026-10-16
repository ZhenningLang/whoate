use super::{ConfigReader, Recognizer, dev_runtimes::parts};
use crate::{
    model::Owner,
    snapshot::{Proc, Snapshot},
};
use std::{collections::BTreeMap, path::Path};

pub struct WebKitHost;
fn kind(proc: &Proc) -> Option<&'static str> {
    let exe = proc.exe_path.as_deref()?;
    if !exe.starts_with("/System/Library/") {
        return None;
    }
    match Path::new(exe).file_name()?.to_str()? {
        "com.apple.WebKit.WebContent" | "com.apple.WebKit.WebContent.EnhancedSecurity" => {
            Some("web")
        }
        "com.apple.WebKit.GPU" => Some("gpu"),
        "com.apple.WebKit.Networking" => Some("network"),
        _ => None,
    }
}
impl Recognizer for WebKitHost {
    fn id(&self) -> &'static str {
        "webkit-host"
    }
    fn matches(&self, proc: &Proc) -> bool {
        kind(proc).is_some()
    }
    fn explain(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        let mut groups: BTreeMap<String, Vec<&Proc>> = BTreeMap::new();
        let mcp = super::coding_agents::mcp_members(owner, snap);
        for proc in &snap.procs {
            if !owner.pids.contains(&proc.pid) || mcp.contains_key(&proc.pid) {
                continue;
            }
            let responsible_is_member_app = proc.responsible_pid.is_some_and(|pid| {
                owner.pids.contains(&pid)
                    && snap.procs.iter().any(|p| {
                        p.pid == pid
                            && p.exe_path
                                .as_deref()
                                .is_some_and(|s| s.contains(".app/Contents/"))
                    })
            });
            if !responsible_is_member_app {
                continue;
            }
            let label = match kind(proc) {
                Some("web") => cfg.language.choose("网页进程", "Web content"),
                Some("gpu") => cfg.language.choose("WebKit 图形进程", "WebKit GPU"),
                Some("network") => cfg.language.choose("WebKit 网络进程", "WebKit network"),
                _ => continue,
            };
            groups.entry(label.into()).or_default().push(proc);
        }
        parts(owner, groups);
    }
}
