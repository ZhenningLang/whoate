mod common;
use common::Temp;
use std::fs;
use whoate_core::{
    recognizers::ConfigReader,
    snapshot::{Proc, Snapshot},
    sources::Sessions,
};

fn process(tool: &str) -> Proc {
    let mut p = Proc::identity(999001, 1, 501, Some(1_000_000), tool.into());
    p.cwd = Some("/demo".into());
    p
}
fn put(home: &Temp, path: &str, data: &str) -> std::path::PathBuf {
    let p = home.0.join(path);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(&p, data).unwrap();
    p
}

fn cache_database(path: &std::path::Path, title: &str) -> rusqlite::Connection {
    let conn = rusqlite::Connection::open(path).unwrap();
    conn.execute_batch("CREATE TABLE session(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE message(id TEXT,session_id TEXT,time_created INTEGER,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT);").unwrap();
    conn.execute(
        "INSERT INTO session VALUES('root','/demo',NULL,?1,1000,5000)",
        [title],
    )
    .unwrap();
    conn
}

#[test]
fn phase09_sqlite_replacement_reopens_the_new_inode_instead_of_cached_connection() {
    for tool in ["opencode", "kilo"] {
        let home = Temp::new();
        let path = put(&home, &format!(".local/share/{tool}/{tool}.db"), "");
        let original = cache_database(&path, "Original inode");
        let cfg = ConfigReader::isolated(home.0.clone());
        let p = process(tool);
        let mut sessions = Sessions::default();
        assert_eq!(
            sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
                .title
                .as_deref(),
            Some("Original inode")
        );
        assert_eq!(
            sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
                .title
                .as_deref(),
            Some("Original inode")
        );
        assert_eq!(sessions.source_reads(), 1);
        let replacement = home.0.join("replacement.db");
        drop(cache_database(&replacement, "New inode"));
        drop(original);
        fs::rename(&replacement, &path).unwrap();
        let (agents, issues) = sessions.collect(std::slice::from_ref(&p), &cfg);
        assert!(issues.is_empty());
        assert_eq!(agents[&p.pid].title.as_deref(), Some("New inode"));
        assert!(!agents[&p.pid].stale);
        assert_eq!(sessions.source_reads(), 2);
    }
}

#[test]
fn phase09_sqlite_lock_release_recovers_fresh_after_stale_memo_failure() {
    for tool in ["opencode", "kilo"] {
        let home = Temp::new();
        let path = put(&home, &format!(".local/share/{tool}/{tool}.db"), "");
        let writer = cache_database(&path, "Before lock");
        let cfg = ConfigReader::isolated(home.0.clone());
        let p = process(tool);
        let mut sessions = Sessions::default();
        assert!(!sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid].stale);
        writer.execute_batch("BEGIN EXCLUSIVE").unwrap();
        let (stale, issues) = sessions.collect(std::slice::from_ref(&p), &cfg);
        assert!(!issues.is_empty());
        assert!(stale[&p.pid].stale);
        assert_eq!(
            stale[&p.pid].confidence,
            whoate_core::model::Confidence::Weak
        );
        assert_eq!(stale[&p.pid].title.as_deref(), Some("Before lock"));
        writer
            .execute_batch("ROLLBACK; UPDATE session SET title='Recovered'")
            .unwrap();
        let (fresh, issues) = sessions.collect(std::slice::from_ref(&p), &cfg);
        assert!(issues.is_empty());
        assert!(!fresh[&p.pid].stale);
        assert_eq!(fresh[&p.pid].title.as_deref(), Some("Recovered"));
    }
}

