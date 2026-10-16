use std::fmt::Write;
use whoate_core::{
    insights::{Insight, Localized},
    model::{FactValue, Report},
    verdict::Language,
};

pub fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_control() || matches!(c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}') {
                '�'
            } else {
                c
            }
        })
        .collect()
}
fn local(value: &Localized, lang: Language) -> &str {
    lang.choose(&value.zh, &value.en)
}
fn cpu(value: Option<f64>, lang: Language) -> String {
    value
        .map(|n| format!("{n:.1}%"))
        .unwrap_or_else(|| lang.choose("未知", "unknown").into())
}
fn fact(value: &FactValue) -> String {
    match value {
        FactValue::Text(s) => safe(s),
        FactValue::Strings(v) => v.iter().map(|s| safe(s)).collect::<Vec<_>>().join(", "),
        FactValue::Count(n) => n.to_string(),
        FactValue::Number(n) => n.to_string(),
        FactValue::Bool(b) => b.to_string(),
    }
}
pub fn insights(entries: &[Insight], lang: Language) -> String {
    let mut out = String::new();
    for entry in entries {
        writeln!(
            out,
            "{}\n  {}\n  {}\n  {} {}",
            safe(&entry.id),
            safe(local(&entry.cause, lang)),
            safe(local(&entry.advice, lang)),
            safe(&entry.source),
            safe(&entry.checked)
        )
        .unwrap();
    }
    out
}
pub fn report(report: &Report, catalog: &[Insight], lang: Language) -> String {
    let mut out = String::new();
    writeln!(out, "whoate  {}", safe(&report.taken_at)).unwrap();
    if let Some(h) = &report.host {
        writeln!(
            out,
            "{}: {} / {} bytes; {}={} bytes; wired={} bytes; swap={} bytes; pressure={:?}",
            lang.choose("内存", "Memory"),
            h.mem_used_bytes,
            h.mem_total_bytes,
            lang.choose("压缩", "compressed"),
            h.compressed_bytes,
            h.wired_bytes,
            h.swap_used_bytes,
            h.pressure
        )
        .unwrap();
    }
    writeln!(
        out,
        "system_procs={:?} as_of={}",
        report.system_procs.status,
        report.system_procs.as_of.as_deref().unwrap_or("unknown")
    )
    .unwrap();
    for owner in &report.owners {
        writeln!(
            out,
            "\n{}  {}  {} bytes  CPU {}  pids={:?}",
            safe(&owner.id),
            safe(&owner.label),
            owner.mem_bytes,
            cpu(owner.cpu_pct, lang),
            owner.pids
        )
        .unwrap();
        writeln!(
            out,
            "  {:?}; unknown_mem_count={}  {:?}: {}",
            owner.kind,
            owner.unknown_mem_count,
            owner.verdict.level,
            safe(&owner.verdict.text)
        )
        .unwrap();
        if let Some(action) = &owner.verdict.action {
            writeln!(out, "  {}", safe(action)).unwrap();
        }
        if let Some(origin) = &owner.origin {
            writeln!(
                out,
                "  pane={} alive={:?} cwd={} ports={:?}",
                safe(origin.tmux_pane.as_deref().unwrap_or("unknown")),
                origin.pane_alive,
                safe(origin.cwd.as_deref().unwrap_or("unknown")),
                origin.ports
            )
            .unwrap();
        }
        if let Some(a) = &owner.agent {
            writeln!(out,"  {}: {} / {}; status={} cwd={} branch={} model={} cost={:?} confidence={:?} stale={}",safe(&a.tool),safe(a.name.as_deref().unwrap_or("unknown")),safe(a.title.as_deref().unwrap_or("unknown")),safe(a.status.as_deref().unwrap_or("unknown")),safe(a.cwd.as_deref().unwrap_or("unknown")),safe(a.branch.as_deref().unwrap_or("unknown")),safe(a.model.as_deref().unwrap_or("unknown")),a.cost_usd,a.confidence,a.stale).unwrap();
            if let Some(s) = &a.first_prompt {
                writeln!(out, "  first_prompt: {}", safe(s)).unwrap();
            }
            if let Some(s) = &a.first_reply {
                writeln!(out, "  first_reply: {}", safe(s)).unwrap();
            }
        }
        for p in &owner.parts {
            writeln!(
                out,
                "  {}: {} bytes, {} {}; pids={:?}; unknown_mem_count={}",
                safe(&p.name),
                p.mem_bytes,
                p.process_count,
                lang.choose("个进程", "processes"),
                p.pids,
                p.unknown_mem_count
            )
            .unwrap();
        }
        for f in &owner.facts {
            let container_memory =
                f.key.starts_with("docker_container.") && f.key.ends_with(".memory_bytes");
            let suffix = if container_memory {
                lang.choose(
                    " bytes（Linux独立统计，不计入macOS内存）",
                    " bytes (independent Linux statistics, excluded from macOS memory)",
                )
            } else {
                ""
            };
            writeln!(
                out,
                "  {} [{}]: {}{}",
                safe(&f.label),
                safe(&f.key),
                fact(&f.value),
                suffix
            )
            .unwrap();
        }
        for e in &owner.evidence {
            writeln!(
                out,
                "  {} [{}]: {}",
                lang.choose("证据", "Evidence"),
                safe(&e.kind),
                safe(&e.detail)
            )
            .unwrap();
        }
        // IDs were attached inside Engine while temporary count signals existed.
        for id in &owner.insights {
            if let Some(entry) = catalog.iter().find(|e| e.id == *id) {
                out.push_str(&insights(std::slice::from_ref(entry), lang));
            }
        }
        if !owner.history.is_empty() {
            writeln!(
                out,
                "  {}: {} {}",
                lang.choose("历史", "History"),
                owner.history.len(),
                lang.choose("个真实采样点", "real sample points")
            )
            .unwrap();
        }
    }
    for group in &report.swarms {
        writeln!(
            out,
            "\n{} [{}] {} bytes; CPU {}; count={}; pids={:?}\n  {}\n  {}: {:?}",
            safe(&group.label),
            safe(&group.signature),
            group.mem_bytes,
            cpu(group.cpu_pct, lang),
            group.count,
            group.pids,
            safe(&group.verdict.text),
            lang.choose("父PID", "Parent PIDs"),
            group.parents
        )
        .unwrap();
        for owner in &group.owners {
            let kind = report
                .owners
                .iter()
                .find(|o| o.id == owner.id)
                .map(|o| format!("{:?}", o.kind))
                .unwrap_or_else(|| lang.choose("视图外归属", "owner outside view").into());
            writeln!(out, "  {} ({kind}): {}", safe(&owner.id), owner.count).unwrap();
        }
    }
    for issue in &report.issues {
        writeln!(
            out,
            "  {}: {} {} pid={:?}",
            lang.choose("采集问题", "Sample issue"),
            safe(&issue.stage),
            safe(&issue.code),
            issue.pid
        )
        .unwrap();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase04_render_unknown_cpu_control_characters_and_catalog_are_readable() {
        let mut r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        r.owners[0].label = "label\x1b[31m\nINJECT".into();
        r.owners[0].cpu_pct = None;
        r.owners[0].facts.push(whoate_core::model::Fact {
            key: "docker_container.example.memory_bytes".into(),
            label: "container".into(),
            value: FactValue::Count(123),
        });
        let entries = whoate_core::insights::all();
        r.owners[0].insights = vec![entries[0].id.clone()];
        for (language, unknown, source) in [
            (Language::En, "CPU unknown", "independent Linux"),
            (Language::Zh, "CPU 未知", "Linux独立统计"),
        ] {
            let text = report(&r, &entries, language);
            assert!(text.contains(unknown));
            assert!(text.contains(source));
            assert!(text.contains(&entries[0].source));
            assert!(text.contains(&entries[0].checked));
            assert!(!text.contains('\x1b'));
            assert!(!text.contains("\nINJECT"));
            assert!(!text.contains('│'));
            assert!(text.contains("MCP: demo-service"));
            assert!(text.contains("900001"));
        }
    }
    #[test]
    fn phase04_insights_list_contains_exact_catalog_ids_and_dates() {
        let entries = whoate_core::insights::all();
        let text = insights(&entries, Language::En);
        for e in &entries {
            assert!(text.contains(&format!("{}\n", e.id)));
            assert!(text.contains(&e.source));
            assert!(text.contains(&e.checked));
            assert!(text.contains(&e.advice.en));
        }
    }
}
