//! Codex sessions run by ChatGPT.app (phase 10 R7). A tool call's process
//! carries CODEX_THREAD_ID; only those ids are looked up, and only metadata
//! columns are read from `~/.codex/state_<N>.sqlite`. Conversation text and
//! session files are never opened.
use super::cache::Stamp;
use crate::{
    model::{Agent, Confidence},
    recognizers::ConfigReader,
};
use rusqlite::Connection;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

/// Subagent edges are followed this many levels up, at most.
// ceiling: one edge query per level, ≤ 8 per lookup; use a recursive CTE if
// subagent trees grow deeper or lookups show up in tick timing.
const MAX_DEPTH: usize = 8;
const COLUMNS: &[&str] = &[
    "id",
    "name",
    "cwd",
    "git_branch",
    "updated_at_ms",
    "archived",
];

/// The state database with the highest version number.
fn database(home: &Path) -> Option<PathBuf> {
    std::fs::read_dir(home.join(".codex"))
        .ok()?
        .filter_map(|entry| {
            let name = entry.ok()?.file_name();
            let version: u32 = name
                .to_str()?
                .strip_prefix("state_")?
                .strip_suffix(".sqlite")?
                .parse()
                .ok()?;
            Some((version, name))
        })
        .max()
        .map(|(_, name)| home.join(".codex").join(name))
}

struct Row {
    name: Option<String>,
    cwd: Option<String>,
    branch: Option<String>,
    updated_at_ms: Option<i64>,
    archived: bool,
}

fn placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

fn columns(conn: &Connection, table: &str) -> rusqlite::Result<BTreeSet<String>> {
    // table is one of the two hard-coded names below.
    conn.prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect()
}

/// Each id mapped to its topmost parent session (itself when it has none).
fn roots(conn: &Connection, ids: &BTreeSet<String>) -> rusqlite::Result<BTreeMap<String, String>> {
    let mut parent: BTreeMap<String, String> = BTreeMap::new();
    let mut queried: BTreeSet<String> = BTreeSet::new();
    let mut pending: BTreeSet<String> = ids.clone();
    for _ in 0..MAX_DEPTH {
        if pending.is_empty() {
            break;
        }
        let list: Vec<_> = pending.iter().cloned().collect();
        queried.extend(pending.iter().cloned());
        let mut statement = conn.prepare(&format!(
            "SELECT parent_thread_id, child_thread_id FROM thread_spawn_edges WHERE child_thread_id IN ({})",
            placeholders(list.len())
        ))?;
        let edges: Vec<(String, String)> = statement
            .query_map(rusqlite::params_from_iter(&list), |r| {
                Ok((r.get(0)?, r.get(1)?))
            })?
            .collect::<rusqlite::Result<_>>()?;
        pending.clear();
        for (up, child) in edges {
            if !queried.contains(&up) {
                pending.insert(up.clone());
            }
            parent.entry(child).or_insert(up);
        }
    }
    Ok(ids
        .iter()
        .map(|id| {
            // At most MAX_DEPTH steps, and a cycle stops where it repeats.
            let mut seen = BTreeSet::from([id.clone()]);
            let mut top = id.clone();
            for _ in 0..MAX_DEPTH {
                match parent.get(&top) {
                    Some(up) if seen.insert(up.clone()) => top = up.clone(),
                    _ => break,
                }
            }
            (id.clone(), top)
        })
        .collect())
}

fn rows(conn: &Connection, ids: &BTreeSet<String>) -> rusqlite::Result<BTreeMap<String, Row>> {
    let list: Vec<_> = ids.iter().cloned().collect();
    let mut statement = conn.prepare(&format!(
        "SELECT {} FROM threads WHERE id IN ({})",
        COLUMNS.join(", "),
        placeholders(list.len())
    ))?;
    statement
        .query_map(rusqlite::params_from_iter(&list), |r| {
            Ok((
                r.get::<_, String>(0)?,
                Row {
                    name: r.get(1)?,
                    cwd: r.get(2)?,
                    branch: r.get(3)?,
                    updated_at_ms: r.get(4)?,
                    archived: r.get::<_, Option<i64>>(5)?.is_some_and(|v| v != 0),
                },
            ))
        })?
        .collect()
}

