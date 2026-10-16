use std::collections::BTreeSet;
use whoate_core::{
    model::{FactValue, Report},
    snapshot::Snapshot,
};

#[derive(Clone, Copy, Default)]
pub struct Policy {
    pub prompts: bool,
    pub redact: bool,
}

pub fn project(input: &Report, snapshot: &Snapshot, policy: Policy) -> Report {
    let mut taints = BTreeSet::new();
    for owner in &input.owners {
        if let Some(agent) = &owner.agent {
            taints.extend(
                [&agent.first_prompt, &agent.first_reply]
                    .into_iter()
                    .flatten()
                    .filter(|s| !s.is_empty())
                    .cloned(),
            );
        }
    }
    let prompt_taints = taints.clone();
    // Default output hides prompt-derived content, not locating metadata that
    // merely collides with another process's argv (including single letters).
    for arg in snapshot
        .procs
        .iter()
        .filter(|_| policy.redact)
        .flat_map(|p| p.argv.iter().skip(1))
    {
        let value = if arg.starts_with("--") {
            let Some((key, value)) = arg.split_once('=') else {
                continue;
            };
            if matches!(
                key,
                "--type" | "--utility-sub-type" | "--max-old-space-size"
            ) {
                continue;
            }
            value
        } else {
            arg
        };
        if !value.is_empty()
            && !matches!(
                value,
                "vite"
                    | "next"
                    | "tsserver.js"
                    | "webpack"
                    | "wrangler"
                    | "dev"
                    | "-m"
                    | "http.server"
                    | "ipykernel_launcher"
            )
        {
            taints.insert(value.to_owned());
        }
    }
    // Longest first avoids exposing a suffix when two untrusted inputs overlap.
    let mut taints: Vec<_> = taints.into_iter().collect();
    taints.sort_by_key(|s| std::cmp::Reverse(s.len()));
    let clean = |value: &mut String| {
        if taints.iter().any(|s| value.contains(s)) {
            *value = "<argument>".into();
        }
        if policy.redact && (value.contains("/Users/") || value.starts_with('~')) {
            *value = "~/<private>".into();
        }
    };
    let mut output = input.clone();
    for owner in &mut output.owners {
        if owner.agent.is_none() {
            clean(&mut owner.label);
        }
        if let Some(origin) = &mut owner.origin
            && let Some(cwd) = &mut origin.cwd
            && policy.redact
        {
            // Locating metadata remains visible by default, even when the same
            // path also occurs as a positional argument (e.g. Code <project>).
            *cwd = "~/<private>".into();
        }
        if let Some(agent) = &mut owner.agent {
            if !policy.prompts || policy.redact {
                agent.first_prompt = None;
                agent.first_reply = None;
            }
            if policy.redact {
                for value in [&mut agent.name, &mut agent.title] {
                    if value.is_some() {
                        *value = Some("<redacted>".into());
                    }
                }
                owner.label = agent.tool.clone();
                if let Some(cwd) = &mut agent.cwd {
                    *cwd = "~/<private>".into();
                }
            }
            for text in [&mut agent.branch, &mut agent.model, &mut agent.status]
                .into_iter()
                .flatten()
            {
                clean(text);
            }
        }
        for evidence in &mut owner.evidence {
            clean(&mut evidence.detail);
        }
        for part in &mut owner.parts {
            clean(&mut part.name);
        }
        for fact in &mut owner.facts {
            clean(&mut fact.label);
            match &mut fact.value {
                FactValue::Text(text) => clean(text),
                FactValue::Strings(values) => {
                    for text in values {
                        clean(text);
                    }
                }
                _ => {}
            }
            if policy.redact && fact.key == "venv" {
                fact.value = FactValue::Text("~/<private>".into());
            }
        }
        // Owner rules format trusted resource/state values, never argv. Keep
        // prompt-leak protection without treating coincidental digits as argv.
        if prompt_taints.iter().any(|s| owner.verdict.text.contains(s)) {
            owner.verdict.text = "<argument>".into();
        }
        if let Some(action) = &mut owner.verdict.action {
            *action = format!(
                "whoate kill {}",
                owner
                    .pids
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
    }
    for swarm in &mut output.swarms {
        // An opaque identity preserves grouping computed from original arguments,
        // without exporting the argument-bearing core signature. Not a security hash.
        let hash = swarm
            .signature
            .bytes()
            .fold(0xcbf29ce484222325_u64, |hash, byte| {
                (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3)
            });
        swarm.signature = format!("swarm:{hash:016x}");
        clean(&mut swarm.label);
        clean(&mut swarm.verdict.text);
        if let Some(action) = &mut swarm.verdict.action {
            *action = format!(
                "whoate kill {}",
                swarm
                    .pids
                    .iter()
                    .map(i32::to_string)
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        for evidence in &mut swarm.evidence {
            clean(&mut evidence.detail);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use whoate_core::model::*;
    #[test]
    fn phase04_privacy_default_short_argv_preserves_generated_locating_fields() {
        let mut report: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        report.owners[0].agent = None;
        report.owners[0].label = "Karabiner-Core-Service".into();
        report.owners[0].evidence = vec![
            Evidence::new("service_fallback", "launchd ancestry", Confidence::Certain),
            Evidence::new("other_uid", "0", Confidence::Certain),
            Evidence::new("pane_ancestor", "%3688", Confidence::Certain),
        ];
        let mut proc =
            whoate_core::snapshot::Proc::identity(940099, 1, 501, Some(1), "unrelated".into());
        proc.argv = [
            "unrelated",
            "e",
            "a",
            "0",
            "1",
            "%",
            "3",
            "--token=ARGUMENT_VALUE",
        ]
        .map(str::to_owned)
        .into();
        let snapshot = Snapshot::historical(report.taken_at.clone(), vec![proc]);
        // Group signatures always use an opaque identity, independently of redact.
        let mut expected = report.clone();
        expected.swarms[0].signature = "swarm:6d4d41e5e6ec65a3".into();
        assert_eq!(project(&report, &snapshot, Policy::default()), expected);
    }
    #[test]
    fn phase04_privacy_numeric_argv_cannot_erase_generated_resource_diagnostics() {
        let mut report: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        let owner = &mut report.owners[0];
        owner.kind = OwnerKind::Orphan;
        owner.agent = None;
        owner.mem_bytes = 1024 * 1024 * 1024;
        owner.origin.as_mut().unwrap().pane_alive = Some(false);
        let mut p =
            whoate_core::snapshot::Proc::identity(owner.pids[0], 1, 501, Some(1), "demo".into());
        p.argv = vec!["demo".into(), "1".into()];
        let snapshot = Snapshot::historical(report.taken_at.clone(), vec![p]);
        owner.verdict = whoate_core::verdict::rules::owner(
            owner,
            &snapshot,
            &whoate_core::history::History::default(),
            false,
            false,
            whoate_core::verdict::Language::En,
        );
        let expected = "Its launching pane is closed (duration unknown); using 1.00 GiB";
        assert_eq!(owner.verdict.text, expected);
        let output = project(&report, &snapshot, Policy::default());
        assert_eq!(output.owners[0].verdict.text, expected);
        assert!(
            crate::render::report(&output, &[], whoate_core::verdict::Language::En)
                .contains(expected)
        );
    }
    #[test]
    fn phase04_privacy_default_preserves_cwd_used_as_a_positional_argument() {
        let mut r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        let cwd = "/Users/example/Projects/locating-metadata";
        r.owners[0].origin.as_mut().unwrap().cwd = Some(cwd.into());
        r.owners[0].agent.as_mut().unwrap().cwd = Some(cwd.into());
        let mut snapshot = Snapshot::historical(r.taken_at.clone(), vec![]);
        let mut proc =
            whoate_core::snapshot::Proc::identity(940001, 1, 501, Some(1), "Code".into());
        proc.argv = vec!["Code".into(), cwd.into()];
        snapshot.procs.push(proc);
        let output = project(&r, &snapshot, Policy::default());
        assert_eq!(
            output.owners[0].origin.as_ref().unwrap().cwd.as_deref(),
            Some(cwd)
        );
        assert_eq!(
            output.owners[0].agent.as_ref().unwrap().cwd.as_deref(),
            Some(cwd)
        );
        let redacted = project(
            &r,
            &snapshot,
            Policy {
                prompts: false,
                redact: true,
            },
        );
        assert_eq!(
            redacted.owners[0].origin.as_ref().unwrap().cwd.as_deref(),
            Some("~/<private>")
        );
        assert_eq!(
            redacted.owners[0].agent.as_ref().unwrap().cwd.as_deref(),
            Some("~/<private>")
        );
    }
    #[test]
    fn phase04_privacy_default_and_prompts_only_authorize_two_fields() {
        let mut r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        let snap = Snapshot::historical(r.taken_at.clone(), vec![]);
        let original = r.clone();
        let output = project(&r, &snap, Policy::default());
        assert_eq!(output.owners[0].agent.as_ref().unwrap().first_prompt, None);
        assert_eq!(output.owners[0].agent.as_ref().unwrap().first_reply, None);
        assert_eq!(
            output.owners[0].agent.as_ref().unwrap().name,
            original.owners[0].agent.as_ref().unwrap().name
        );
        assert_eq!(r, original);
        r.swarms[0].signature = r#"["node","--prompt=SECRET_FIRST_QUESTION"]"#.into();
        r.owners[0].agent.as_mut().unwrap().first_prompt = Some("SECRET_FIRST_QUESTION".into());
        r.owners[0].facts.push(Fact {
            key: "project".into(),
            label: "Project".into(),
            value: FactValue::Text("SECRET_FIRST_QUESTION".into()),
        });
        r.owners[0].evidence[0].detail = "SECRET_FIRST_QUESTION".into();
        r.owners[0].parts[0].name = "SECRET_FIRST_QUESTION".into();
        r.owners[0].verdict.text = "SECRET_FIRST_QUESTION".into();
        let output = project(&r, &snap, Policy::default());
        assert!(
            !serde_json::to_string(&output)
                .unwrap()
                .contains("SECRET_FIRST_QUESTION")
        );
        let mut output = project(
            &r,
            &snap,
            Policy {
                prompts: true,
                redact: false,
            },
        );
        assert_eq!(
            output.owners[0]
                .agent
                .as_ref()
                .unwrap()
                .first_prompt
                .as_deref(),
            Some("SECRET_FIRST_QUESTION")
        );
        output.owners[0].agent.as_mut().unwrap().first_prompt = None;
        assert!(
            !serde_json::to_string(&output)
                .unwrap()
                .contains("SECRET_FIRST_QUESTION")
        );
    }
    #[test]
    fn phase04_privacy_redact_preserves_keys_resources_and_identity() {
        let r: Report = serde_json::from_str(include_str!(
            "../../../tests/fixtures/attribution/sample-v1.json"
        ))
        .unwrap();
        let snap = Snapshot::historical(r.taken_at.clone(), vec![]);
        let output = project(
            &r,
            &snap,
            Policy {
                prompts: false,
                redact: true,
            },
        );
        let a = output.owners[0].agent.as_ref().unwrap();
        assert_eq!(a.name.as_deref(), Some("<redacted>"));
        assert_eq!(a.title.as_deref(), Some("<redacted>"));
        assert_eq!(output.owners[0].pids, r.owners[0].pids);
        assert_eq!(output.owners[0].mem_bytes, r.owners[0].mem_bytes);
        assert_eq!(output.owners[0].id, r.owners[0].id);
    }
}
