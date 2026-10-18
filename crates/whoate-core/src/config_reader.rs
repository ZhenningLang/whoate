//! Read-only filesystem boundary. Tests inject paths instead of changing HOME.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
#[path = "config_reader/cache.rs"]
mod cache;
#[path = "config_reader/jsonc.rs"]
mod jsonc;

#[derive(Clone, Debug)]
pub struct ConfigReader {
    pub language: crate::verdict::Language,
    pub home: PathBuf,
    pub cache: PathBuf,
    pub rules: PathBuf,
    /// Registered software configuration signals supplied by read-only adapters.
    pub values: BTreeMap<String, Value>,
    isolated: bool,
    rules_explicit: bool,
    // Optional validation capability: one Claude PID, first pair only. Private
    // and immutable after construction so clones cannot widen the read scope.
    session_pid: Option<i32>,
    files: Arc<Mutex<cache::Files>>,
    docker: Arc<super::docker::Monitor>,
}

impl ConfigReader {
    pub fn from_env() -> Result<Self, &'static str> {
        Self::from_lookup(|key| std::env::var_os(key))
    }
    pub fn from_lookup(
        mut env: impl FnMut(&str) -> Option<std::ffi::OsString>,
    ) -> Result<Self, &'static str> {
        let override_home = env("WHOATE_HOME_OVERRIDE");
        let isolated = override_home.is_some();
        let home = override_home
            .or_else(|| env("HOME"))
            .ok_or("home_unavailable")?;
        let home = PathBuf::from(home);
        let cache = env("WHOATE_CACHE_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("Library/Caches/whoate"));
        let rules_override = env("WHOATE_RULES");
        let rules_explicit = rules_override.is_some();
        let rules = rules_override
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config/whoate/rules.toml"));
        let session_pid = env("WHOATE_SESSION_PID")
            .map(|raw| {
                raw.to_str()
                    .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|s| s.parse::<i32>().ok())
                    .filter(|pid| *pid > 0)
                    .ok_or("invalid_session_read_limit")
            })
            .transpose()?;
        Ok(Self {
            language: crate::verdict::Language::En,
            docker: Arc::new(super::docker::Monitor::native(home.clone())),
            home,
            cache,
            rules,
            values: BTreeMap::new(),
            isolated,
            rules_explicit,
            session_pid,
            files: Arc::default(),
        })
    }
    pub fn isolated(home: PathBuf) -> Self {
        Self {
            language: crate::verdict::Language::En,
            cache: home.join("cache"),
            rules: home.join("rules.toml"),
            docker: Arc::new(super::docker::Monitor::native(home.clone())),
            home,
            values: BTreeMap::new(),
            isolated: true,
            rules_explicit: false,
            session_pid: None,
            files: Arc::default(),
        }
    }
    /// WHOATE_SESSION_PID only narrows source reads; absence preserves normal
    /// product behavior. It never filters process/resource sampling itself.
    pub fn session_pid(&self) -> Option<i32> {
        self.session_pid
    }
    pub fn allows(&self, path: &Path) -> bool {
        !self.isolated
            || path
                .canonicalize()
                .ok()
                .zip(self.home.canonicalize().ok())
                .is_some_and(|(p, home)| p.starts_with(home))
    }
    pub fn with_docker_source(mut self, source: impl super::docker::DockerSource) -> Self {
        self.docker = Arc::new(super::docker::Monitor::new(source));
        self
    }
    pub fn docker_monitor(&self) -> Arc<super::docker::Monitor> {
        self.docker.clone()
    }
    pub fn read_json(&self, path: &Path) -> Result<Value, &'static str> {
        let data = self.read_text(path)?;
        serde_json::from_str(&data).map_err(|_| "invalid_json")
    }
    pub fn read_text(&self, path: &Path) -> Result<String, &'static str> {
        if !self.allows(path) {
            return Err("outside_fixture_root");
        }
        self.files
            .lock()
            .map_err(|_| "config_cache_poisoned")?
            .read(path)
    }
    pub fn read_jsonc(&self, path: &Path) -> Result<Value, &'static str> {
        jsonc::parse(&self.read_text(path)?)
    }
    /// WHOATE_RULES is an explicit single-file capability, not a general root.
    pub fn read_rules_text(&self) -> Result<Option<String>, &'static str> {
        match std::fs::symlink_metadata(&self.rules) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("rules_read_failed"),
            Ok(_) => {}
        }
        if !self.rules_explicit && !self.allows(&self.rules) {
            return Err("outside_fixture_root");
        }
        self.files
            .lock()
            .map_err(|_| "config_cache_poisoned")?
            .read(&self.rules)
            .map(Some)
    }
    /// Observable I/O count; clones share the same read-only file cache.
    pub fn disk_reads(&self) -> usize {
        self.files.lock().expect("config cache lock").reads
    }
    pub fn plist_parses(&self) -> usize {
        self.files.lock().expect("config cache lock").plist_parses
    }
    pub fn project_file(&self, cwd: &Path, name: &str) -> Option<PathBuf> {
        for ancestor in cwd.ancestors() {
            if self.isolated && !ancestor.starts_with(&self.home) {
                break;
            }
            let path = ancestor.join(name);
            if path.is_file() && self.allows(&path) {
                return Some(path);
            }
        }
        None
    }
    pub fn read_plist(&self, path: &Path) -> Result<plist::Value, &'static str> {
        if !self.allows(path) {
            return Err("outside_fixture_root");
        }
        let mut files = self.files.lock().map_err(|_| "config_cache_poisoned")?;
        files.read_plist(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase02_insights_three_path_overrides_and_default_contract() {
        let env = BTreeMap::from([
            ("HOME", "/unused"),
            ("WHOATE_HOME_OVERRIDE", "/fixture"),
            ("WHOATE_CACHE_DIR", "/cache"),
            ("WHOATE_RULES", "/rules.toml"),
        ]);
        let cfg =
            ConfigReader::from_lookup(|key| env.get(key).map(std::ffi::OsString::from)).unwrap();
        assert_eq!(cfg.home, PathBuf::from("/fixture"));
        assert_eq!(cfg.cache, PathBuf::from("/cache"));
        assert_eq!(cfg.rules, PathBuf::from("/rules.toml"));
        let cfg = ConfigReader::from_lookup(|key| {
            (key == "HOME").then(|| std::ffi::OsString::from("/fixture"))
        })
        .unwrap();
        assert_eq!(cfg.cache, PathBuf::from("/fixture/Library/Caches/whoate"));
        assert_eq!(
            cfg.rules,
            PathBuf::from("/fixture/.config/whoate/rules.toml")
        );
        assert!(ConfigReader::from_lookup(|_| None).is_err());
    }
}
