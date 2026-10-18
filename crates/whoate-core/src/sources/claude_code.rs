//! Claude's private format is best effort; errors carry safe codes, never text.
use crate::{
    model::{Agent, Confidence},
    recognizers::ConfigReader,
    snapshot::Proc,
};
use serde_json::Value;
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

pub const PREFIX_BYTES: u64 = 256 * 1024;
pub const TAIL_BYTES: u64 = 64 * 1024;

/// Deserialize metadata only. Unknown fields (including message.content) use
/// serde IgnoredAny; this path does not construct a body Value or clean text.
pub fn last_activity_from_tail(
    tail: &[u8],
    truncated_start: bool,
) -> Result<Option<String>, &'static str> {
    #[derive(serde::Deserialize)]
    struct Activity {
        #[serde(rename = "type")]
        kind: Option<String>,
        timestamp: Option<String>,
        #[serde(rename = "isSidechain", default)]
        sidechain: bool,
    }
    if tail.len() > TAIL_BYTES as usize {
        return Err("activity_tail_limit");
    }
    let mut last = None;
    let mut lines = tail.split(|b| *b == b'\n').enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let record = match serde_json::from_slice::<Activity>(line) {
            Ok(record) => record,
            Err(_) if index == 0 && truncated_start => continue,
            Err(_) if lines.peek().is_none() => return Err("activity_pending_last_line"),
            Err(_) => return Err("activity_invalid_line"),
        };
        if record.sidechain || !matches!(record.kind.as_deref(), Some("user" | "assistant")) {
            continue;
        }
        // A later message without a usable timestamp invalidates earlier time.
        last = match record.timestamp {
            Some(at)
                if time::OffsetDateTime::parse(
                    &at,
                    &time::format_description::well_known::Rfc3339,
                )
                .is_ok_and(|t| t.unix_timestamp() >= 0) =>
            {
                Some(at)
            }
            _ => None,
        };
    }
    Ok(last)
}

/// Only text blocks can become a prompt/reply. Wrapped contents are discarded.
pub fn clean_text(value: &Value, limit: usize) -> Option<String> {
    let text = if let Some(s) = value.as_str() {
        s.to_owned()
    } else {
        value
            .as_array()?
            .iter()
            .filter(|v| v["type"] == "text")
            .filter_map(|v| v["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut output = String::new();
    let mut rest = text.as_str();
    let mut hidden: Vec<&str> = vec![];
    // Each step consumes a UTF-8 boundary or the remaining input; malformed tags terminate.
    while let Some(start) = rest.find('<') {
        if hidden.is_empty() {
            output.push_str(&rest[..start]);
        }
        rest = &rest[start..];
        let Some(end) = rest.find('>') else {
            if hidden.is_empty() {
                output.push_str(rest);
            }
            rest = "";
            break;
        };
        let tag = &rest[1..end];
        let closing = tag.starts_with('/');
        let name = tag
            .trim_start_matches('/')
            .split_whitespace()
            .next()
            .unwrap_or("")
            .trim_end_matches('/');
        if matches!(name, "system-reminder" | "pasted_content") {
            if closing {
                if hidden.last() == Some(&name) {
                    hidden.pop();
                }
            } else if !tag.ends_with('/') {
                hidden.push(name);
            }
        } else if hidden.is_empty() {
            output.push_str(&rest[..=end]);
        }
        rest = &rest[end + 1..];
    }
    if hidden.is_empty() {
        output.push_str(rest);
    }
    let output: String = output.trim().chars().take(limit).collect();
    (!output.is_empty()).then_some(output)
}

pub struct ClaudeRead {
    pub agent: Agent,
    pub issues: Vec<&'static str>,
    pub(super) sources: Option<Vec<super::cache::Stamp>>,
}
impl std::fmt::Debug for ClaudeRead {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClaudeRead")
            .field("issues", &self.issues)
            .finish_non_exhaustive()
    }
}

fn string(value: &Value, key: &str) -> Option<String> {
    value[key].as_str().map(str::to_owned)
}

pub fn read(proc: &Proc, cfg: &ConfigReader) -> Result<ClaudeRead, &'static str> {
    if cfg.session_pid().is_some_and(|pid| pid != proc.pid) {
        return Err("session_read_restricted");
    }
    let session = cfg.home.join(format!(".claude/sessions/{}.json", proc.pid));
    let session_stamp = super::cache::Stamp::read(&session);
    let value = cfg.read_json(&session)?;
    let mut agent = Agent::unknown("claude");
    agent.session_id = string(&value, "sessionId");
    agent.name = string(&value, "name");
    agent.name_source = string(&value, "nameSource");
    agent.status = string(&value, "status");
    agent.cwd = string(&value, "cwd").or_else(|| proc.cwd.clone());
    agent.confidence = Confidence::Certain;
    let Some(id) = agent.session_id.as_deref() else {
        return Err("session_id_missing");
    };
    // A session id is a filename, never a path supplied by the source.
    if id.is_empty()
        || !Path::new(id)
            .components()
            .all(|c| matches!(c, std::path::Component::Normal(_)))
        || id.contains('/')
        || id.contains('\\')
    {
        return Err("invalid_session_id");
    }
    let mut issues = vec![];
    let slug: String = agent
        .cwd
        .as_deref()
        .unwrap_or("")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let transcript = cfg
        .home
        .join(".claude/projects")
        .join(slug)
        .join(format!("{id}.jsonl"));
    let mut transcript_stamp = None;
    if !cfg.allows(&transcript) {
        issues.push("transcript_unavailable");
    } else {
        transcript_stamp = super::cache::Stamp::read(&transcript);
        let result = if cfg.session_pid().is_some() {
            // Titles after the first pair are not authorized by this capability.
            // Only the separate session metadata may supply a title here.
            agent.title = string(&value, "title");
            read_limited_transcript(&transcript, &mut agent, &mut issues)
        } else {
            read_transcript(&transcript, &mut agent, &mut issues)
        };
        if let Err(code) = result {
            issues.push(code);
        }
    }
    if !issues.is_empty() {
        agent.confidence = Confidence::Weak;
    }
    let sources = session_stamp
        .zip(transcript_stamp)
        .filter(|(session, transcript)| {
            issues.is_empty() && session.unchanged(cfg) && transcript.unchanged(cfg)
        })
        .map(|(session, transcript)| vec![session, transcript]);
    Ok(ClaudeRead {
        agent,
        issues,
        sources,
    })
}

fn read_limited_transcript(
    path: &Path,
    agent: &mut Agent,
    issues: &mut Vec<&'static str>,
) -> Result<(), &'static str> {
    let mut file = File::open(path).map_err(|_| "transcript_unavailable")?;
    agent.updated_at_ms = file
        .metadata()
        .map_err(|_| "transcript_stat_failed")?
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    read_first_pair(&mut file, agent, issues)
}

