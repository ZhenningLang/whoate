//! Phase 11 QA3 (D0 = A): the GUI switches language without rebuilding the engine.
mod common;
use common::Temp;
use std::{collections::VecDeque, time::Duration};
use whoate_core::{
    engine::{Clock, Context, Engine},
    recognizers::ConfigReader,
    sample::{
        SampleResult, Sampler,
        system_memory::{BASIS, KernelZone, KernelZones},
    },
    snapshot::{Freshness, Host, Pressure, Proc, Snapshot},
    verdict::Language,
};

struct Replay(VecDeque<Snapshot>);
impl Sampler for Replay {
    fn sample(&mut self) -> SampleResult<Snapshot> {
        Ok(self.0.pop_front().expect("fixture exhausted"))
    }
}
struct Time;
impl Clock for Time {
    fn now_ns(&self) -> u128 {
        1
    }
    fn wait(&mut self, _: Duration) {}
}

fn cjk(text: &str) -> bool {
    text.chars()
        .any(|c| matches!(c as u32, 0x3000..=0x303F | 0x3400..=0x9FFF | 0xFF00..=0xFFEF))
}
fn strings(value: &serde_json::Value, out: &mut Vec<String>) {
    match value {
        serde_json::Value::String(s) => out.push(s.clone()),
        serde_json::Value::Array(a) => a.iter().for_each(|v| strings(v, out)),
        serde_json::Value::Object(o) => o.iter().for_each(|(k, v)| {
            out.push(k.clone());
            strings(v, out);
        }),
        _ => {}
    }
}

/// The frozen 19:40 capture plus a project, a Claude session and a host whose
/// kernel zone triggers a memory diagnostic: verdicts, facts, parts and host
/// text all carry language-dependent wording.
fn frame(home: &Temp, second: i64) -> Snapshot {
    let mut snap: Snapshot = serde_json::from_str(include_str!(
        "../../../tests/fixtures/capture/snapshot-20261014-1940.json"
    ))
    .unwrap();
    let at = time::OffsetDateTime::from_unix_timestamp(1_760_442_000 + second).unwrap();
    snap.taken_at = at
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let mut node = Proc::identity(999001, 1, 501, Some(1), "node".into());
    node.cwd = Some(home.0.join("project").to_string_lossy().into());
    node.footprint_bytes = Some(1);
    let mut agent = Proc::identity(999002, 1, 501, Some(1), "claude".into());
    agent.footprint_bytes = Some(1);
    snap.procs.extend([node, agent]);
    snap.host = Some(Host {
        mem_total_bytes: 24 << 30,
        mem_used_bytes: 20 << 30,
        compressed_bytes: 2 << 30,
        wired_bytes: 3 << 30,
        swap_used_bytes: 1 << 30,
        swap_total_bytes: None,
        pressure: Pressure::Warn,
        kernel_zones: Some(KernelZones {
            status: Freshness::Fresh,
            as_of: Some("2026-10-17T00:00:00Z".into()),
            basis: BASIS.into(),
            reason: None,
            top: vec![KernelZone {
                name: "data.kalloc.1024".into(),
                bytes: 4_294_967_296,
                collectible: Some(false),
                pageable: Some(false),
            }],
        }),
        gpu_memory: None,
        memory_diagnostics: None,
        cpu: None,
    });
    snap
}

#[test]
fn qa3_set_language_switches_every_text_field_and_keeps_history() {
    let home = Temp::new();
    for (path, body) in [
        ("project/package.json", r#"{"name":"locale-project"}"#),
        (
            ".claude/sessions/999002.json",
            r#"{"sessionId":"locale-session","cwd":"/demo"}"#,
        ),
        (
            ".claude/projects/-demo/locale-session.jsonl",
            "{\"type\":\"user\",\"message\":{\"content\":\"Question\"}}\n",
        ),
    ] {
        let path = home.0.join(path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
    let frames = (0..3).map(|s| frame(&home, s)).collect();
    let mut engine = Engine::with_sampler(
        Replay(frames),
        Time,
        Context {
            cfg: ConfigReader::isolated(home.0.clone()),
            uid: 501,
            language: Language::Zh,
            platform_probe: |_| Ok(false),
        },
    );
    let zh = engine.tick().unwrap();
    let mut before = vec![];
    strings(&serde_json::to_value(&zh).unwrap(), &mut before);
    assert!(
        before.iter().any(|s| cjk(s)),
        "Chinese tick has Chinese text"
    );
    let zh_host = zh
        .host
        .as_ref()
        .unwrap()
        .memory_diagnostics
        .as_ref()
        .unwrap();
    assert!(
        zh_host.iter().any(|v| cjk(&v.text)),
        "host diagnostics in Chinese"
    );

    engine.set_language(Language::En);
    let en = engine.tick().unwrap();
    let mut after = vec![];
    strings(&serde_json::to_value(&en).unwrap(), &mut after);
    // The system-service fact label follows the language too (it was both at once until Phase 11).
    assert!(before.iter().any(|s| s == "系统服务"));
    assert!(after.iter().any(|s| s == "System service"));
    let leftover: Vec<_> = after.iter().filter(|s| cjk(s)).collect();
    assert!(
        leftover.is_empty(),
        "Chinese left after switching: {leftover:?}"
    );
    assert!(
        en.owners
            .iter()
            .flat_map(|o| &o.facts)
            .any(|f| f.key == "project" && f.label == "Project"),
        "fact labels switch"
    );
    assert!(
        !en.host
            .as_ref()
            .unwrap()
            .memory_diagnostics
            .as_ref()
            .unwrap()
            .is_empty()
    );
    for owner in &en.owners {
        let old = zh.owners.iter().find(|o| o.id == owner.id);
        if let Some(old) = old {
            assert!(
                owner.history.len() >= old.history.len(),
                "history kept for {}",
                owner.id
            );
        }
    }
    assert!(
        en.owners.iter().any(|o| o.history.len() == 2),
        "the second tick extends the first one's history"
    );
}
