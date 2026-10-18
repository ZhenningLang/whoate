//! Read-only adapters for the upstream session/message/part and v2 schemas.
use super::claude_code::clean_text;
use crate::{
    model::{Agent, Confidence},
    snapshot::Proc,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension};
use serde_json::Value;
use std::{collections::BTreeSet, path::Path, time::Duration};

pub fn open_read_only(path: &Path) -> Result<Connection, &'static str> {
    let conn = Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|_| "database_open_failed")?;
    conn.busy_timeout(Duration::from_millis(50))
        .map_err(|_| "database_timeout_failed")?;
    Ok(conn)
}
fn exists(conn: &Connection, table: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name=?1)",
        [table],
        |r| r.get(0),
    )
}
fn columns(conn: &Connection, table: &str) -> rusqlite::Result<BTreeSet<String>> {
    // table is selected only from the hard-coded upstream schema names below.
    conn.prepare(&format!("PRAGMA table_info({table})"))?
        .query_map([], |r| r.get(1))?
        .collect()
}
fn model(value: &Value) -> Option<String> {
    value["id"]
        .as_str()
        .or_else(|| value["modelID"].as_str())
        .or_else(|| value.as_str())
        .map(str::to_owned)
}

/// Only message creation metadata, never session modification or part/data.
pub fn last_activity_ms(conn: &Connection, session: &str) -> rusqlite::Result<Option<i64>> {
    let has_modern = exists(conn, "session_message")?;
    let table = if has_modern
        && (!exists(conn, "message")?
            || conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_message WHERE session_id=?1)",
                [session],
                |r| r.get::<_, bool>(0),
            )?) {
        "session_message"
    } else {
        "message"
    };
    conn.query_row(
        &format!("SELECT MAX(time_created) FROM {table} WHERE session_id=?1"),
        [session],
        |r| r.get(0),
    )
}
pub fn read(path: &Path, proc: &Proc, tool: &str) -> Result<Option<Agent>, &'static str> {
    let Some((cwd, start)) = proc.cwd.as_deref().zip(proc.start_time) else {
        return Ok(None);
    };
    let start = i64::try_from(start / 1000).map_err(|_| "process_time_invalid")?;
    let end = start.checked_add(10_000).ok_or("process_time_invalid")?;
    let mut conn = open_read_only(path)?;
    let tx = conn.transaction().map_err(|_| "database_busy")?;
    read_transaction(&tx, cwd, start, end, tool)
}
fn read_transaction(
    tx: &Connection,
    cwd: &str,
    start: i64,
    end: i64,
    tool: &str,
) -> Result<Option<Agent>, &'static str> {
    let result = (|| -> rusqlite::Result<Option<Agent>> {
        let table = if exists(tx, "session_v2")? {
            "session_v2"
        } else {
            "session"
        };
        let cols = columns(tx, table)?;
        let optional = |name: &str| {
            if cols.contains(name) {
                name.to_owned()
            } else {
                "NULL".into()
            }
        };
        let query = format!(
            "SELECT id,title,time_updated,{},{} FROM {table} WHERE directory=?1 AND parent_id IS NULL AND time_created>=?2 AND time_created<=?3 ORDER BY time_created DESC,id DESC LIMIT 1",
            optional("model"),
            optional("cost")
        );
        let row = tx
            .query_row(&query, rusqlite::params![cwd, start, end], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, Option<String>>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                ))
            })
            .optional()?;
        let Some((id, title, updated, stored_model, stored_cost)) = row else {
            return Ok(None);
        };
        let mut agent = Agent::unknown(tool);
        agent.session_id = Some(id.clone());
        agent.title = title;
        agent.cwd = Some(cwd.into());
        agent.updated_at_ms = Some(updated);
        agent.last_activity_at = last_activity_ms(tx, &id)?
            .filter(|at| *at >= 0)
            .and_then(|at| {
                time::OffsetDateTime::from_unix_timestamp_nanos(at as i128 * 1_000_000).ok()
            })
            .and_then(|at| {
                at.format(&time::format_description::well_known::Rfc3339)
                    .ok()
            });
        agent.confidence = Confidence::Inferred;
        agent.cost_usd = stored_cost;
        if let Some(m) = stored_model {
            agent.model = serde_json::from_str::<Value>(&m)
                .ok()
                .and_then(|v| model(&v))
                .or(Some(m));
        }
        let modern = exists(tx, "session_message")?
            && tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM session_message WHERE session_id=?1)",
                [&id],
                |r| r.get::<_, bool>(0),
            )?;
        let query = if modern {
            "SELECT type,data FROM session_message WHERE session_id=?1 ORDER BY time_created,id"
        } else {
            "SELECT json_extract(data,'$.role'),data FROM message WHERE session_id=?1 ORDER BY time_created,id"
        };
        let mut statement = tx.prepare(query)?;
        let mut rows = statement.query([&id])?;
        let mut cost = None;
        while let Some(row) = rows.next()? {
            let role: String = row.get(0)?;
            let raw: String = row.get(1)?;
            let v: Value = serde_json::from_str(&raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
            let text = if modern {
                if role == "user" {
                    clean_text(&v["text"], 200)
                } else {
                    clean_text(&v["content"], 120)
                }
            } else {
                // Join on message_id as well as session_id; tool parts never become text.
                // The legacy row's id is fetched separately in the ordered text query below.
                None
            };
            if role == "user" && agent.first_prompt.is_none() {
                agent.first_prompt = text.clone();
            }
            if role == "assistant" {
                if agent.first_reply.is_none() {
                    agent.first_reply = text;
                }
                if agent.model.is_none() {
                    agent.model =
                        model(&v["model"]).or_else(|| v["modelID"].as_str().map(str::to_owned));
                }
                if let Some(c) = v["cost"].as_f64() {
                    cost = Some(cost.unwrap_or(0.0) + c);
                }
            }
        }
        if !modern {
            let mut stmt = tx.prepare("SELECT json_extract(m.data,'$.role'),p.data FROM message m JOIN part p ON p.message_id=m.id AND p.session_id=m.session_id WHERE m.session_id=?1 ORDER BY m.time_created,m.id,p.time_created,p.id")?;
            let mut parts = stmt.query([&id])?;
            while let Some(row) = parts.next()? {
                let role: String = row.get(0)?;
                let raw: String = row.get(1)?;
                let v: Value =
                    serde_json::from_str(&raw).map_err(|_| rusqlite::Error::InvalidQuery)?;
                if v["type"] != "text" {
                    continue;
                }
                if role == "user" && agent.first_prompt.is_none() {
                    agent.first_prompt = clean_text(&v["text"], 200);
                }
                if role == "assistant" && agent.first_reply.is_none() {
                    agent.first_reply = clean_text(&v["text"], 120);
                }
            }
        }
        if agent.cost_usd.is_none() {
            agent.cost_usd = cost;
        }
        Ok(Some(agent))
    })();
    result.map_err(|_| "database_read_failed")
}

