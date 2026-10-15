//! Read-only filesystem boundary. Tests inject paths instead of changing HOME.
use serde_json::Value;
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct ConfigReader {
    pub language: crate::verdict::Language,
    pub home: PathBuf,
    pub cache: PathBuf,
    pub rules: PathBuf,
    /// Registered software configuration signals supplied by read-only adapters.
    pub values: BTreeMap<String, Value>,
    isolated: bool,
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
        let rules = env("WHOATE_RULES")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".config/whoate/rules.toml"));
        Ok(Self {
            language: crate::verdict::Language::En,
            home,
            cache,
            rules,
            values: BTreeMap::new(),
            isolated,
        })
    }
    pub fn isolated(home: PathBuf) -> Self {
        Self {
            language: crate::verdict::Language::En,
            cache: home.join("cache"),
            rules: home.join("rules.toml"),
            home,
            values: BTreeMap::new(),
            isolated: true,
        }
    }
    pub fn allows(&self, path: &Path) -> bool {
        !self.isolated
            || path
                .canonicalize()
                .ok()
                .zip(self.home.canonicalize().ok())
                .is_some_and(|(p, home)| p.starts_with(home))
    }
    pub fn read_json(&self, path: &Path) -> Result<Value, &'static str> {
        if !self.allows(path) {
            return Err("outside_fixture_root");
        }
        let data = std::fs::read(path).map_err(|_| "read_failed")?;
        serde_json::from_slice(&data).map_err(|_| "invalid_json")
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
        plist::Value::from_file(path).map_err(|_| "app_metadata_unavailable")
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
