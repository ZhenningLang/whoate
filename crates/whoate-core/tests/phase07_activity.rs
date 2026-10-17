use whoate_core::{
    history::History,
    model::{Agent, Level, Owner, OwnerKind},
    snapshot::Snapshot,
    verdict::{Language, rules},
};
mod common;

#[test]
fn claude_source_uses_tail_activity_not_file_mtime_and_restricted_mode_never_tails() {
    use whoate_core::{recognizers::ConfigReader, snapshot::Proc, sources::claude_code};
    let home = common::Temp::new();
    let session = home.0.join(".claude/sessions");
    std::fs::create_dir_all(&session).unwrap();
    std::fs::write(
        session.join("999001.json"),
        r#"{"sessionId":"activity","cwd":"/work","status":"idle"}"#,
    )
    .unwrap();
    let dir = home.0.join(".claude/projects/-work");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("activity.jsonl");
    std::fs::write(&path,concat!("{\"type\":\"user\",\"message\":{\"content\":\"First\"},\"timestamp\":\"2026-10-16T00:00:00Z\"}\n","{\"type\":\"assistant\",\"message\":{\"content\":\"Reply\"},\"timestamp\":\"2026-10-16T01:00:00Z\"}\n","{\"type\":\"user\",\"timestamp\":\"2026-10-16T02:00:00Z\"}\n")).unwrap();
    std::fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000))
        .unwrap();
    let proc = Proc::identity(999001, 1, 501, Some(1), "claude".into());
    let read = claude_code::read(&proc, &ConfigReader::isolated(home.0.clone())).unwrap();
    assert_eq!(read.agent.updated_at_ms, Some(1_000_000));
    assert_eq!(
        read.agent.last_activity_at.as_deref(),
        Some("2026-10-16T02:00:00Z")
    );
    let cfg = ConfigReader::from_lookup(|key| match key {
        "WHOATE_HOME_OVERRIDE" => Some(home.0.clone().into_os_string()),
        "WHOATE_SESSION_PID" => Some("999001".into()),
        _ => None,
    })
    .unwrap();
    let limited = claude_code::read(&proc, &cfg).unwrap();
    assert_eq!(limited.agent.last_activity_at, None);
    assert_eq!(limited.agent.first_reply.as_deref(), Some("Reply"));
    let mut other = proc;
    other.pid = 999002;
    assert_eq!(
        claude_code::read(&other, &cfg).unwrap_err(),
        "session_read_restricted"
    );
}
#[test]
fn idle_recent_message_wins_over_old_mtime_and_process_age() {
    let mut agent = serde_json::to_value(Agent::unknown("claude")).unwrap();
    agent["updated_at_ms"] = serde_json::json!(0);
    agent["status"] = serde_json::json!("idle");
    agent["last_activity_at"] = serde_json::json!("2026-10-17T00:00:00Z");
    let mut owner = Owner::new("agent:demo".into(), OwnerKind::AgentSession, "demo".into());
    owner.mem_bytes = 524_288_000;
    owner.agent = Some(serde_json::from_value(agent).unwrap());
    let snapshot = Snapshot::historical("2026-10-17T00:00:01Z".into(), vec![]);
    assert_eq!(
        rules::owner(
            &owner,
            &snapshot,
            &History::default(),
            false,
            false,
            Language::En
        )
        .level,
        Level::Ok
    );
}

