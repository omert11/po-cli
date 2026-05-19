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
    pub obsolete_entries: Vec<PoEntry>,
}

pub fn parse(path: &Path) -> Result<ParsedPo> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read PO file: {}", path.display()))?;
    let obsolete_entries = parse_obsolete_entries(&raw);

    let catalog = parse_cleaned_from_str(&raw, path)?;

    let mut untranslated_entries: Vec<PoEntry> = Vec::new();
    let mut fuzzy_entries: Vec<PoEntry> = Vec::new();
    let mut translated = 0usize;
    let mut untranslated = 0usize;
    let mut fuzzy = 0usize;
    let mut total = 0usize;

    for message in catalog.messages() {
        if is_header(message) {
            continue;
        }

        total += 1;
        let is_untranslated = msgstr_is_empty(message);

        if message.is_fuzzy() {
            fuzzy += 1;
            fuzzy_entries.push(message_to_entry(message));
        } else if is_untranslated {
            untranslated += 1;
            untranslated_entries.push(message_to_entry(message));
        } else {
            translated += 1;
        }
    }

    let obsolete = obsolete_entries.len();
    total += obsolete;

    Ok(ParsedPo {
        statistics: PoStatistics {
            translated,
            untranslated,
            fuzzy,
            obsolete,
            total,
        },
        untranslated_entries,
        fuzzy_entries,
        obsolete_entries,
    })
}

pub fn parse_cleaned(path: &Path) -> Result<Catalog> {
    let raw = fs::read_to_string(path)
        .with_context(|| format!("Failed to read PO file: {}", path.display()))?;
    parse_cleaned_from_str(&raw, path)
}

