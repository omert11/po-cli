use anyhow::{Context, Result};
use polib::catalog::Catalog;
use polib::message::MessageView;
use polib::po_file;
use std::fs;
use std::path::Path;

use crate::types::{PoEntry, PoStatistics};

const PLURAL_SEP: &str = "\u{0000}";
const FUZZY_FLAG: &str = "fuzzy";

pub struct ParsedPo {
    pub statistics: PoStatistics,
    pub untranslated_entries: Vec<PoEntry>,
    pub fuzzy_entries: Vec<PoEntry>,
}

pub fn parse(path: &Path) -> Result<ParsedPo> {
    let catalog = parse_cleaned(path)?;

    let mut untranslated_entries: Vec<PoEntry> = Vec::new();
    let mut fuzzy_entries: Vec<PoEntry> = Vec::new();
    let mut translated = 0usize;
    let mut untranslated = 0usize;
    let mut fuzzy = 0usize;
    let mut total = 0usize;

    for message in catalog.messages() {
        total += 1;
        let is_fuzzy = message.is_fuzzy();
        let msgstr_empty = message.msgstr().map(|s| s.is_empty()).unwrap_or(true);

        if is_fuzzy {
            fuzzy += 1;
            fuzzy_entries.push(message_to_entry(message));
        } else if msgstr_empty {
            untranslated += 1;
            untranslated_entries.push(message_to_entry(message));
        } else {
            translated += 1;
        }
    }

    Ok(ParsedPo {
        statistics: PoStatistics {
            translated,
            untranslated,
            fuzzy,
            total,
        },
        untranslated_entries,
        fuzzy_entries,
    })
}

pub fn parse_cleaned(path: &Path) -> Result<Catalog> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read PO file: {}", path.display()))?;

    if !raw.contains("#~|") {
        return po_file::parse(path)
            .with_context(|| format!("Failed to parse PO file: {}", path.display()));
    }

    let cleaned = preprocess_po_content(&raw);
    let tmp_path = std::env::temp_dir().join(format!(
        "po-cli-{}-{}",
        std::process::id(),
        path.file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "input.po".to_string())
    ));
    fs::write(&tmp_path, &cleaned)
        .with_context(|| format!("Failed to write temp PO file: {}", tmp_path.display()))?;

    let catalog = po_file::parse(&tmp_path)
        .with_context(|| format!("Failed to parse PO file: {}", path.display()));

    let _ = fs::remove_file(&tmp_path);
    catalog
}

fn message_to_entry(m: &dyn MessageView) -> PoEntry {
    let msgstr = if m.is_singular() {
        m.msgstr().unwrap_or("").to_string()
    } else {
        m.msgstr_plural()
            .ok()
            .map(|v| v.join(PLURAL_SEP))
            .unwrap_or_default()
    };
    let context = m.msgctxt().and_then(|c| {
        if c.is_empty() {
            None
        } else {
            Some(c.to_string())
        }
    });
    PoEntry {
        msgid: m.msgid().to_string(),
        msgstr,
        context,
    }
}

pub fn fuzzy_flag() -> &'static str {
    FUZZY_FLAG
}

fn preprocess_po_content(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim_start().starts_with("#~|"))
        .collect::<Vec<_>>()
        .join("\n")
}
