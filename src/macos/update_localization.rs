use crate::i18n::{self, Language};
use objc2_app_kit::{NSApplication, NSButton, NSTextField, NSView, NSWindow};
use objc2_foundation::{
    NSBundle, NSData, NSDictionary, NSPropertyListMutabilityOptions, NSPropertyListSerialization,
    NSString,
};
use regex::{Captures, Regex};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    sync::LazyLock,
};

static FORMAT: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"%(?:(\d+)\$)?(?:@|(?:ll|l)?[diu]|(?:\.\d+)?f|%)").unwrap());

struct Template {
    key: String,
    pattern: Regex,
    arguments: Vec<usize>,
}

struct Catalog {
    strings: BTreeMap<String, BTreeMap<String, String>>,
    reverse: BTreeMap<String, String>,
    templates: Vec<Template>,
}

thread_local! {static CATALOG: RefCell<Option<Catalog>> = const {RefCell::new(None)};}

fn resource_locale(language: Language) -> &'static str {
    match language {
        Language::Simplified => "zh_CN",
        Language::Traditional => "zh_TW",
        Language::English => "Base",
        Language::Portuguese => "pt-BR",
        _ => language.code(),
    }
}

fn format_text(template: &str, arguments: &[String]) -> String {
    let mut next = 0;
    FORMAT
        .replace_all(template, |captures: &Captures<'_>| {
            if &captures[0] == "%%" {
                return "%".into();
            }
            let index = captures
                .get(1)
                .and_then(|n| n.as_str().parse::<usize>().ok())
                .map(|i| i.saturating_sub(1))
                .unwrap_or_else(|| {
                    let i = next;
                    next += 1;
                    i
                });
            arguments
                .get(index)
                .cloned()
                .unwrap_or_else(|| captures[0].into())
        })
        .into_owned()
}

fn compile_template(key: &str, value: &str) -> Option<Template> {
    let mut pattern = String::from("(?s)^");
    let mut arguments = vec![];
    let mut next = 0;
    let mut end = 0;
    for captures in FORMAT.captures_iter(value) {
        let token = captures.get(0)?;
        pattern.push_str(&regex::escape(&value[end..token.start()]));
        if token.as_str() == "%%" {
            pattern.push('%');
        } else {
            let index = captures
                .get(1)
                .and_then(|n| n.as_str().parse::<usize>().ok())
                .map(|i| i.saturating_sub(1))
                .unwrap_or_else(|| {
                    let i = next;
                    next += 1;
                    i
                });
            arguments.push(index);
            pattern.push_str("(.*?)");
        }
        end = token.end();
    }
    if arguments.is_empty() {
        return None;
    }
    pattern.push_str(&regex::escape(&value[end..]));
    pattern.push('$');
    Some(Template {
        key: key.into(),
        pattern: Regex::new(&pattern).ok()?,
        arguments,
    })
}

impl Catalog {
    fn load(bundle: &NSBundle) -> Result<Self, String> {
        let root = bundle
            .resourcePath()
            .ok_or("Sparkle resources are missing")?
            .to_string();
        let mut locales = Language::ALL
            .into_iter()
            .map(|language| resource_locale(language).to_string())
            .collect::<BTreeSet<_>>();
        locales.extend(
            bundle
                .preferredLocalizations()
                .iter()
                .map(|name| name.to_string()),
        );
        let overrides: BTreeMap<String, BTreeMap<String, String>> =
            serde_json::from_str(include_str!("../../locales/updater-overrides.json"))
                .map_err(|e| e.to_string())?;
        let mut strings = BTreeMap::new();
        let mut reverse = BTreeMap::new();
        let mut templates = vec![];
        let mut seen = BTreeSet::new();
        for locale in locales {
            let path = NSString::from_str(&format!("{root}/{locale}.lproj/Sparkle.strings"));
            let Some(data) = NSData::dataWithContentsOfFile(&path) else {
                continue;
            };
            let object = unsafe {
                NSPropertyListSerialization::propertyListWithData_options_format_error(
                    &data,
                    NSPropertyListMutabilityOptions::Immutable,
                    std::ptr::null_mut(),
                )
            }
            .map_err(|e| e.localizedDescription().to_string())?;
            let dictionary = object
                .downcast::<NSDictionary>()
                .map_err(|_| "Invalid Sparkle strings dictionary")?;
            let mut translated = BTreeMap::new();
            for key in dictionary.allKeys() {
                let Some(value) = dictionary
                    .objectForKey(&key)
                    .and_then(|o| o.downcast::<NSString>().ok())
                else {
                    continue;
                };
                let Ok(key) = key.downcast::<NSString>() else {
                    continue;
                };
                let key = key.to_string();
                let value = value.to_string();
                translated.insert(key, value);
            }
            if let Some(language) = Language::ALL
                .into_iter()
                .find(|language| resource_locale(*language) == locale)
            {
                for (key, values) in &overrides {
                    if let Some(value) = values.get(language.code()) {
                        translated.insert(key.clone(), value.clone());
                    }
                }
            }
            for (key, value) in &translated {
                for source in [key, value] {
                    reverse.entry(source.clone()).or_insert_with(|| key.clone());
                    if seen.insert(source.clone()) {
                        if let Some(template) = compile_template(key, source) {
                            templates.push(template);
                        }
                    }
                }
            }
            strings.insert(locale, translated);
        }
        for language in Language::ALL {
            if !strings.contains_key(resource_locale(language)) {
                return Err(format!("Missing Sparkle locale: {}", language.code()));
            }
        }
        for language in Language::ALL {
            for key in strings["Base"].keys() {
                if !strings[resource_locale(language)].contains_key(key) {
                    return Err(format!(
                        "Missing updater translation: {} / {key}",
                        language.code()
                    ));
                }
            }
        }
        Ok(Self {
            strings,
            reverse,
            templates,
        })
    }

