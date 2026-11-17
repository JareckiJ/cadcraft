//! Command names in the interface language, typed at the command line the way the English names
//! are: `OKRĄG` (or `OKRAG`) starts CIRCLE while the interface is Polish. A leading `_` always asks
//! for the English name (`_O` is OFFSET), which is how scripts and LISP routines stay portable.
//!
//! The language is set per thread by the UI ([`set_language`]); headless use, the CLI and tests
//! keep English unless they choose otherwise.

use std::cell::Cell;

/// One localized command: its id, its name and an optional short form.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LocalName {
    pub id: &'static str,
    pub name: &'static str,
    pub shortcut: Option<&'static str>,
}

struct Catalog {
    code: &'static str,
    source: &'static str,
}

const CATALOGS: &[Catalog] = &[Catalog { code: "pl", source: include_str!("localized/pl.tsv") }];

thread_local! {
    static LANGUAGE: Cell<&'static str> = const { Cell::new("en") };
}

/// Sets the language whose command names the command line accepts on this thread.
pub fn set_language(code: &str) {
    let code = CATALOGS.iter().find(|c| c.code == code).map_or("en", |c| c.code);
    LANGUAGE.set(code);
}

/// The language whose command names the command line accepts on this thread (`"en"`: none).
pub fn language() -> &'static str {
    LANGUAGE.get()
}

/// Every localized command name of `code`, in catalog order.
pub fn names(code: &str) -> Vec<LocalName> {
    let Some(catalog) = CATALOGS.iter().find(|c| c.code == code) else {
        return Vec::new();
    };
    catalog
        .source
        .lines()
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let mut cols = l.split('\t');
            let id = cols.next()?.trim();
            let name = cols.next()?.trim();
            let shortcut = cols.next().map(str::trim).filter(|s| !s.is_empty());
            (!id.is_empty() && !name.is_empty()).then_some(LocalName { id, name, shortcut })
        })
        .collect()
}

/// `word` without Polish diacritics, lower-cased: `Łuk` → `luk`.
pub fn fold(word: &str) -> String {
    word.chars()
        .flat_map(char::to_lowercase)
        .map(|c| match c {
            'ą' => 'a',
            'ć' => 'c',
            'ę' => 'e',
            'ł' => 'l',
            'ń' => 'n',
            'ó' => 'o',
            'ś' => 's',
            'ź' | 'ż' => 'z',
            c => c,
        })
        .collect()
}

/// The command id that `typed` names in `code`: a name in any case, a name typed without Polish
/// letters (only for names of three letters or more, so `OS` stays OSNAP), or a shortcut as spelled.
pub fn resolve_in(code: &str, typed: &str) -> Option<&'static str> {
    let lower = typed.trim().to_lowercase();
    if lower.is_empty() {
        return None;
    }
    let folded = fold(&lower);
    let all = names(code);
    all.iter()
        .find(|n| n.name.to_lowercase() == lower)
        .or_else(|| all.iter().find(|n| n.shortcut.is_some_and(|s| s.to_lowercase() == lower)))
        .or_else(|| all.iter().find(|n| n.name.chars().count() >= 3 && fold(n.name) == folded))
        .map(|n| n.id)
}

/// [`resolve_in`] for the language set on this thread.
pub fn resolve(typed: &str) -> Option<&'static str> {
    match language() {
        "en" => None,
        code => resolve_in(code, typed),
    }
}

/// The localized name and shortcut of command `id` in the language set on this thread.
pub fn name_of(id: &str) -> Option<LocalName> {
    names(language()).into_iter().find(|n| n.id.eq_ignore_ascii_case(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_polish_row_names_a_command_once() {
        let all = names("pl");
        assert!(all.len() >= 30);
        let mut seen = std::collections::HashSet::new();
        for n in &all {
            assert!(super::super::find_command(n.id).is_some(), "{} is no command", n.id);
            assert!(seen.insert(n.name.to_lowercase()), "{} twice", n.name);
            if let Some(s) = n.shortcut {
                assert!(seen.insert(s.to_lowercase()), "shortcut {s} twice");
            }
        }
    }

    #[test]
    fn polish_names_resolve_in_any_case_and_without_diacritics() {
        assert_eq!(resolve_in("pl", "OKRĄG"), Some("circle"));
        assert_eq!(resolve_in("pl", "okrąg"), Some("circle"));
        assert_eq!(resolve_in("pl", "OKRAG"), Some("circle"));
        assert_eq!(resolve_in("pl", "Łuk"), Some("arc"));
        assert_eq!(resolve_in("pl", "luk"), Some("arc"));
        assert_eq!(resolve_in("pl", "ł"), Some("arc"));
        assert_eq!(resolve_in("pl", "o"), Some("circle"));
        assert_eq!(resolve_in("pl", "PROSTOKAT"), Some("rectang"));
        assert_eq!(resolve_in("pl", "OŚ"), Some("centerline"));
        assert_eq!(resolve_in("pl", "OS"), None, "a two-letter name needs its diacritic");
        assert_eq!(resolve_in("pl", "nieznane"), None);
        assert_eq!(resolve_in("en", "okrąg"), None);
    }

    #[test]
    fn the_language_is_per_thread_and_defaults_to_english() {
        assert_eq!(language(), "en");
        assert_eq!(resolve("okrąg"), None);
        set_language("pl");
        assert_eq!(resolve("okrąg"), Some("circle"));
        std::thread::spawn(|| assert_eq!(resolve("okrąg"), None)).join().unwrap();
        set_language("xx");
        assert_eq!(language(), "en");
    }
}
