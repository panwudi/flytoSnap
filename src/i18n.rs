use regex::Regex;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{
        LazyLock,
        atomic::{AtomicU8, Ordering},
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Language {
    #[default]
    #[serde(rename = "zh-CN")]
    Simplified,
    #[serde(rename = "zh-TW")]
    Traditional,
    #[serde(rename = "en")]
    English,
    #[serde(rename = "ja")]
    Japanese,
    #[serde(rename = "ru")]
    Russian,
    #[serde(rename = "fr")]
    French,
    #[serde(rename = "es")]
    Spanish,
    #[serde(rename = "pt")]
    Portuguese,
    #[serde(rename = "de")]
    German,
    #[serde(rename = "ko")]
    Korean,
}
impl Language {
    pub const ALL: [Self; 10] = [
        Self::Simplified,
        Self::Traditional,
        Self::English,
        Self::Japanese,
        Self::Russian,
        Self::French,
        Self::Spanish,
        Self::Portuguese,
        Self::German,
        Self::Korean,
    ];
    pub fn code(self) -> &'static str {
        [
            "zh-CN", "zh-TW", "en", "ja", "ru", "fr", "es", "pt", "de", "ko",
        ][self as usize]
    }
    pub fn native_name(self) -> &'static str {
        [
            "简体中文",
            "繁體中文",
            "English",
            "日本語",
            "Русский",
            "Français",
            "Español",
            "Português",
            "Deutsch",
            "한국어",
        ][self as usize]
    }
}
static LANGUAGE: AtomicU8 = AtomicU8::new(0);
pub fn current() -> Language {
    Language::ALL
        .get(LANGUAGE.load(Ordering::Relaxed) as usize)
        .copied()
        .unwrap_or_default()
}
pub fn set(language: Language) {
    LANGUAGE.store(language as u8, Ordering::Relaxed);
}
pub fn detected(name: &str) -> Language {
    let s = name.to_ascii_lowercase();
    if s.starts_with("zh") {
        if s.contains("hant") || s.contains("tw") || s.contains("hk") {
            Language::Traditional
        } else {
            Language::Simplified
        }
    } else {
        Language::ALL
            .into_iter()
            .find(|language| s.split(['-', '_']).next() == Some(language.code()))
            .unwrap_or(Language::English)
    }
}
pub fn preference(name: &str, system: &str) -> Language {
    Language::ALL
        .into_iter()
        .find(|language| language.code() == name)
        .unwrap_or_else(|| detected(system))
}

type Catalog = BTreeMap<String, BTreeMap<String, String>>;
static CATALOG: LazyLock<Catalog> = LazyLock::new(|| {
    serde_json::from_str(include_str!("../locales/ui.json")).expect("valid translations")
});
struct Template {
    matcher: Regex,
    source: String,
}
static TEMPLATES: LazyLock<Vec<Template>> = LazyLock::new(|| {
    let mut result = vec![];
    for (source, translations) in CATALOG.iter() {
        if !source.contains("{}") {
            continue;
        }
        for text in std::iter::once(source).chain(translations.values()) {
            let pattern = format!("^{}$", regex::escape(text).replace("\\{\\}", "(.*?)"));
            result.push(Template {
                matcher: Regex::new(&pattern).expect("valid template"),
                source: source.clone(),
            });
        }
    }
    result
});
fn translated<'a>(source: &'a str, entry: &'a BTreeMap<String, String>) -> &'a str {
    if current() == Language::Simplified {
        source
    } else {
        entry
            .get(current().code())
            .map(String::as_str)
            .unwrap_or(source)
    }
}
pub fn tr(text: &str) -> String {
    if let Some(entry) = CATALOG.get(text) {
        return translated(text, entry).into();
    }
    for (source, entry) in CATALOG.iter() {
        if entry.values().any(|value| value == text) {
            return translated(source, entry).into();
        }
    }
    for template in TEMPLATES.iter() {
        if let Some(captures) = template.matcher.captures(text) {
            let entry = &CATALOG[&template.source];
            let target = translated(&template.source, entry);
            let mut values = (1..captures.len()).map(|i| captures.get(i).unwrap().as_str());
            let mut output = String::new();
            for (i, part) in target.split("{}").enumerate() {
                if i > 0 {
                    output.push_str(values.next().unwrap_or(""));
                }
                output.push_str(part);
            }
            return output;
        }
    }
    text.into()
}
pub fn language_name(language: Language) -> &'static str {
    language.native_name()
}
pub fn verify_catalog() -> Result<usize, String> {
    for (source, entry) in CATALOG.iter() {
        for key in Language::ALL
            .into_iter()
            .filter(|l| *l != Language::Simplified)
            .map(Language::code)
        {
            let value = entry
                .get(key)
                .ok_or_else(|| format!("Missing {key}: {source}"))?;
            if source.matches("{}").count() != value.matches("{}").count() {
                return Err(format!("Placeholder mismatch: {source}"));
            }
        }
    }
    Ok(CATALOG.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn locale_detection_and_catalog_are_complete() {
        assert_eq!(detected("zh-Hant-HK"), Language::Traditional);
        assert_eq!(detected("zh-Hans-CN"), Language::Simplified);
        assert_eq!(detected("en-US"), Language::English);
        for (name, language) in [
            ("ja-JP", Language::Japanese),
            ("ru-RU", Language::Russian),
            ("fr-CA", Language::French),
            ("es-MX", Language::Spanish),
            ("pt-BR", Language::Portuguese),
            ("de-DE", Language::German),
            ("ko-KR", Language::Korean),
        ] {
            assert_eq!(detected(name), language);
            assert_eq!(preference(language.code(), "en-US"), language);
        }
        assert!(verify_catalog().unwrap() > 100);
    }
}