fn read_first_pair(
    reader: &mut impl Read,
    agent: &mut Agent,
    issues: &mut Vec<&'static str>,
) -> Result<(), &'static str> {
    let mut remaining = PREFIX_BYTES;
    let (mut user, mut assistant) = (false, false);
    let mut seen = std::collections::BTreeSet::new();
    while remaining > 0 && !(user && assistant) {
        let mut line = Vec::new();
        let mut eof = false;
        while remaining > 0 {
            let mut byte = [0];
            // No BufReader/read-ahead: even the next byte after the first pair
            // may be unauthorized. The normal prefix budget still bounds I/O.
            let count = match reader.read(&mut byte) {
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => return Err("transcript_read_failed"),
                Ok(count) => count,
            };
            if count == 0 {
                eof = true;
                break;
            }
            remaining -= 1;
            if byte[0] == b'\n' {
                break;
            }
            line.push(byte[0]);
        }
        if line.is_empty() {
            if eof {
                break;
            }
            continue;
        }
        let Ok(value) = serde_json::from_slice::<Value>(&line) else {
            if !eof && remaining > 0 {
                issues.push("transcript_invalid_line");
            }
            continue;
        };
        if value["isSidechain"] == true {
            continue;
        }
        if let Some(uuid) = value["uuid"].as_str()
            && !seen.insert(uuid.to_owned())
        {
            continue;
        }
        if value["type"] == "user" && !user {
            user = true;
            agent.first_prompt = clean_text(&value["message"]["content"], 200);
        }
        if value["type"] == "assistant" && !assistant {
            assistant = true;
            agent.first_reply = clean_text(&value["message"]["content"], 120);
            agent.model = string(&value["message"], "model");
        }
        if eof {
            break;
        }
    }
    if !(user && assistant) {
        issues.push("transcript_first_pair_incomplete");
    }
    issues.sort_unstable();
    issues.dedup();
    Ok(())
}