#[test]
fn phase09_claude_unchanged_collect_reuses_both_sources_without_freezing_idle() {
    let home = Temp::new();
    let session = put(
        &home,
        ".claude/sessions/999001.json",
        r#"{"sessionId":"demo","cwd":"/demo","name":"First"}"#,
    );
    let transcript = put(
        &home,
        ".claude/projects/-demo/demo.jsonl",
        concat!(
            "{\"type\":\"user\",\"timestamp\":\"2026-10-17T00:00:00Z\",\"message\":{\"content\":\"Question\"}}\n",
            "{\"type\":\"assistant\",\"timestamp\":\"2026-10-17T00:00:01Z\",\"message\":{\"content\":\"Answer\",\"model\":\"example\"}}\n"
        ),
    );
    let cfg = ConfigReader::isolated(home.0.clone());
    let p = process("claude");
    let mut sessions = Sessions::default();
    let first = sessions
        .collect(std::slice::from_ref(&p), &cfg)
        .0
        .remove(&p.pid)
        .unwrap();
    let second = sessions
        .collect(std::slice::from_ref(&p), &cfg)
        .0
        .remove(&p.pid)
        .unwrap();
    assert_eq!(first, second);
    assert_eq!(
        sessions.source_reads(),
        1,
        "unchanged metadata+transcript must not parse again"
    );
    assert_eq!(first.first_prompt.as_deref(), Some("Question"));
    assert_eq!(first.first_reply.as_deref(), Some("Answer"));
    let at = Snapshot::historical("2026-10-17T00:00:11Z".into(), vec![]);
    let later = Snapshot::historical("2026-10-17T00:00:21Z".into(), vec![]);
    assert_eq!(whoate_core::sources::idle_seconds(&second, &at), Some(10));
    assert_eq!(
        whoate_core::sources::idle_seconds(&second, &later),
        Some(20)
    );
    fs::write(
        session,
        r#"{"sessionId":"demo","cwd":"/demo","name":"Metadata changed"}"#,
    )
    .unwrap();
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
            .name
            .as_deref(),
        Some("Metadata changed")
    );
    let mut data = fs::read_to_string(&transcript).unwrap();
    data.push_str("{\"type\":\"custom-title\",\"customTitle\":\"Latest\"}\n");
    fs::write(&transcript, data).unwrap();
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
            .title
            .as_deref(),
        Some("Latest")
    );
    fs::remove_file(&transcript).unwrap();
    let missing = sessions.collect(std::slice::from_ref(&p), &cfg);
    assert!(!missing.1.is_empty());
    assert_eq!(missing.0[&p.pid].first_prompt, None);
}

#[test]
fn phase09_sqlite_two_tools_schemas_reuse_results_and_notice_wal_messages() {
    for tool in ["opencode", "kilo"] {
        for modern in [false, true] {
            let home = Temp::new();
            let path = put(&home, &format!(".local/share/{tool}/{tool}.db"), "");
            let conn = rusqlite::Connection::open(&path).unwrap();
            let session = if modern { "session_v2" } else { "session" };
            let message = if modern { "session_message" } else { "message" };
            conn.execute_batch(&format!("PRAGMA journal_mode=WAL; CREATE TABLE {session}(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE {message}(id TEXT,session_id TEXT,time_created INTEGER,type TEXT,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT); INSERT INTO {session} VALUES('root','/demo',NULL,'Original',1000,5000); INSERT INTO {message} VALUES('a','root',2000,'assistant','{{\"role\":\"assistant\",\"content\":\"Answer\",\"cost\":0.25}}');")).unwrap();
            let cfg = ConfigReader::isolated(home.0.clone());
            let p = process(tool);
            let mut sessions = Sessions::default();
            let first = sessions
                .collect(std::slice::from_ref(&p), &cfg)
                .0
                .remove(&p.pid)
                .unwrap();
            assert_eq!(first.cost_usd, Some(0.25));
            assert_eq!(
                sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid],
                first
            );
            assert_eq!(
                sessions.source_reads(),
                1,
                "{tool} modern={modern} must reuse parsed result"
            );
            // No session update: message-only WAL commit must invalidate the memo.
            conn.execute_batch(&format!("INSERT INTO {message} VALUES('b','root',9000,'assistant','{{\"role\":\"assistant\",\"content\":\"Later\",\"cost\":0.5}}');")).unwrap();
            let next = sessions
                .collect(std::slice::from_ref(&p), &cfg)
                .0
                .remove(&p.pid)
                .unwrap();
            assert_eq!(next.cost_usd, Some(0.75));
            assert_eq!(next.updated_at_ms, Some(5000));
            assert_eq!(
                next.last_activity_at.as_deref(),
                Some("1970-01-01T00:00:09Z")
            );
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
                .unwrap();
            assert_eq!(
                sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid],
                next
            );
            let mut reused = p.clone();
            reused.start_time = Some(100_000_000);
            assert_eq!(sessions.collect(&[reused], &cfg).0[&p.pid].session_id, None);
        }
    }
}