fn parse_cleaned_from_str(raw: &str, path: &Path) -> Result<Catalog> {
    if !raw.contains("#~|") {
        return po_file::parse(path)
            .with_context(|| format!("Failed to parse PO file: {}", path.display()));
    }

    let cleaned = preprocess_po_content(raw);
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

fn is_header(m: &dyn MessageView) -> bool {
    m.msgid().is_empty()
}

fn msgstr_is_empty(m: &dyn MessageView) -> bool {
    if m.is_singular() {
        m.msgstr().map(|s| s.is_empty()).unwrap_or(true)
    } else {
        m.msgstr_plural()
            .map(|forms| forms.is_empty() || forms.iter().any(|s| s.is_empty()))
            .unwrap_or(true)
    }
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
    let mut out = String::with_capacity(content.len());
    let mut first = true;
    for line in content.lines() {
        if line.trim_start().starts_with("#~|") {
            continue;
        }
        if !first {
            out.push('\n');
        }
        out.push_str(line);
        first = false;
    }
    out
}

fn parse_obsolete_entries(content: &str) -> Vec<PoEntry> {
    if !content.contains("#~") {
        return Vec::new();
    }

    let mut entries: Vec<PoEntry> = Vec::new();
    let mut current: Option<ObsoleteBuilder> = None;
    let flush = |cur: &mut Option<ObsoleteBuilder>, entries: &mut Vec<PoEntry>| {
        if let Some(entry) = cur.take().and_then(ObsoleteBuilder::finish) {
            entries.push(entry);
        }
    };

    for raw_line in content.lines() {
        let line = raw_line.trim_start();
        if !line.starts_with("#~") {
            flush(&mut current, &mut entries);
            continue;
        }

        let payload = line.trim_start_matches("#~").trim_start();
        if payload.is_empty() {
            continue;
        }

        if starts_with_keyword(payload, "msgctxt") {
            flush(&mut current, &mut entries);
            let context = extract_string_literal(payload);
            current = Some(ObsoleteBuilder {
                context: normalize_context(context),
                ..Default::default()
            });
        } else if starts_with_keyword(payload, "msgid_plural") {
            if let Some(b) = current.as_mut() {
                b.section = ObsoleteSection::IdPlural;
                b.id_plural = Some(extract_string_literal(payload));
            }
        } else if starts_with_keyword(payload, "msgid") {
            if current.as_ref().is_some_and(|b| b.id.is_some()) {
                flush(&mut current, &mut entries);
            }
            let mut b = current.take().unwrap_or_default();
            b.section = ObsoleteSection::Id;
            b.id = Some(extract_string_literal(payload));
            current = Some(b);
        } else if payload.starts_with("msgstr[") {
            if let Some(b) = current.as_mut() {
                b.section = ObsoleteSection::StrPlural;
                b.str_plural.push(extract_string_literal(payload));
            }
        } else if starts_with_keyword(payload, "msgstr") {
            if let Some(b) = current.as_mut() {
                b.section = ObsoleteSection::Str;
                b.str_singular = Some(extract_string_literal(payload));
            }
        } else if payload.starts_with('"') {
            if let Some(b) = current.as_mut() {
                if let Some(target) = b.current_target_mut() {
                    target.push_str(&extract_string_literal(payload));
                }
            }
        }
    }

    flush(&mut current, &mut entries);
    entries
}

fn starts_with_keyword(payload: &str, keyword: &str) -> bool {
    let Some(rest) = payload.strip_prefix(keyword) else {
        return false;
    };
    matches!(rest.as_bytes().first(), Some(b' ' | b'\t'))
}

fn normalize_context(c: String) -> Option<String> {
    if c.is_empty() {
        None
    } else {
        Some(c)
    }
}

#[derive(Default)]
struct ObsoleteBuilder {
    context: Option<String>,
    id: Option<String>,
    id_plural: Option<String>,
    str_singular: Option<String>,
    str_plural: Vec<String>,
    section: ObsoleteSection,
}

#[derive(Default, Clone, Copy)]
enum ObsoleteSection {
    #[default]
    None,
    Id,
    IdPlural,
    Str,
    StrPlural,
}

impl ObsoleteBuilder {
    fn finish(self) -> Option<PoEntry> {
        let msgid = self.id?;
        let msgstr = if !self.str_plural.is_empty() {
            self.str_plural.join(PLURAL_SEP)
        } else {
            self.str_singular.unwrap_or_default()
        };
        Some(PoEntry {
            msgid,
            msgstr,
            context: self.context,
        })
    }

    fn current_target_mut(&mut self) -> Option<&mut String> {
        match self.section {
            ObsoleteSection::Id => self.id.as_mut(),
            ObsoleteSection::IdPlural => self.id_plural.as_mut(),
            ObsoleteSection::Str => self.str_singular.as_mut(),
            ObsoleteSection::StrPlural => self.str_plural.last_mut(),
            ObsoleteSection::None => None,
        }
    }
}

fn extract_string_literal(line: &str) -> String {
    let Some(start) = line.find('"') else {
        return String::new();
    };
    let Some(end_offset) = line[start + 1..].rfind('"') else {
        return String::new();
    };
    let end = start + 1 + end_offset;
    if end <= start + 1 {
        return String::new();
    }
    unescape_po_string(&line[start + 1..end])
}

fn unescape_po_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('r') => out.push('\r'),
            Some('"') => out.push('"'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_po(content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "po-cli-test-{}-{}.po",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut f = fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    const HEADER: &str = r#"msgid ""
msgstr ""
"Content-Type: text/plain; charset=UTF-8\n"
"Plural-Forms: nplurals=2; plural=(n != 1);\n"

"#;

    #[test]
    fn header_entry_is_skipped() {
        let po = format!("{}msgid \"Hello\"\nmsgstr \"Merhaba\"\n", HEADER);
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.total, 1);
        assert_eq!(parsed.statistics.translated, 1);
        assert_eq!(parsed.statistics.untranslated, 0);
        assert!(parsed.untranslated_entries.is_empty());
    }

    #[test]
    fn plural_with_all_forms_filled_is_translated() {
        let po = format!(
            "{}msgid \"%(n)s ticket\"\nmsgid_plural \"%(n)s tickets\"\nmsgstr[0] \"%(n)s bilet\"\nmsgstr[1] \"%(n)s bilet\"\n",
            HEADER
        );
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.translated, 1);
        assert_eq!(parsed.statistics.untranslated, 0);
        assert!(parsed.untranslated_entries.is_empty());
    }

    #[test]
    fn plural_with_empty_form_is_untranslated() {
        let po = format!(
            "{}msgid \"%(n)s ticket\"\nmsgid_plural \"%(n)s tickets\"\nmsgstr[0] \"%(n)s bilet\"\nmsgstr[1] \"\"\n",
            HEADER
        );
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.untranslated, 1);
        assert_eq!(parsed.statistics.translated, 0);
        assert_eq!(parsed.untranslated_entries.len(), 1);
        assert_eq!(parsed.untranslated_entries[0].msgid, "%(n)s ticket");
    }

    #[test]
    fn obsolete_singular_entry_is_collected() {
        let po = format!(
            "{}msgid \"Active\"\nmsgstr \"Aktif\"\n\n#~ msgid \"Process Date\"\n#~ msgstr \"İşlem Tarihi\"\n",
            HEADER
        );
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.translated, 1);
        assert_eq!(parsed.statistics.obsolete, 1);
        assert_eq!(parsed.statistics.total, 2);
        assert_eq!(parsed.obsolete_entries.len(), 1);
        assert_eq!(parsed.obsolete_entries[0].msgid, "Process Date");
        assert_eq!(parsed.obsolete_entries[0].msgstr, "İşlem Tarihi");
        assert!(parsed.obsolete_entries[0].context.is_none());
    }

    #[test]
    fn obsolete_multiline_string_is_concatenated() {
        let po = format!(
            "{}#~ msgid \"\"\n#~ \"Booking \"\n#~ \"Number\"\n#~ msgstr \"\"\n#~ \"Rezervasyon \"\n#~ \"No\"\n",
            HEADER
        );
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.obsolete, 1);
        assert_eq!(parsed.obsolete_entries[0].msgid, "Booking Number");
        assert_eq!(parsed.obsolete_entries[0].msgstr, "Rezervasyon No");
    }

    #[test]
    fn obsolete_plural_entry_joins_forms() {
        let po = format!(
            "{}#~ msgid \"%(n)s ticket\"\n#~ msgid_plural \"%(n)s tickets\"\n#~ msgstr[0] \"%(n)s bilet\"\n#~ msgstr[1] \"%(n)s bilet\"\n",
            HEADER
        );
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.obsolete, 1);
        assert_eq!(parsed.obsolete_entries[0].msgid, "%(n)s ticket");
        assert!(parsed.obsolete_entries[0]
            .msgstr
            .contains("%(n)s bilet\u{0000}%(n)s bilet"));
    }

    #[test]
    fn obsolete_msgctxt_is_captured() {
        let po = format!(
            "{}#~ msgctxt \"menu\"\n#~ msgid \"Open\"\n#~ msgstr \"Aç\"\n",
            HEADER
        );
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.obsolete, 1);
        assert_eq!(parsed.obsolete_entries[0].context.as_deref(), Some("menu"));
    }

    #[test]
    fn no_obsolete_block_yields_zero() {
        let po = format!("{}msgid \"Hi\"\nmsgstr \"Selam\"\n", HEADER);
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.obsolete, 0);
        assert!(parsed.obsolete_entries.is_empty());
        assert_eq!(parsed.statistics.total, 1);
    }

    #[test]
    fn singular_empty_msgstr_is_untranslated() {
        let po = format!("{}msgid \"Hello\"\nmsgstr \"\"\n", HEADER);
        let path = write_temp_po(&po);
        let parsed = parse(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(parsed.statistics.untranslated, 1);
        assert_eq!(parsed.untranslated_entries[0].msgid, "Hello");
    }
}
