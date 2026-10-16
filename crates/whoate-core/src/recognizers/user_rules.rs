//! Post-attribution display adaptation; this module never executes advice.
use super::{ConfigReader, dev_runtimes::fact};
use crate::{
    model::{FactValue, Owner},
    snapshot::{Proc, Snapshot},
};
use regex::Regex;
use serde::Deserialize;
use std::path::Path;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    rules: Vec<Rule>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    executable: Option<String>,
    argv_regex: Option<String>,
    cwd_prefix: Option<String>,
    label: String,
    advice: String,
}
struct Compiled {
    rule: Rule,
    argv: Option<Regex>,
}
#[derive(Default)]
pub struct Rules {
    rules: Vec<Compiled>,
}
impl Rules {
    pub fn load(cfg: &ConfigReader) -> Result<Self, &'static str> {
        let Some(text) = cfg.read_rules_text()? else {
            return Ok(Self::default());
        };
        let file: File = toml::from_str(&text).map_err(|_| "invalid_rules_toml")?;
        let mut rules = vec![];
        for rule in file.rules {
            if rule.label.trim().is_empty()
                || rule.advice.trim().is_empty()
                || (rule.executable.is_none()
                    && rule.argv_regex.is_none()
                    && rule.cwd_prefix.is_none())
                || [&rule.executable, &rule.argv_regex, &rule.cwd_prefix]
                    .iter()
                    .any(|s| s.as_ref().is_some_and(|v| v.is_empty()))
            {
                return Err("invalid_rule");
            }
            let argv = rule
                .argv_regex
                .as_deref()
                .map(Regex::new)
                .transpose()
                .map_err(|_| "invalid_regex")?;
            rules.push(Compiled { rule, argv });
        }
        Ok(Self { rules })
    }
    pub fn apply(&self, owner: &mut Owner, snap: &Snapshot, cfg: &ConfigReader) {
        // ceiling: linear scan for personal rule files; index by executable only
        // if measured user rule cost threatens the one-second tick budget.
        for (index, compiled) in self.rules.iter().enumerate() {
            if !snap
                .procs
                .iter()
                .any(|p| owner.pids.contains(&p.pid) && compiled.matches(p))
            {
                continue;
            }
            owner.label = compiled.rule.label.clone();
            fact(
                owner,
                "user_rule",
                cfg.language.choose("用户规则", "User rule"),
                FactValue::Count(index as u64 + 1),
            );
            fact(
                owner,
                "user_advice",
                cfg.language.choose("用户建议", "User advice"),
                FactValue::Text(compiled.rule.advice.clone()),
            );
            break;
        }
    }
}
impl Compiled {
    fn matches(&self, proc: &Proc) -> bool {
        let executable = proc
            .exe_path
            .as_deref()
            .or_else(|| proc.argv.first().map(String::as_str))
            .unwrap_or(&proc.comm);
        self.rule.executable.as_deref().is_none_or(|want| {
            Path::new(executable)
                .file_name()
                .is_some_and(|name| name == want)
        }) && self
            .argv
            .as_ref()
            .is_none_or(|regex| regex.is_match(&proc.argv.join(" ")))
            && self.rule.cwd_prefix.as_deref().is_none_or(|prefix| {
                proc.cwd
                    .as_deref()
                    .is_some_and(|cwd| Path::new(cwd).starts_with(prefix))
            })
    }
}