#[test]
fn phase09_claude_replacements_truncation_pending_pair_and_stale_recovery() {
    let home = Temp::new();
    let session = put(
        &home,
        ".claude/sessions/999001.json",
        r#"{"sessionId":"demo","cwd":"/demo"}"#,
    );
    let transcript = put(
        &home,
        ".claude/projects/-demo/demo.jsonl",
        "{\"type\":\"user\",\"message\":{\"content\":\"First\"}}\n",
    );
    let cfg = ConfigReader::isolated(home.0.clone());
    let mut sessions = Sessions::default();
    let mut p = process("claude");
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid].first_reply,
        None
    );
    let pair = concat!(
        "{\"type\":\"user\",\"message\":{\"content\":\"First\"}}\n",
        "{\"type\":\"assistant\",\"message\":{\"content\":\"Reply\"}}\n"
    );
    fs::write(&transcript, pair).unwrap();
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
            .first_reply
            .as_deref(),
        Some("Reply")
    );
    let replacement = put(&home, "replacement", &pair.replace("First", "Other"));
    let stamp = fs::metadata(&transcript).unwrap().modified().unwrap();
    fs::File::options()
        .write(true)
        .open(&replacement)
        .unwrap()
        .set_modified(stamp)
        .unwrap();
    fs::rename(replacement, &transcript).unwrap();
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
            .first_prompt
            .as_deref(),
        Some("Other")
    );
    fs::write(&transcript, "{}\n").unwrap();
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid].first_prompt,
        None
    );
    fs::write(&transcript, format!("{pair}bad line\n{{}}\n")).unwrap();
    assert!(
        !sessions
            .collect(std::slice::from_ref(&p), &cfg)
            .1
            .is_empty()
    );
    fs::write(&transcript, pair).unwrap();
    assert!(
        sessions
            .collect(std::slice::from_ref(&p), &cfg)
            .1
            .is_empty()
    );
    fs::remove_file(&session).unwrap();
    let stale = sessions.collect(std::slice::from_ref(&p), &cfg);
    assert!(stale.0[&p.pid].stale);
    assert_eq!(
        stale.0[&p.pid].confidence,
        whoate_core::model::Confidence::Weak
    );
    assert_eq!(stale.0[&p.pid].first_reply.as_deref(), Some("Reply"));
    p.start_time = Some(2_000_000);
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid].session_id,
        None
    );
    fs::write(session, r#"{"sessionId":"demo","cwd":"/demo"}"#).unwrap();
    assert!(!sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid].stale);
}

#[test]
fn phase09_engine_recomputes_idle_on_a_successful_source_memo_hit() {
    use whoate_core::{
        engine::{Clock, Context, Engine},
        sample::{SampleResult, Sampler},
        verdict::Language,
    };
    struct Frames {
        p: Proc,
        second: u8,
    }
    impl Sampler for Frames {
        fn sample(&mut self) -> SampleResult<Snapshot> {
            self.second += 10;
            Ok(Snapshot::historical(
                format!("2026-10-17T00:00:{:02}Z", self.second),
                vec![self.p.clone()],
            ))
        }
    }
    #[derive(Default)]
    struct Time(u128);
    impl Clock for Time {
        fn now_ns(&self) -> u128 {
            self.0
        }
        fn wait(&mut self, _: std::time::Duration) {
            self.0 += 1_000_000_000;
        }
    }
    let home = Temp::new();
    put(
        &home,
        ".claude/sessions/999001.json",
        r#"{"sessionId":"demo","cwd":"/demo"}"#,
    );
    put(
        &home,
        ".claude/projects/-demo/demo.jsonl",
        "{\"type\":\"user\",\"timestamp\":\"2026-10-17T00:00:01Z\",\"message\":{\"content\":\"First\"}}\n",
    );
    let mut p = process("claude");
    p.footprint_bytes = Some(1);
    p.cpu_time_ns = Some(0);
    let mut engine = Engine::with_sampler(
        Frames { p, second: 0 },
        Time::default(),
        Context {
            cfg: ConfigReader::isolated(home.0.clone()),
            uid: 501,
            language: Language::En,
            platform_probe: |_| Ok(false),
        },
    );
    let first = engine.tick().unwrap();
    let second = engine.tick().unwrap();
    assert_eq!(
        first
            .owners
            .iter()
            .find_map(|o| o.agent.as_ref())
            .unwrap()
            .idle_seconds,
        Some(19)
    );
    assert_eq!(
        second
            .owners
            .iter()
            .find_map(|o| o.agent.as_ref())
            .unwrap()
            .idle_seconds,
        Some(29)
    );
}

