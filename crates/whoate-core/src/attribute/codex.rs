//! Codex apps (phase 10 R7). A process carrying CODEX_THREAD_ID belongs to its
//! Codex session only when one of these apps is on the parent chain above the
//! topmost process carrying that id (that process itself does not count);
//! anywhere else it is attributed as if the variable were absent.

/// Bundle ids of the Codex app and the CLI bundle inside it. The one place this list lives.
pub const CODEX_BUNDLE_IDS: &[&str] = &["com.openai.codex", "com.openai.codex.cli"];

pub fn is_codex_app(bundle_id: &str) -> bool {
    CODEX_BUNDLE_IDS.contains(&bundle_id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn phase10_codex_app_list_is_chatgpt_and_its_cli_only() {
        assert!(super::is_codex_app("com.openai.codex"));
        assert!(super::is_codex_app("com.openai.codex.cli"));
        for id in [
            "com.openai.chat",
            "com.anthropic.claudefordesktop",
            "codex",
            "",
        ] {
            assert!(!super::is_codex_app(id), "{id}");
        }
    }
}