/// One agent per top-level session, keyed by every id that maps to it.
type Lookup = (BTreeMap<String, Agent>, Vec<&'static str>);

fn read(path: Option<&Path>, ids: &BTreeSet<String>) -> Lookup {
    let only_ids = |code| {
        let agents = ids.iter().map(|id| (id.clone(), agent(id, None))).collect();
        (agents, vec![code])
    };
    let Some(path) = path else {
        return only_ids("codex_db_missing");
    };
    // Opening and reading the schema can fail for a moment (busy, locked): that
    // is a read failure, retried next tick. Only a schema read that succeeded
    // and lacks columns is a stable answer.
    let schema = super::opencode::open_read_only(path).ok().and_then(|conn| {
        let threads = columns(&conn, "threads").ok()?;
        let edges = columns(&conn, "thread_spawn_edges").ok()?;
        Some((conn, threads, edges))
    });
    let Some((conn, threads, edges)) = schema else {
        return only_ids("codex_db_read_failed");
    };
    if !COLUMNS.iter().all(|c| threads.contains(*c))
        || !edges.contains("parent_thread_id")
        || !edges.contains("child_thread_id")
    {
        return only_ids("codex_schema_unknown");
    }
    let result = (|| {
        let top = roots(&conn, ids)?;
        let wanted: BTreeSet<_> = top.values().cloned().collect();
        let rows = rows(&conn, &wanted)?;
        Ok::<_, rusqlite::Error>((top, rows))
    })();
    let Ok((top, rows)) = result else {
        return only_ids("codex_db_read_failed");
    };
    let mut issues = vec![];
    let agents = top
        .into_iter()
        .map(|(id, root)| {
            let row = rows.get(&root);
            if row.is_none() {
                issues.push("codex_thread_unknown");
            }
            (id, agent(&root, row))
        })
        .collect();
    issues.dedup();
    (agents, issues)
}

fn agent(session: &str, row: Option<&Row>) -> Agent {
    let mut a = Agent::unknown("codex");
    a.session_id = Some(session.into());
    a.confidence = Confidence::Certain;
    if let Some(row) = row {
        a.name = row.name.clone().filter(|n| !n.trim().is_empty());
        a.name_source = a.name.as_ref().map(|_| "codex".into());
        a.cwd = row.cwd.clone();
        a.branch = row.branch.clone();
        a.updated_at_ms = row.updated_at_ms;
        a.status = row.archived.then(|| "archived".into());
    }
    a
}

/// The last lookup, reused while the id set and the database files are unchanged.
#[derive(Default)]
pub(super) struct Cached {
    key: Option<(BTreeSet<String>, Option<Stamp>, Option<Stamp>)>,
    value: Option<Lookup>,
}
impl Cached {
    pub(super) fn lookup(&mut self, ids: &BTreeSet<String>, cfg: &ConfigReader) -> Lookup {
        if ids.is_empty() {
            self.key = None;
            self.value = None;
            return Default::default();
        }
        let path = database(&cfg.home).filter(|p| cfg.allows(p));
        let key = (
            ids.clone(),
            path.as_deref().and_then(Stamp::read),
            path.as_ref()
                .and_then(|p| Stamp::read(Path::new(&format!("{}-wal", p.display())))),
        );
        if self.key.as_ref() == Some(&key)
            && let Some(value) = &self.value
        {
            return value.clone();
        }
        let value = read(path.as_deref(), ids);
        // A failed read (busy, mid-write) is retried next tick even if no file changes.
        let failed = value.1.contains(&"codex_db_read_failed");
        self.key = (!failed).then_some(key);
        self.value = Some(value.clone());
        value
    }
}
