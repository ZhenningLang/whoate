//! Terminal applications. For a process adopted by launchd whose responsible
//! process is one of these, rule ④ (owner ruling D1 = C) applies the orphan
//! rule: a dev runtime working in HOME is an orphan from a terminal; the
//! terminal's own XPC services and daemons stay with the terminal app.

/// Bundle ids of terminal emulators. The one place this list lives.
pub const TERMINAL_BUNDLE_IDS: &[&str] = &[
    "com.apple.Terminal",
    "com.mitchellh.ghostty",
    "com.googlecode.iterm2",
    "com.github.wez.wezterm",
    "net.kovidgoyal.kitty",
    "org.alacritty",
    "dev.warp.Warp-Stable",
    "co.zeit.hyper",
    "com.raphaelamorim.rio",
    "io.tabby",
];

pub fn is_terminal(bundle_id: &str) -> bool {
    TERMINAL_BUNDLE_IDS.contains(&bundle_id)
}

#[cfg(test)]
mod tests {
    #[test]
    fn phase10_terminal_list_covers_common_emulators_and_nothing_else() {
        for id in [
            "com.apple.Terminal",
            "com.mitchellh.ghostty",
            "com.googlecode.iterm2",
            "com.github.wez.wezterm",
            "net.kovidgoyal.kitty",
            "org.alacritty",
        ] {
            assert!(super::is_terminal(id), "{id}");
        }
        for id in [
            "com.openai.codex",
            "com.google.Chrome",
            "com.microsoft.VSCode",
            "",
        ] {
            assert!(!super::is_terminal(id), "{id}");
        }
        let mut sorted = super::TERMINAL_BUNDLE_IDS.to_vec();
        sorted.sort_unstable();
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            super::TERMINAL_BUNDLE_IDS.len(),
            "no duplicates"
        );
    }
}