pub fn read_transcript(
    path: &Path,
    agent: &mut Agent,
    issues: &mut Vec<&'static str>,
) -> Result<(), &'static str> {
    let mut file = File::open(path).map_err(|_| "transcript_unavailable")?;
    let metadata = file.metadata().map_err(|_| "transcript_stat_failed")?;
    let len = metadata.len();
    agent.updated_at_ms = metadata
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|d| i64::try_from(d.as_millis()).ok());
    let mut prefix = vec![];
    (&mut file)
        .take(PREFIX_BYTES.min(len))
        .read_to_end(&mut prefix)
        .map_err(|_| "transcript_read_failed")?;
    let mut seen = std::collections::BTreeSet::new();
    let mut lines = prefix.split(|b| *b == b'\n').peekable();
    while let Some(line) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            // A bounded read may end mid-record. Only that final fragment is
            // ignored; complete invalid lines inside the window still report.
            if lines.peek().is_none() {
                continue;
            }
            issues.push("transcript_invalid_line");
            continue;
        };
        if let Some(uuid) = v["uuid"].as_str()
            && !seen.insert(uuid.to_owned())
        {
            continue;
        }
        if v["isSidechain"] == true {
            continue;
        }
        if v["type"] == "user" && agent.first_prompt.is_none() {
            agent.first_prompt = clean_text(&v["message"]["content"], 200);
        }
        if v["type"] == "assistant" && agent.first_reply.is_none() {
            agent.first_reply = clean_text(&v["message"]["content"], 120);
            if agent.model.is_none() {
                agent.model = string(&v["message"], "model");
            }
        }
    }
    let offset = len.saturating_sub(TAIL_BYTES);
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| "transcript_seek_failed")?;
    let mut tail = vec![];
    (&mut file)
        .take(TAIL_BYTES)
        .read_to_end(&mut tail)
        .map_err(|_| "transcript_read_failed")?;
    agent.last_activity_at = match last_activity_from_tail(&tail, offset > 0) {
        Ok(at) => at,
        // Concurrent append fragments are pending, not malformed. Keep the
        // activity unknown without weakening the legacy session confidence.
        Err("activity_pending_last_line") => None,
        Err(code) => {
            // The legacy parser already reports malformed complete records.
            // Reuse its public code rather than duplicating the same failure.
            issues.push(if code == "activity_invalid_line" {
                "transcript_invalid_line"
            } else {
                code
            });
            None
        }
    };
    let mut lines = tail.split(|b| *b == b'\n').enumerate().peekable();
    while let Some((index, line)) = lines.next() {
        if line.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_slice::<Value>(line) else {
            // Only an invalid leading fragment is discarded. A complete JSON
            // record at the exact window boundary must remain visible.
            if offset > 0 && index == 0 {
                continue;
            }
            // A writer can be between writes at EOF. Only its invalid,
            // unterminated final fragment is pending, never a complete bad line.
            if lines.peek().is_none() {
                continue;
            }
            issues.push("transcript_invalid_line");
            continue;
        };
        if let Some(branch) = string(&v, "gitBranch") {
            agent.branch = Some(branch);
        }
        if (v["type"] == "ai-title" || v["type"] == "custom-title")
            && let Some(title) = string(&v, "aiTitle")
                .or_else(|| string(&v, "customTitle"))
                .or_else(|| clean_text(&v["message"]["content"], usize::MAX))
        {
            agent.title = Some(title);
        }
    }
    issues.sort_unstable();
    issues.dedup();
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::{model::Agent, sources::testing::Temp};
    #[test]
    fn phase04_session_first_pair_never_reads_the_next_byte() {
        struct PairOnly {
            data: Vec<u8>,
            offset: usize,
        }
        impl std::io::Read for PairOnly {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                assert!(
                    self.offset + bytes.len() <= self.data.len(),
                    "unauthorized read after first pair or read-ahead"
                );
                bytes.copy_from_slice(&self.data[self.offset..self.offset + bytes.len()]);
                self.offset += bytes.len();
                Ok(bytes.len())
            }
        }
        let data = concat!(
            "{\"type\":\"user\",\"message\":{\"content\":\"Hello\"}}\n",
            "{\"type\":\"assistant\",\"message\":{\"content\":\"World\"}}\n"
        )
        .as_bytes()
        .to_vec();
        let mut reader = PairOnly { data, offset: 0 };
        let mut agent = Agent::unknown("claude");
        super::read_first_pair(&mut reader, &mut agent, &mut vec![]).unwrap();
        assert_eq!(agent.first_prompt.as_deref(), Some("Hello"));
        assert_eq!(agent.first_reply.as_deref(), Some("World"));
    }
    #[test]
    fn phase04_session_first_pair_malformed_and_bounded_inputs_degrade_safely() {
        for data in [
            b"{\"type\":".as_slice(),
            b"\xff\xfe\n",
            b"{}\n{}\n",
            b"{\"type\":\"user\",\"message\":null}\n",
        ] {
            let mut agent = Agent::unknown("claude");
            let mut issues = vec![];
            super::read_first_pair(&mut std::io::Cursor::new(data), &mut agent, &mut issues)
                .unwrap();
            assert!(issues.contains(&"transcript_first_pair_incomplete"));
        }
        struct Endless {
            bytes: u64,
        }
        impl std::io::Read for Endless {
            fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
                out.fill(b'x');
                self.bytes += out.len() as u64;
                Ok(out.len())
            }
        }
        let mut reader = Endless { bytes: 0 };
        super::read_first_pair(&mut reader, &mut Agent::unknown("claude"), &mut vec![]).unwrap();
        assert_eq!(reader.bytes, super::PREFIX_BYTES);
    }
    #[test]
    fn phase02_claude_complete_bad_prefix_line_still_degrades() {
        let t = Temp::new();
        t.write(
            ".claude/sessions/999001.json",
            r#"{"sessionId":"bad-line","cwd":"/demo"}"#,
        );
        t.write(
            ".claude/projects/-demo/bad-line.jsonl",
            "NOT JSON\n{\"type\":\"user\",\"message\":{\"content\":\"Question\"}}\n",
        );
        let p = crate::snapshot::Proc::identity(999001, 1, 501, Some(1), "claude".into());
        let read =
            super::read(&p, &crate::recognizers::ConfigReader::isolated(t.0.clone())).unwrap();
        assert_eq!(read.issues, vec!["transcript_invalid_line"]);
        assert_eq!(read.agent.confidence, crate::model::Confidence::Weak);
    }
    #[test]
    fn phase02_claude_valid_large_transcript_keeps_session_certain() {
        let t = Temp::new();
        t.write(
            ".claude/sessions/999001.json",
            r#"{"sessionId":"long-session","cwd":"/demo"}"#,
        );
        let text = format!(
            "{{\"type\":\"user\",\"message\":{{\"content\":\"First\"}}}}\n{{\"type\":\"progress\",\"data\":\"{}\"}}\n{{\"type\":\"ai-title\",\"aiTitle\":\"Tail title\"}}\n",
            "x".repeat(400 * 1024)
        );
        t.write(".claude/projects/-demo/long-session.jsonl", text);
        let p = crate::snapshot::Proc::identity(999001, 1, 501, Some(1), "claude".into());
        let read =
            super::read(&p, &crate::recognizers::ConfigReader::isolated(t.0.clone())).unwrap();
        assert!(read.issues.is_empty());
        assert_eq!(read.agent.confidence, crate::model::Confidence::Certain);
    }
    #[test]
    fn phase02_claude_invalid_session_ids_are_rejected_before_path_lookup() {
        let t = Temp::new();
        let cfg = crate::recognizers::ConfigReader::isolated(t.0.clone());
        let p = crate::snapshot::Proc::identity(999001, 1, 501, Some(1), "claude".into());
        for id in [
            "",
            ".",
            "..",
            "../escape",
            "/absolute",
            "folder/name",
            "folder\\name",
        ] {
            t.write(
                ".claude/sessions/999001.json",
                serde_json::to_vec(&serde_json::json!({"sessionId":id,"cwd":"/demo"})).unwrap(),
            );
            assert!(
                super::read(&p, &cfg).is_err(),
                "invalid id must not become a shared session: {id:?}"
            );
        }
    }
    #[test]
    fn phase02_claude_tail_window_accepts_complete_first_record() {
        let t = Temp::new();
        let record = "{\"type\":\"ai-title\",\"aiTitle\":\"At exact tail boundary\"}\n";
        let content = format!(
            "{}{}{}",
            "\n".repeat(100),
            record,
            "\n".repeat(super::TAIL_BYTES as usize - record.len())
        );
        let path = t.write("boundary", content);
        let mut agent = Agent::unknown("claude");
        let mut issues = vec![];
        super::read_transcript(&path, &mut agent, &mut issues).unwrap();
        assert_eq!(agent.title.as_deref(), Some("At exact tail boundary"));
    }
    #[test]
    fn phase02_claude_malformed_table_unicode_reply_and_nested_wrappers() {
        for text in [
            "<system-reminder>SECRET",
            "<pasted_content name='x'><system-reminder>SECRET</system-reminder></pasted_content>visible",
            "<system-reminder><pasted_content>SECRET</pasted_content></system-reminder>visible",
        ] {
            let output = super::clean_text(&serde_json::json!(text), 120).unwrap_or_default();
            assert!(!output.contains("SECRET"));
            assert!(output.chars().count() <= 120);
        }
        assert_eq!(
            super::clean_text(&serde_json::json!("答".repeat(200)), 120),
            Some("答".repeat(120))
        );
        let t = Temp::new();
        let malformed:&[&[u8]]=&[
            b"{\"type\":",
            b"\xff\xfe\n",
            b"{\"message\":[]}\n",
            b"{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"tool_result\",\"content\":\"SECRET\"}]}}\n",
        ];
        for (i, bytes) in malformed.iter().enumerate() {
            let path = t.write(&format!("malformed-{i}"), bytes);
            let mut agent = Agent::unknown("claude");
            let mut issues = vec![];
            super::read_transcript(&path, &mut agent, &mut issues).unwrap();
            assert_eq!(agent.first_reply, None);
            assert_eq!(agent.first_prompt, None);
        }
    }
    #[test]
    fn phase02_claude_metadata_name_source_and_real_update_time() {
        let t = Temp::new();
        t.write(".claude/sessions/999001.json",r#"{"sessionId":"demo","name":"Fixture","nameSource":"manual","status":"idle","cwd":"/demo"}"#);
        let path = t.write(
            ".claude/projects/-demo/demo.jsonl",
            "{\"type\":\"user\",\"message\":{\"content\":\"Question\"}}\n",
        );
        let file = std::fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(std::time::UNIX_EPOCH + std::time::Duration::from_secs(1000))
            .unwrap();
        let p = crate::snapshot::Proc::identity(999001, 1, 501, Some(1), "claude".into());
        let read =
            super::read(&p, &crate::recognizers::ConfigReader::isolated(t.0.clone())).unwrap();
        assert_eq!(read.agent.name_source.as_deref(), Some("manual"));
        assert_eq!(read.agent.updated_at_ms, Some(1_000_000));
        assert_eq!(read.agent.status.as_deref(), Some("idle"));
    }
    #[test]
    fn phase02_claude_published_format_sample() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/attribution/published-claude.jsonl");
        let before = std::fs::read(&path).unwrap();
        let temp = Temp::new();
        let path = temp.write("published-format.jsonl", &before);
        let mut agent = Agent::unknown("claude");
        let mut issues = vec![];
        super::read_transcript(&path, &mut agent, &mut issues).unwrap();
        assert_eq!(agent.first_prompt.as_deref(), Some("fix the failing test"));
        assert_eq!(agent.first_reply, None);
        assert_eq!(agent.branch.as_deref(), Some("main"));
        assert!(issues.is_empty());
        assert_eq!(std::fs::read(path).unwrap(), before);
    }
    #[test]
    fn phase02_claude_bounded_windows_skip_middle_and_use_latest_tail() {
        let temp = Temp::new();
        let first = "{\"type\":\"user\",\"message\":{\"content\":\"First\"}}\n";
        let mut text = first.to_owned();
        text.push_str(&" ".repeat(super::PREFIX_BYTES as usize));
        text.push_str("\n{\"type\":\"assistant\",\"message\":{\"content\":\"MIDDLE SENTINEL\"}}\n");
        text.push_str(&" ".repeat(super::TAIL_BYTES as usize));
        text.push_str("\n{\"type\":\"ai-title\",\"aiTitle\":\"Old\",\"gitBranch\":\"old\"}\n{\"type\":\"ai-title\",\"aiTitle\":\"Latest\",\"gitBranch\":\"feature\"}\n");
        let path = temp.write("transcript", &text);
        let mut agent = Agent::unknown("claude");
        let mut issues = vec![];
        super::read_transcript(&path, &mut agent, &mut issues).unwrap();
        assert_eq!(agent.first_prompt.as_deref(), Some("First"));
        assert_eq!(agent.first_reply, None);
        assert_eq!(agent.title.as_deref(), Some("Latest"));
        assert_eq!(agent.branch.as_deref(), Some("feature"));
        assert_eq!(std::fs::read_to_string(path).unwrap(), text);
    }
    #[test]
    fn phase02_claude_text_excludes_tools_wrappers_and_truncates_unicode() {
        let text = serde_json::json!([
            {"type":"tool_result","content":"SECRET"},
            {"type":"text","text":format!("<system-reminder>SECRET</system-reminder>问{}<pasted_content name=\"x\">SECRET</pasted_content>", "答".repeat(240))}
        ]);
        assert_eq!(
            super::clean_text(&text, 200),
            Some(format!("问{}", "答".repeat(199)))
        );
        assert_eq!(
            super::clean_text(
                &serde_json::json!([{ "type":"tool_use", "name":"SECRET"}]),
                120
            ),
            None
        );
    }
}