#[test]
fn claude_activity_reads_message_metadata_and_rejects_incomplete_newest_time() {
    use whoate_core::sources::claude_code::{TAIL_BYTES, last_activity_from_tail};
    let data = concat!(
        "{\"type\":\"user\",\"timestamp\":\"2026-10-16T00:00:00Z\",\"message\":{\"content\":\"SECRET\"}}\n",
        "{\"type\":\"assistant\",\"timestamp\":\"2026-10-16T01:00:00+00:00\",\"message\":{\"content\":{\"unexpected\":true}}}\n",
        "{\"type\":\"user\",\"timestamp\":\"2026-10-17T00:00:00Z\",\"isSidechain\":true}\n",
        "{\"type\":\"custom-title\",\"timestamp\":\"2026-10-17T01:00:00Z\"}\n",
        "{\"type\":\"progress\",\"timestamp\":\"2026-10-17T02:00:00Z\"}\n"
    );
    assert_eq!(
        last_activity_from_tail(data.as_bytes(), false)
            .unwrap()
            .as_deref(),
        Some("2026-10-16T01:00:00+00:00")
    );
    for bad in ["not-time", "1960-01-01T00:00:00Z", ""] {
        let tail = format!("{data}{{\"type\":\"user\",\"timestamp\":\"{bad}\"}}\n");
        assert_eq!(
            last_activity_from_tail(tail.as_bytes(), false).unwrap(),
            None
        );
    }
    assert_eq!(
        last_activity_from_tail(format!("{data}{{\"type\":").as_bytes(), false),
        Err("activity_pending_last_line")
    );
    assert_eq!(
        last_activity_from_tail(&vec![b'x'; TAIL_BYTES as usize + 1], false),
        Err("activity_tail_limit")
    );
    assert_eq!(
        last_activity_from_tail(b"\xff\xfe\n", false),
        Err("activity_invalid_line")
    );
    assert_eq!(
        last_activity_from_tail(format!("fragment\n{data}").as_bytes(), true)
            .unwrap()
            .as_deref(),
        Some("2026-10-16T01:00:00+00:00")
    );
    // Malformed/truncated/repeated structures terminate safely without body use.
    for tail in [
        b"{\"type\":[] }\n".as_slice(),
        b"{\"type\":\"user\",\"timestamp\":42}\n",
        b"{}\n{}\n",
        b"{\"message\":{\"a\":]}}\n",
    ] {
        let _ = last_activity_from_tail(tail, false);
    }
}

#[test]
fn idle_boundaries_missing_future_and_stale_keep_original_thresholds() {
    for seconds in [7199, 7200, 7201] {
        for bytes in [524_287_999, 524_288_000, 524_288_001] {
            for status in [
                None,
                Some("idle"),
                Some("waiting"),
                Some("stopped"),
                Some("running"),
            ] {
                let mut a = Agent::unknown("claude");
                a.status = status.map(str::to_owned);
                a.updated_at_ms = Some(i64::MAX);
                a.last_activity_at = Some("2026-10-17T00:00:00Z".into());
                let mut o = Owner::new("agent:x".into(), OwnerKind::AgentSession, "x".into());
                o.agent = Some(a);
                o.mem_bytes = bytes;
                let at = time::OffsetDateTime::parse(
                    "2026-10-17T00:00:00Z",
                    &time::format_description::well_known::Rfc3339,
                )
                .unwrap()
                    + time::Duration::seconds(seconds);
                let snap = Snapshot::historical(
                    at.format(&time::format_description::well_known::Rfc3339)
                        .unwrap(),
                    vec![],
                );
                let v = rules::owner(&o, &snap, &History::default(), false, false, Language::En);
                assert_eq!(
                    v.level,
                    if seconds >= 7200 && bytes >= 524_288_000 && status != Some("running") {
                        Level::Watch
                    } else {
                        Level::Ok
                    }
                );
                assert_eq!(
                    whoate_core::sources::idle_seconds(o.agent.as_ref().unwrap(), &snap),
                    Some(seconds as u64)
                );
            }
        }
    }
    let snap = Snapshot::historical("2026-10-17T00:00:00Z".into(), vec![]);
    for at in [
        None,
        Some("bad"),
        Some("2026-10-17T00:00:01Z"),
        Some("1960-01-01T00:00:00Z"),
    ] {
        let mut a = Agent::unknown("claude");
        a.last_activity_at = at.map(str::to_owned);
        a.updated_at_ms = Some(0);
        assert_eq!(whoate_core::sources::idle_seconds(&a, &snap), None);
    }
    let mut a = Agent::unknown("claude");
    a.last_activity_at = Some("2026-10-16T00:00:00Z".into());
    a.stale = true;
    assert_eq!(whoate_core::sources::idle_seconds(&a, &snap), None);
}