#[test]
fn phase09_claude_normal_and_first_pair_capabilities_never_share_a_memo() {
    let home = Temp::new();
    put(
        &home,
        ".claude/sessions/999001.json",
        r#"{"sessionId":"demo","cwd":"/demo","title":"Metadata only"}"#,
    );
    put(
        &home,
        ".claude/projects/-demo/demo.jsonl",
        concat!(
            "{\"type\":\"user\",\"message\":{\"content\":\"First\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":\"Reply\"}}\n",
            "{\"type\":\"custom-title\",\"customTitle\":\"Unauthorized tail\"}\n",
            "{\"type\":\"user\",\"timestamp\":\"2026-10-19T00:00:00Z\",\"message\":{\"content\":\"Later\"}}\n"
        ),
    );
    let normal = ConfigReader::isolated(home.0.clone());
    let limited = ConfigReader::from_lookup(|key| match key {
        "WHOATE_HOME_OVERRIDE" => Some(home.0.clone().into_os_string()),
        "WHOATE_SESSION_PID" => Some("999001".into()),
        _ => None,
    })
    .unwrap();
    let mut sessions = Sessions::default();
    let p = process("claude");
    for cfg in [&normal, &limited, &limited, &normal] {
        let a = sessions
            .collect(std::slice::from_ref(&p), cfg)
            .0
            .remove(&p.pid)
            .unwrap();
        assert_eq!(a.first_prompt.as_deref(), Some("First"));
        assert_eq!(a.first_reply.as_deref(), Some("Reply"));
        if cfg.session_pid().is_some() {
            assert_eq!(a.title.as_deref(), Some("Metadata only"));
            assert_eq!(a.last_activity_at, None);
        } else {
            assert_eq!(a.title.as_deref(), Some("Unauthorized tail"));
            assert_eq!(a.last_activity_at.as_deref(), Some("2026-10-19T00:00:00Z"));
        }
    }
}

#[test]
fn phase09_physical_home_replacement_does_not_reuse_an_old_stale_session() {
    let old = Temp::new();
    let new = Temp::new();
    let root = Temp::new();
    put(
        &old,
        ".claude/sessions/999001.json",
        r#"{"sessionId":"private-old","cwd":"/demo"}"#,
    );
    put(
        &old,
        ".claude/projects/-demo/private-old.jsonl",
        "{\"type\":\"user\",\"message\":{\"content\":\"Old root\"}}\n",
    );
    let link = root.0.join("home");
    std::os::unix::fs::symlink(&old.0, &link).unwrap();
    let cfg = ConfigReader::isolated(link.clone());
    let p = process("claude");
    let mut sessions = Sessions::default();
    assert_eq!(
        sessions.collect(std::slice::from_ref(&p), &cfg).0[&p.pid]
            .session_id
            .as_deref(),
        Some("private-old")
    );
    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&new.0, &link).unwrap();
    let current = sessions
        .collect(std::slice::from_ref(&p), &cfg)
        .0
        .remove(&p.pid)
        .unwrap();
    assert_eq!(
        current.session_id, None,
        "same path spelling is not the same physical home capability"
    );
    assert_eq!(current.first_prompt, None);
}
