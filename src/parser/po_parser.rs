use anyhow::{Context, Result};
use polib::catalog::Catalog;
use polib::message::MessageView;
use polib::po_file;
use std::fs;
use std::path::Path;

use crate::types::{PoEntry, PoStatistics};

pub struct ParsedPo {
    pub entries: Vec<PoEntry>,
    pub statistics: PoStatistics,
}

pub fn load_catalog(path: &Path) -> Result<Catalog> {
    po_file::parse(path).with_context(|| format!("Failed to parse PO file: {}", path.display()))
}

pub fn parse(path: &Path) -> Result<ParsedPo> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read PO file: {}", path.display()))?;
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
        .with_context(|| format!("Failed to parse PO file: {}", path.display()))?;

    let _ = fs::remove_file(&tmp_path);

    let mut entries: Vec<PoEntry> = Vec::new();
    let mut translated = 0usize;
    let mut untranslated = 0usize;
    let mut fuzzy = 0usize;

    for message in catalog.messages() {
        let msgid = message.msgid().to_string();
        let msgstr = if message.is_singular() {
            message.msgstr().unwrap_or("").to_string()
        } else {
            message
                .msgstr_plural()
                .ok()
                .map(|v| v.join("\u{0000}"))
                .unwrap_or_default()
        };
        let context = message.msgctxt().and_then(|c| {
            if c.is_empty() {
                None
            } else {
                Some(c.to_string())
            }
        });

        let is_fuzzy = message.is_fuzzy();
        let is_empty = msgstr.is_empty();

        if is_fuzzy {
            fuzzy += 1;
        } else if is_empty {
            untranslated += 1;
        } else {
            translated += 1;
        }

        entries.push(PoEntry {
            msgid,
            msgstr,
            context,
        });
    }

    let statistics = PoStatistics {
        translated,
        untranslated,
        fuzzy,
        total: entries.len(),
    };

    Ok(ParsedPo {
        entries,
        statistics,
    })
}

pub fn fuzzy(catalog: &Catalog) -> Vec<PoEntry> {
    catalog
        .messages()
        .filter(|m| m.is_fuzzy())
        .map(message_to_entry)
        .collect()
}

fn message_to_entry(m: &dyn MessageView) -> PoEntry {
    let msgstr = if m.is_singular() {
        m.msgstr().unwrap_or("").to_string()
    } else {
        m.msgstr_plural()
            .ok()
            .map(|v| v.join("\u{0000}"))
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

/// Strip obsolete `#~|` lines (previous msgid metadata) that some parsers reject.
pub fn preprocess_po_content(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.trim_start().starts_with("#~|"))
        .collect::<Vec<_>>()
        .join("\n")
}