#[test]
fn sqlite_kilo_and_opencode_activity_survives_title_changes_and_busy_sources_are_errors() {
    use whoate_core::{
        snapshot::Proc,
        sources::{kilo, opencode},
    };
    for modern in [false, true] {
        let home = common::Temp::new();
        let path = home.0.join("db");
        let conn = rusqlite::Connection::open(&path).unwrap();
        let session = if modern { "session_v2" } else { "session" };
        let table = if modern { "session_message" } else { "message" };
        conn.execute_batch(&format!("CREATE TABLE {session}(id TEXT,directory TEXT,parent_id TEXT,title TEXT,time_created INTEGER,time_updated INTEGER); CREATE TABLE {table}(id TEXT,session_id TEXT,time_created INTEGER,type TEXT,data TEXT); CREATE TABLE part(id TEXT,message_id TEXT,session_id TEXT,time_created INTEGER,data TEXT); INSERT INTO {session} VALUES ('root','/work',NULL,'Changed title',1000,9000000),('child','/work','root','Child',1001,9999999); INSERT INTO {table} VALUES ('a','root',2000,'user','{{\"role\":\"user\",\"text\":\"First\"}}'),('b','root',3000,'assistant','{{\"role\":\"assistant\",\"content\":\"Reply\"}}'),('c','child',8000000,'user','{{\"role\":\"user\"}}');")).unwrap();
        let before = std::fs::read(&path).unwrap();
        let mut p = Proc::identity(999001, 1, 501, Some(1_000_000), "opencode".into());
        p.cwd = Some("/work".into());
        for agent in [
            opencode::read(&path, &p, "opencode").unwrap().unwrap(),
            kilo::read(&path, &p).unwrap().unwrap(),
        ] {
            assert_eq!(agent.session_id.as_deref(), Some("root"));
            assert_eq!(
                agent.last_activity_at.as_deref(),
                Some("1970-01-01T00:00:03Z")
            );
            assert_eq!(agent.updated_at_ms, Some(9_000_000));
            assert_eq!(agent.title.as_deref(), Some("Changed title"));
        }
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let ro = opencode::open_read_only(&path).unwrap();
        assert!(ro.execute(&format!("DELETE FROM {table}"), []).is_err());
        conn.execute_batch("BEGIN EXCLUSIVE").unwrap();
        assert_eq!(kilo::read(&path, &p), Err("database_read_failed"));
        conn.execute_batch("ROLLBACK").unwrap();
        drop(ro);
        drop(conn);
        std::fs::write(&path, "corrupt PRIVATE_BODY_SENTINEL").unwrap();
        assert_eq!(
            opencode::read(&path, &p, "opencode"),
            Err("database_read_failed")
        );
    }
}
#[test]
fn sqlite_activity_query_uses_only_associated_message_creation_metadata() {
    use whoate_core::sources::opencode::last_activity_ms;
    for table in ["message", "session_message"] {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch(&format!("CREATE TABLE {table}(id TEXT,session_id TEXT,time_created INTEGER); INSERT INTO {table} VALUES ('a','root',1000),('b','root',2000),('c','child',9000),('d','other',10000);")).unwrap();
        // No data/part/session columns even exist: querying any would fail.
        assert_eq!(last_activity_ms(&conn, "root").unwrap(), Some(2000));
        assert_eq!(last_activity_ms(&conn, "empty").unwrap(), None);
    }
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    assert!(last_activity_ms(&conn, "missing").is_err());
}

#[test]
fn pane_escaped_titles_preserve_origin_without_changing_attribution() {
    use whoate_core::{recognizers::ConfigReader, sample::tmux::parse_panes, snapshot::Proc};
    let text = "%42\twork:0.0\t\"Window 雪\"\t999001\t\"/work dir\"\t\"tab\\tline\\n雪\\033\"\n";
    let panes = parse_panes(text).unwrap();
    assert_eq!(panes[0].pane_title.as_deref(), Some("tab\tline\n雪\u{1b}"));
    assert_eq!(panes[0].window_name.as_deref(), Some("Window 雪"));
    let mut snap = Snapshot::historical(
        "2026-10-17T00:00:00Z".into(),
        vec![Proc::identity(999001, 1, 501, Some(1), "shell".into())],
    );
    snap.tmux_panes = panes;
    let home = std::env::temp_dir();
    let owners = whoate_core::attribute::attribute(
        &snap,
        501,
        &ConfigReader::isolated(home),
        &Default::default(),
    );
    assert_eq!(owners[0].id, "pane:%42");
    assert_eq!(owners[0].pids, vec![999001]);
    assert_eq!(
        owners[0].origin.as_ref().unwrap().pane_title.as_deref(),
        Some("tab\tline\n雪\u{1b}")
    );
    assert_eq!(
        owners[0].origin.as_ref().unwrap().window_name.as_deref(),
        Some("Window 雪")
    );
    assert_eq!(
        parse_panes("%1\twork:0.0\told\t9\t/work\n").unwrap()[0].pane_title,
        None
    );
    for bad in [
        "%1\tx\tname\tINJECT\tx\tgood\n",
        "%1\tx\tname\t9\tx\ttitle\textra\n",
    ] {
        assert!(parse_panes(bad).is_err());
    }
}

