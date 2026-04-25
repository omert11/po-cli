use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

use crate::types::{TranslationEntry, ValidationOutput, ValidationResult};

struct Patterns {
    python_old_style: Regex,
    python_format: Regex,
    django_var: Regex,
    django_tag: Regex,
    html_tag: Regex,
    url: Regex,
    js_code: Regex,
}

fn patterns() -> &'static Patterns {
    static P: OnceLock<Patterns> = OnceLock::new();
    P.get_or_init(|| Patterns {
        python_old_style: Regex::new(r"%\([^)]+\)[diouxXeEfFgGcrs]").unwrap(),
        python_format: Regex::new(r"\{[^}]*\}").unwrap(),
        django_var: Regex::new(r"\{\{[^}]+\}\}").unwrap(),
        django_tag: Regex::new(r"\{%[^%]+%\}").unwrap(),
        html_tag: Regex::new(r"<[^>]+>").unwrap(),
        url: Regex::new(r#"https?://[^\s<>"]+|www\.[^\s<>"]+"#).unwrap(),
        js_code: Regex::new(
            r#"(?:[a-zA-Z_$][\w$]*\.)*[a-zA-Z_$][\w$]*\s*\([^)]*['"`]|(?:[a-zA-Z_$][\w$]*\.)+[a-zA-Z_$][\w$]*\s*\([^)]*\)"#,
        )
        .unwrap(),
    })
}

#[derive(Default, Debug)]
struct Extracted<'a> {
    variables: HashSet<&'a str>,
    html_tags: HashSet<&'a str>,
    urls: HashSet<&'a str>,
    js_codes: HashSet<&'a str>,
}

fn extract(text: &str) -> Extracted<'_> {
    let p = patterns();
    let mut e = Extracted::default();

    for r in [
        &p.python_old_style,
        &p.python_format,
        &p.django_var,
        &p.django_tag,
    ] {
        for m in r.find_iter(text) {
            e.variables.insert(m.as_str());
        }
    }
    for m in p.html_tag.find_iter(text) {
        e.html_tags.insert(m.as_str());
    }
    for m in p.url.find_iter(text) {
        e.urls.insert(m.as_str());
    }
    for m in p.js_code.find_iter(text) {
        e.js_codes.insert(m.as_str());
    }

    e
}

pub fn validate(translations: &[TranslationEntry], strict: bool) -> ValidationOutput {
    let mut invalids: Vec<ValidationResult> = Vec::new();

    for t in translations {
        let mut issues: Vec<String> = Vec::new();

        if strict {
            let src = extract(&t.msgid);
            let tgt = extract(&t.msgstr);
            check_set("variables", &src.variables, &tgt.variables, &mut issues);
            check_set("HTML tags", &src.html_tags, &tgt.html_tags, &mut issues);
            check_exact("URL", &src.urls, &tgt.urls, &mut issues);
            check_exact("JavaScript code", &src.js_codes, &tgt.js_codes, &mut issues);
        }

        if t.msgstr.trim().is_empty() {
            issues.push("Translation is empty".to_string());
        }

        if !issues.is_empty() {
            invalids.push(ValidationResult {
                msgid: t.msgid.clone(),
                msgstr: t.msgstr.clone(),
                issues,
            });
        }
    }

    let total = translations.len();
    let valid = invalids.is_empty();
    ValidationOutput {
        invalids,
        total,
        valid,
    }
}

fn check_set(label: &str, src: &HashSet<&str>, tgt: &HashSet<&str>, issues: &mut Vec<String>) {
    let missing: Vec<&&str> = src.difference(tgt).collect();
    let extra: Vec<&&str> = tgt.difference(src).collect();
    if !missing.is_empty() {
        let joined = missing
            .iter()
            .copied()
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        issues.push(format!("Missing {label}: {joined}"));
    }
    if !extra.is_empty() {
        let joined = extra
            .iter()
            .copied()
            .copied()
            .collect::<Vec<_>>()
            .join(", ");
        issues.push(format!("Extra {label}: {joined}"));
    }
}

fn check_exact(label: &str, src: &HashSet<&str>, tgt: &HashSet<&str>, issues: &mut Vec<String>) {
    for url in src.difference(tgt) {
        issues.push(format!("{label} changed or missing: {url}"));
    }
    for url in tgt.difference(src) {
        issues.push(format!("Unexpected new {label}: {url}"));
    }
}
