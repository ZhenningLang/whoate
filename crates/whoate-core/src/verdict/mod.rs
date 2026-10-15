pub mod rules;

#[path = "../i18n.rs"]
mod i18n;
pub use i18n::Language;

#[cfg(test)]
mod language_tests;