#[test]
fn pane_tmux_q_a_forms_keep_exact_metadata_and_identity() {
    use whoate_core::sample::tmux::parse_panes;
    // tmux 3.7c quoting forms from review, plus a byte-oriented vis octal case.
    for (encoded, expected) in [
        (r#"'a"b\\\\c'"#, r#"a"b\\c"#),
        (r"C:\\\\dir", r"C:\\dir"),
        (r"a\\\\b", r"a\\b"),
        (r#"'x"y'"#, r#"x"y"#),
        (r#"'é"q'"#, r#"é"q"#),
        (r#"'日本"語'"#, r#"日本"語"#),
        (r"\~", "~"),
        (r#"\""#, "\""),
        (r"\'", "'"),
        (r#""\~/Projects whoate""#, "~/Projects whoate"),
        (r"\~/Projects/whoate", "~/Projects/whoate"),
        (r"\{", "{"),
        (r"\}", "}"),
        (r"\%", "%"),
        ("''", ""),
        ("'say\"hi'", "say\"hi"),
        ("\"'\"", "'"),
        ("\"''\"", "''"),
        (r#""a\$b""#, "a$b"),
        (r#""\303\251""#, "é"),
    ] {
        let panes = parse_panes(&format!(
            "%42\twork:0.0\t{encoded}\t999001\t{encoded}\t{encoded}\n"
        ))
        .unwrap();
        assert_eq!(panes[0].pane_title.as_deref(), Some(expected), "{encoded}");
        assert_eq!(panes[0].window_name.as_deref(), Some(expected), "{encoded}");
        assert_eq!(panes[0].cwd.as_deref(), Some(expected), "{encoded}");
        assert_eq!(
            (&*panes[0].pane_id, &*panes[0].target, panes[0].pane_pid),
            ("%42", "work:0.0", 999001)
        );
    }
}

#[test]
fn pane_bad_metadata_is_missing_without_losing_other_panes_or_attribution() {
    use whoate_core::{recognizers::ConfigReader, sample::tmux::parse_panes, snapshot::Proc};
    // No fuzz infrastructure: truncated/mismatched quoting, invalid octal/UTF-8,
    // repeated escapes and unknown escape forms are explicit hostile cases.
    for bad in [
        r#""bad\""#,
        r#""bad\09x""#,
        "'unfinished",
        r#""\377""#,
        r#""\q""#,
        r#""\"\"#,
    ] {
        let panes = parse_panes(&format!(
            "%42\twork:0.0\t{bad}\t999001\t/work\t{bad}\n%43\twork:0.1\tgood\t999002\t/work\tgood\n"
        ))
        .unwrap();
        assert_eq!(panes.len(), 2);
        assert_eq!(panes[0].pane_title, None);
        assert_eq!(panes[0].window_name, None);
        assert_eq!(panes[1].pane_title.as_deref(), Some("good"));
        let mut snap = Snapshot::historical(
            "2026-10-17T00:00:00Z".into(),
            vec![
                Proc::identity(999001, 1, 501, Some(1), "shell".into()),
                Proc::identity(999002, 1, 501, Some(1), "shell".into()),
            ],
        );
        snap.tmux_panes = panes;
        let owners = whoate_core::attribute::attribute(
            &snap,
            501,
            &ConfigReader::isolated(std::env::temp_dir()),
            &Default::default(),
        );
        assert!(
            owners
                .iter()
                .any(|o| o.id == "pane:%42" && o.pids == [999001])
        );
        assert!(
            owners
                .iter()
                .any(|o| o.id == "pane:%43" && o.pids == [999002])
        );
    }
}
