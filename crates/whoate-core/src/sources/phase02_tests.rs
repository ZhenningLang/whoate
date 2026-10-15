use super::{Sessions, opencode, testing::Temp};
use crate::{model::Confidence, recognizers::ConfigReader, snapshot::Proc};

#[test]
fn phase02_sqlite_interpreter_entrypoint_is_a_tool_signal_not_arbitrary_text() {
    for (entry, want) in [
        ("/demo/opencode", Some("opencode")),
        ("/demo/kilo.js", Some("kilo")),
        ("/demo/not-opencode", None),
        ("--eval", None),
    ] {
        let mut p = process("node");
        p.argv = vec!["node".into(), entry.into(), "opencode".into()];
        assert_eq!(super::tool(&p), want);
    }
}

#[test]
fn phase02_sqlite_cache_cannot_cross_home_override_roots() {
    let a = Temp::new();
    let b = Temp::new();
    let (_, connection) = database(&a, "opencode", false);
    let p = process("opencode");
    let mut sessions = Sessions::default();
    assert_eq!(
        sessions
            .collect(
                std::slice::from_ref(&p),
                &ConfigReader::isolated(a.0.clone())
            )
            .0[&p.pid]
            .session_id
            .as_deref(),
        Some("legacy")
    );
    let (agents, _) = sessions.collect(
        std::slice::from_ref(&p),
        &ConfigReader::isolated(b.0.clone()),
    );
    assert_eq!(agents[&p.pid].session_id, None);
    drop(connection);
}

fn process(tool: &str) -> Proc {
    let mut p = Proc::identity(999001, 1, 501, Some(1_000_000), tool.into());
    p.cwd = Some("/demo".into());
    p
}
fn database(t: &Temp, tool: &str, modern: bool) -> (std::path::PathBuf, rusqlite::Connection) {
    let path = t.write(&format!(".local/share/{tool}/{tool}.db"), "");
    let c = rusqlite::Connection::open(&path).unwrap();
    if modern {
        c.execute_batch("CREATE TABLE session_v2(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER,model TEXT,cost REAL);CREATE TABLE session_message(id TEXT,session_id TEXT,type TEXT,time_created INTEGER,data TEXT);
        INSERT INTO session_v2 VALUES('modern','/demo',NULL,'Modern title',1000,12000,'{\"id\":\"modern-model\",\"providerID\":\"example\"}',0.5);
        INSERT INTO session_message VALUES('u','modern','user',1,'{\"text\":\"Modern question\"}'),('a','modern','assistant',2,'{\"content\":[{\"type\":\"reasoning\",\"text\":\"SECRET\"},{\"type\":\"text\",\"text\":\"Modern answer\"}]}');").unwrap();
    } else {
        c.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER);CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT);CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);INSERT INTO session VALUES('legacy','/demo',NULL,'Legacy title',1000,12000);").unwrap();
    }
    (path, c)
}
#[test]
fn phase02_sqlite_both_adapters_v2_text_and_exact_pairing_window() {
    for tool in ["opencode", "kilo"] {
        let t = Temp::new();
        let (path, c) = database(&t, tool, true);
        let p = process(tool);
        let a = opencode::read(&path, &p, tool).unwrap().unwrap();
        assert_eq!(a.first_prompt.as_deref(), Some("Modern question"));
        assert_eq!(a.first_reply.as_deref(), Some("Modern answer"));
        assert_eq!(a.model.as_deref(), Some("modern-model"));
        assert_eq!(a.cost_usd, Some(0.5));
        for (created, matched) in [(999, false), (1000, true), (11000, true), (11001, false)] {
            c.execute("UPDATE session_v2 SET time_created=?1", [created])
                .unwrap();
            assert_eq!(
                opencode::read(&path, &p, tool).unwrap().is_some(),
                matched,
                "creation={created}"
            );
        }
        c.execute(
            "UPDATE session_v2 SET time_created=1000,parent_id='parent'",
            [],
        )
        .unwrap();
        assert!(opencode::read(&path, &p, tool).unwrap().is_none());
        c.execute(
            "UPDATE session_v2 SET parent_id=NULL,directory='/other'",
            [],
        )
        .unwrap();
        assert!(opencode::read(&path, &p, tool).unwrap().is_none());
    }
}
#[test]
fn phase02_sqlite_real_lock_stale_cache_and_identity_reuse() {
    for tool in ["opencode", "kilo"] {
        let t = Temp::new();
        let (_, c) = database(&t, tool, false);
        let cfg = ConfigReader::isolated(t.0.clone());
        let mut cache = Sessions::default();
        let mut p = process(tool);
        let (fresh, _) = cache.collect(&[p.clone()], &cfg);
        assert_eq!(fresh[&p.pid].session_id.as_deref(), Some("legacy"));
        assert!(!fresh[&p.pid].stale);
        c.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let (stale, issues) = cache.collect(&[p.clone()], &cfg);
        assert_eq!(stale[&p.pid].session_id.as_deref(), Some("legacy"));
        assert!(stale[&p.pid].stale);
        assert_eq!(stale[&p.pid].confidence, Confidence::Weak);
        assert!(!issues.is_empty());
        p.start_time = Some(2_000_000);
        let (reused, _) = cache.collect(&[p.clone()], &cfg);
        assert_eq!(reused[&p.pid].session_id, None);
        c.execute_batch("ROLLBACK").unwrap();
    }
}
#[test]
fn phase02_sqlite_wal_updates_are_visible_and_no_reader_writes() {
    let t = Temp::new();
    let (path, c) = database(&t, "opencode", true);
    c.pragma_update(None, "journal_mode", "WAL").unwrap();
    let p = process("opencode");
    assert_eq!(
        opencode::read(&path, &p, "opencode")
            .unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("Modern title")
    );
    c.execute("UPDATE session_v2 SET title='WAL update'", [])
        .unwrap();
    assert_eq!(
        opencode::read(&path, &p, "opencode")
            .unwrap()
            .unwrap()
            .title
            .as_deref(),
        Some("WAL update")
    );
    assert_eq!(
        c.query_row("SELECT COUNT(*) FROM session_v2", [], |r| r
            .get::<_, i64>(0))
            .unwrap(),
        1
    );
}
