use std::{ffi::OsString, process::Command};
pub use whoate_core::verdict::Language;

pub fn resolve(
    mut lookup: impl FnMut(&str) -> Option<OsString>,
    apple: impl FnOnce() -> Option<String>,
) -> Language {
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(raw) = lookup(key) {
            let raw = raw.to_string_lossy();
            let raw = raw.trim();
            if raw.is_empty() || matches!(raw, "C" | "POSIX" | "C.UTF-8" | "C.utf8") {
                continue;
            }
            return language(raw);
        }
    }
    apple()
        .as_deref()
        .and_then(|text| {
            // defaults returns an ordered property-list array; only its first value wins.
            let text = text.trim().strip_prefix('(')?.strip_suffix(')')?;
            let mut values = text.split(',').map(|item| {
                let item = item.trim();
                let item = if item.starts_with('"') {
                    item.strip_prefix('"')?.strip_suffix('"')?
                } else {
                    item
                };
                (!item.is_empty()
                    && item
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_')))
                .then_some(item)
            });
            let first = values.next()??;
            values.all(|item| item.is_some()).then(|| language(first))
        })
        .unwrap_or(Language::En)
}
fn language(value: &str) -> Language {
    if value
        .split(['-', '_', '.'])
        .next()
        .is_some_and(|s| s.eq_ignore_ascii_case("zh"))
    {
        Language::Zh
    } else {
        Language::En
    }
}
pub fn system() -> Language {
    resolve(
        |key| std::env::var_os(key),
        || {
            let result = Command::new("/usr/bin/defaults")
                .args(["read", "-g", "AppleLanguages"])
                .output()
                .ok()?;
            result
                .status
                .success()
                .then(|| String::from_utf8(result.stdout).ok())
                .flatten()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn phase04_locale_malformed_apple_values_fall_back_without_guessing() {
        assert_eq!(
            resolve(|_| None, || Some("(\"zh-Hans\", en)".into())),
            Language::Zh
        );
        for text in ["(", "(\"zh-Hans)", "(zh,)", "((zh))", "(zh,,en)", "(zh,\0)"] {
            assert_eq!(
                resolve(|_| None, || Some(text.into())),
                Language::En,
                "malformed={text:?}"
            );
        }
    }
    #[test]
    fn phase04_locale_priorities_and_fallbacks() {
        for (vars, apple, expected) in [
            (
                vec![("LC_ALL", "en_US"), ("LANG", "zh_CN")],
                Some("(zh-Hans)"),
                Language::En,
            ),
            (
                vec![("LC_MESSAGES", "zh_TW"), ("LANG", "en_US")],
                None,
                Language::Zh,
            ),
            (vec![("LANG", "zh_CN.UTF-8")], Some("(en)"), Language::Zh),
            (
                vec![("LANG", "C")],
                Some("(\n \"zh-Hans\",\n en\n)"),
                Language::Zh,
            ),
            (vec![], Some("(en, zh-Hans)"), Language::En),
            (vec![], Some("broken"), Language::En),
            (vec![], None, Language::En),
            (vec![("LANG", "de_DE")], Some("(zh-Hans)"), Language::En),
        ] {
            assert_eq!(
                resolve(
                    |k| vars
                        .iter()
                        .find(|(name, _)| *name == k)
                        .map(|(_, v)| OsString::from(v)),
                    || apple.map(str::to_owned)
                ),
                expected
            );
        }
    }
}
