mod common;
use common::Temp;
use std::{fs, io::Write, sync::mpsc};
use whoate_core::{
    model::Confidence, recognizers::ConfigReader, snapshot::Proc, sources::claude_code,
};

#[test]
fn phase03_transcript_append_handshake_waits_for_invalid_fragment_then_recovers() {
    let home = Temp::new();
    let session = home.0.join(".claude/sessions");
    let project = home.0.join(".claude/projects/-demo");
    fs::create_dir_all(&session).unwrap();
    fs::create_dir_all(&project).unwrap();
    fs::write(
        session.join("930001.json"),
        r#"{"sessionId":"append","cwd":"/demo"}"#,
    )
    .unwrap();
    let path = project.join("append.jsonl");
    fs::write(&path, "{\"type\":\"user\",\"message\":{\"content\":\"first question\"}}\n{\"type\":\"ai-title\",\"aiTitle\":\"").unwrap();
    let original = fs::read(&path).unwrap();
    let proc = Proc::identity(930001, 1, 501, Some(1), "claude".into());
    let cfg = ConfigReader::isolated(home.0.clone());
    let (resume_tx, resume_rx) = mpsc::channel();
    let (done_tx, done_rx) = mpsc::channel();
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        resume_rx.recv().unwrap();
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(writer_path)
            .unwrap();
        f.write_all(b"finished title\"}").unwrap();
        f.flush().unwrap();
        done_tx.send(()).unwrap();
    });
    let read = claude_code::read(&proc, &cfg).unwrap();
    assert_eq!(read.agent.first_prompt.as_deref(), Some("first question"));
    assert_eq!(read.agent.title, None);
    assert_eq!(read.issues, Vec::<&str>::new());
    assert_eq!(read.agent.confidence, Confidence::Certain);
    assert_eq!(fs::read(&path).unwrap(), original);
    resume_tx.send(()).unwrap();
    done_rx.recv().unwrap();
    writer.join().unwrap();
    let read = claude_code::read(&proc, &cfg).unwrap();
    assert_eq!(read.agent.title.as_deref(), Some("finished title"));
    assert_eq!(read.agent.confidence, Confidence::Certain);
    assert_eq!(read.issues, Vec::<&str>::new());
    let mut f = fs::OpenOptions::new().append(true).open(&path).unwrap();
    f.write_all(b"\nnot-json\n").unwrap();
    f.flush().unwrap();
    let read = claude_code::read(&proc, &cfg).unwrap();
    assert_eq!(read.issues, vec!["transcript_invalid_line"]);
    assert_eq!(read.agent.confidence, Confidence::Weak);
}

#[test]
fn phase03_transcript_complete_first_message_without_newline_is_valid() {
    let home = Temp::new();
    let path = home.0.join("complete.jsonl");
    fs::write(
        &path,
        r#"{"type":"user","message":{"content":"no newline"}}"#,
    )
    .unwrap();
    let mut agent = whoate_core::model::Agent::unknown("claude");
    let mut issues = vec![];
    claude_code::read_transcript(&path, &mut agent, &mut issues).unwrap();
    assert_eq!(agent.first_prompt.as_deref(), Some("no newline"));
    assert_eq!(issues, Vec::<&str>::new());
}

#[test]
fn phase03_transcript_bounded_windows_do_not_read_middle_or_swallow_complete_bad_lines() {
    use whoate_core::model::Agent;
    let home = Temp::new();
    let path = home.0.join("windows.jsonl");
    let prefix = format!("{{\"padding\":\"{}\"}}\n", "p".repeat(270000));
    let suffix = format!("{{\"padding\":\"{}\"}}\n", "s".repeat(200000));
    let text = format!(
        "{prefix}{{\"type\":\"user\",\"message\":{{\"content\":\"middle must not be read\"}}}}\n{suffix}{{\"type\":\"ai-title\",\"aiTitle\":\"visible tail\"}}"
    );
    fs::write(&path, &text).unwrap();
    let modified = fs::metadata(&path).unwrap().modified().unwrap();
    let mut agent = Agent::unknown("claude");
    let mut issues = vec![];
    claude_code::read_transcript(&path, &mut agent, &mut issues).unwrap();
    assert_eq!(agent.first_prompt, None);
    assert_eq!(agent.title.as_deref(), Some("visible tail"));
    assert!(issues.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
    assert_eq!(fs::metadata(&path).unwrap().modified().unwrap(), modified);
    let mut file = fs::OpenOptions::new().append(true).open(&path).unwrap();
    file.write_all(b"\n{}\ncomplete-invalid-line\n{\"type\":\"ai-title\",\"aiTitle\":\"")
        .unwrap();
    file.flush().unwrap();
    let mut agent = Agent::unknown("claude");
    let mut issues = vec![];
    claude_code::read_transcript(&path, &mut agent, &mut issues).unwrap();
    assert_eq!(issues, vec!["transcript_invalid_line"]);
    assert_eq!(agent.title.as_deref(), Some("visible tail"));
    file.write_all(b"completed next title\"}").unwrap();
    file.flush().unwrap();
    let mut agent = Agent::unknown("claude");
    let mut issues = vec![];
    claude_code::read_transcript(&path, &mut agent, &mut issues).unwrap();
    assert_eq!(agent.title.as_deref(), Some("completed next title"));
    assert_eq!(issues, vec!["transcript_invalid_line"]);
}

#[test]
fn phase03_transcript_prompt_and_reply_keep_unicode_limits_and_prefix_faults_visible() {
    use whoate_core::model::Agent;
    let home = Temp::new();
    let path = home.0.join("limits.jsonl");
    let first = serde_json::json!({"type":"user", "message":{"content":"问".repeat(250)}});
    let second = serde_json::json!({"type":"assistant", "message":{"content":"答".repeat(160)}});
    fs::write(
        &path,
        format!("{first}\n{second}\ninvalid complete prefix line\n"),
    )
    .unwrap();
    let mut agent = Agent::unknown("claude");
    let mut issues = vec![];
    claude_code::read_transcript(&path, &mut agent, &mut issues).unwrap();
    assert_eq!(
        agent.first_prompt.as_deref(),
        Some("问".repeat(200).as_str())
    );
    assert_eq!(
        agent.first_reply.as_deref(),
        Some("答".repeat(120).as_str())
    );
    assert_eq!(issues, vec!["transcript_invalid_line"]);
}