/// One read-only connection per active source, with one parsed result per full
/// process identity. SQLite's read lock and data_version are checked even on hits.
#[derive(Default)]
pub(super) struct Cached {
    databases: std::collections::BTreeMap<std::path::PathBuf, Database>,
}
struct Database {
    conn: Connection,
    stamp: super::cache::Stamp,
    version: Option<i64>,
    wal: Option<super::cache::Stamp>,
    memo: std::collections::BTreeMap<super::SessionKey, Agent>,
}
impl Cached {
    pub(super) fn retain(&mut self, active: &std::collections::BTreeSet<super::SessionKey>) {
        self.databases.retain(|_, database| {
            database.memo.retain(|key, _| active.contains(key));
            !database.memo.is_empty()
        });
    }
    pub(super) fn read(
        &mut self,
        path: &Path,
        proc: &Proc,
        tool: &str,
        key: Option<&super::SessionKey>,
        reads: &mut usize,
    ) -> Result<Option<Agent>, &'static str> {
        let Some((cwd, start)) = proc.cwd.as_deref().zip(proc.start_time) else {
            return Ok(None);
        };
        let start = i64::try_from(start / 1000).map_err(|_| "process_time_invalid")?;
        let end = start.checked_add(10_000).ok_or("process_time_invalid")?;
        let stamp = super::cache::Stamp::read(path).ok_or("database_open_failed")?;
        if !self.databases.get(path).is_some_and(|db| db.stamp == stamp) {
            self.databases.remove(path);
            self.databases.insert(
                path.into(),
                Database {
                    conn: open_read_only(path)?,
                    stamp,
                    version: None,
                    wal: None,
                    memo: Default::default(),
                },
            );
        }
        let db = self.databases.get_mut(path).expect("opened database");
        let tx = db
            .conn
            .unchecked_transaction()
            .map_err(|_| "database_busy")?;
        // This real table read acquires SQLite's shared lock; PRAGMA alone can
        // otherwise return a cached version while an EXCLUSIVE writer owns it.
        tx.query_row("SELECT COUNT(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|_| "database_read_failed")?;
        let version = tx
            .query_row("PRAGMA data_version", [], |r| r.get::<_, i64>(0))
            .map_err(|_| "database_read_failed")?;
        let wal = super::cache::Stamp::read(Path::new(&format!("{}-wal", path.display())));
        if db.version != Some(version) || db.wal != wal {
            db.memo.clear();
            db.version = Some(version);
            db.wal = wal;
        }
        if let Some(agent) = key.and_then(|key| db.memo.get(key)) {
            if super::cache::Stamp::read(path).as_ref() != Some(&db.stamp) {
                return Err("database_changed_during_read");
            }
            return Ok(Some(agent.clone()));
        }
        *reads += 1;
        let result = read_transaction(&tx, cwd, start, end, tool)?;
        if super::cache::Stamp::read(path).as_ref() != Some(&db.stamp)
            || super::cache::Stamp::read(Path::new(&format!("{}-wal", path.display()))) != db.wal
        {
            return Err("database_changed_during_read");
        }
        if let Some(key) = key
            && let Some(agent) = &result
        {
            db.memo.insert(key.clone(), agent.clone());
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use crate::{model::Confidence, snapshot::Proc, sources::testing::Temp};
    #[test]
    fn phase02_sqlite_root_window_latest_and_read_only() {
        let temp = Temp::new();
        let path = temp.0.join("db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE session(id TEXT PRIMARY KEY,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);
        INSERT INTO session VALUES ('early','/work',NULL,'early',1000,1000),('latest','/work',NULL,'Latest',11000,12000),('late','/work',NULL,'late',11001,11001),('child','/work','early','child',10000,10000),('other','/other',NULL,'other',11000,11000);
        INSERT INTO message VALUES ('u','latest',1,'{\"role\":\"user\"}'),('a','latest',2,'{\"role\":\"assistant\",\"modelID\":\"example-model\",\"cost\":0.25}');
        INSERT INTO part VALUES ('p1','u','latest',1,'{\"type\":\"text\",\"text\":\"Question\"}'),('p2','a','latest',2,'{\"type\":\"tool\",\"text\":\"SECRET\"}'),('p3','a','latest',3,'{\"type\":\"text\",\"text\":\"Answer\"}');").unwrap();
        let mut p = Proc::identity(999001, 1, 501, Some(1_000_000), "opencode".into());
        p.cwd = Some("/work".into());
        let agent = super::read(&path, &p, "opencode").unwrap().unwrap();
        assert_eq!(agent.session_id.as_deref(), Some("latest"));
        assert_eq!(agent.title.as_deref(), Some("Latest"));
        assert_eq!(agent.confidence, Confidence::Inferred);
        assert_eq!(agent.first_prompt.as_deref(), Some("Question"));
        assert_eq!(agent.first_reply.as_deref(), Some("Answer"));
        assert_eq!(agent.model.as_deref(), Some("example-model"));
        assert_eq!(agent.cost_usd, Some(0.25));
        assert_eq!(agent.updated_at_ms, Some(12000));
        let reader = super::open_read_only(&path).unwrap();
        assert_eq!(
            reader
                .query_row("PRAGMA busy_timeout", [], |row| row.get::<_, i64>(0))
                .unwrap(),
            50
        );
        assert!(reader.execute("DELETE FROM session", []).is_err());
        let missing = temp.0.join("missing");
        assert!(super::read(&missing, &p, "opencode").is_err());
        assert!(!missing.exists());
    }
}