    fn translate(&self, text: &str, language: Language) -> String {
        let target = &self.strings[resource_locale(language)];
        if let Some(key) = self.reverse.get(text) {
            return target.get(key).cloned().unwrap_or_else(|| text.into());
        }
        for template in &self.templates {
            let Some(captures) = template.pattern.captures(text) else {
                continue;
            };
            let Some(translated) = target.get(&template.key) else {
                continue;
            };
            let count = template.arguments.iter().max().copied().unwrap_or(0) + 1;
            let mut arguments = vec![String::new(); count];
            for (capture, index) in template.arguments.iter().enumerate() {
                arguments[*index] = captures[capture + 1].into();
            }
            return format_text(translated, &arguments);
        }
        text.into()
    }
}

pub fn initialize(bundle: &NSBundle) -> Result<(), String> {
    let catalog = Catalog::load(bundle)?;
    CATALOG.with(|slot| *slot.borrow_mut() = Some(catalog));
    Ok(())
}

fn translate(text: &str) -> String {
    CATALOG.with(|slot| {
        slot.borrow()
            .as_ref()
            .map(|catalog| catalog.translate(text, i18n::current()))
            .unwrap_or_else(|| text.into())
    })
}

fn localize_view(view: &NSView) {
    for child in view.subviews() {
        if let Ok(button) = child.clone().downcast::<NSButton>() {
            let old = button.title().to_string();
            let new = translate(&old);
            if new != old {
                button.setTitle(&NSString::from_str(&new));
            }
        } else if let Ok(label) = child.clone().downcast::<NSTextField>() {
            if !label.isEditable() {
                let old = label.stringValue().to_string();
                let new = translate(&old);
                if new != old {
                    label.setStringValue(&NSString::from_str(&new));
                }
            }
        }
        localize_view(&child);
    }
}

pub fn localize_window(window: &NSWindow) {
    let class = window.class().name().to_string_lossy();
    let controller = window
        .windowController()
        .map(|controller| controller.class().name().to_string_lossy().into_owned())
        .unwrap_or_default();
    if !(controller.starts_with("SU") || controller.starts_with("SPU") || class.contains("Alert")) {
        return;
    }
    let old = window.title().to_string();
    let new = translate(&old);
    if new != old {
        window.setTitle(&NSString::from_str(&new));
    }
    if let Some(view) = window.contentView() {
        localize_view(&view);
    }
}

pub fn localize_windows() {
    let Some(mtm) = objc2_foundation::MainThreadMarker::new() else {
        return;
    };
    for window in NSApplication::sharedApplication(mtm).windows() {
        if window.isVisible() {
            localize_window(&window);
        }
    }
}

pub fn verify() -> Result<(), String> {
    CATALOG.with(|slot| {
        let slot=slot.borrow();let catalog=slot.as_ref().ok_or("Update localization is not initialized")?;
        for language in Language::ALL {
            for key in ["Install Update","Skip This Version","Remind Me Later","Cancel"] {
                let actual=catalog.translate(key,language);
                let expected=catalog.strings[resource_locale(language)].get(key).ok_or_else(||format!("Missing updater button: {key}"))?;
                if &actual!=expected {return Err(format!("Updater button locale mismatch: {} / {key}",language.code()));}
                for other in Language::ALL {
                    if catalog.translate(&actual,other)!=catalog.translate(key,other){return Err("Updater button did not follow a live language switch".into());}
                }
            }
            let source=format_text("%@ %@ is currently the newest version available.",&["flytoSnap".into(),"0.4.0".into()]);
            let translated=catalog.translate(&source,language);
            if !translated.contains("flytoSnap")||!translated.contains("0.4.0"){return Err("Updater localization lost the app name or version".into());}
        }
        println!("Update localization check passed: ten languages, live button translation, app name and version placeholders.");
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn positional_update_templates_preserve_and_reorder_arguments() {
        assert_eq!(
            format_text("%2$@: %1$@ (%1$@)", &["App".into(), "2.0".into()]),
            "2.0: App (App)"
        );
        let template = compile_template("key", "%2$@: %1$@ (%1$@)").unwrap();
        let captures = template.pattern.captures("2.0: App (App)").unwrap();
        let mut arguments = vec![String::new(); 2];
        for (capture, index) in template.arguments.iter().enumerate() {
            arguments[*index] = captures[capture + 1].into();
        }
        assert_eq!(format_text("%@ %@", &arguments), "App 2.0");
    }
}
