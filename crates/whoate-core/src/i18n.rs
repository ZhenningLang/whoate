#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    Zh,
    En,
}
impl Language {
    pub fn from_locale(locale: &str) -> Self {
        if locale.trim().to_ascii_lowercase().starts_with("zh") {
            Self::Zh
        } else {
            Self::En
        }
    }
    pub fn system() -> Self {
        for key in ["LC_ALL", "LC_MESSAGES"] {
            if let Ok(value) = std::env::var(key)
                && !value.is_empty()
            {
                return Self::from_locale(&value);
            }
        }
        if let Ok(output) = std::process::Command::new("/usr/bin/defaults")
            .args(["read", "-g", "AppleLanguages"])
            .output()
            && output.status.success()
            && let Some(locale) = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|s| s.trim().trim_matches(['"', ',', ' ']))
                .find(|s| !s.is_empty() && *s != "(" && *s != ")")
        {
            return Self::from_locale(locale);
        }
        Self::from_locale(&std::env::var("LANG").unwrap_or_else(|_| "en".into()))
    }
    pub fn choose<'a>(self, zh: &'a str, en: &'a str) -> &'a str {
        match self {
            Self::Zh => zh,
            Self::En => en,
        }
    }
}
