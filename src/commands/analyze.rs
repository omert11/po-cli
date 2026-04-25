use anyhow::{bail, Result};
use colored::Colorize;
use std::path::Path;

use crate::parser::po_parser;
use crate::types::{AnalyzeOutput, PoEntry};

pub fn run(po_file: &Path, json: bool) -> Result<()> {
    if !po_file.exists() {
        bail!("PO file not found: {}", po_file.display());
    }
    if !po_file.is_file() {
        bail!("Path is not a file: {}", po_file.display());
    }

    let parsed = po_parser::parse(po_file)?;

    let untranslated_entries: Vec<PoEntry> = parsed
        .entries
        .iter()
        .filter(|e| e.msgstr.is_empty())
        .cloned()
        .collect();

    let fuzzy_entries: Vec<PoEntry> = {
        let catalog = po_parser::load_catalog(po_file)?;
        po_parser::fuzzy(&catalog)
    };

    let untranslated_only: Vec<PoEntry> = untranslated_entries
        .into_iter()
        .filter(|e| {
            !fuzzy_entries
                .iter()
                .any(|f| f.msgid == e.msgid && f.context == e.context)
        })
        .collect();

    let output = AnalyzeOutput {
        file_path: po_file.display().to_string(),
        statistics: parsed.statistics,
        untranslated_entries: untranslated_only,
        fuzzy_entries,
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_human(&output);
    }
    Ok(())
}

fn print_human(o: &AnalyzeOutput) {
    println!("{}", "PO File Analysis".bold().underline());
    println!("Path: {}", o.file_path.cyan());
    println!();
    println!("{}", "Statistics".bold());
    println!("  Translated:   {}", o.statistics.translated.to_string().green());
    println!(
        "  Untranslated: {}",
        o.statistics.untranslated.to_string().yellow()
    );
    println!("  Fuzzy:        {}", o.statistics.fuzzy.to_string().yellow());
    println!("  Total:        {}", o.statistics.total.to_string().bold());

    if !o.untranslated_entries.is_empty() {
        println!("\n{} ({})", "Untranslated".yellow().bold(), o.untranslated_entries.len());
        for (i, e) in o.untranslated_entries.iter().take(20).enumerate() {
            println!("  {:>3}. {}", i + 1, truncate(&e.msgid, 100));
        }
        if o.untranslated_entries.len() > 20 {
            println!("  ... {} more", o.untranslated_entries.len() - 20);
        }
    }

    if !o.fuzzy_entries.is_empty() {
        println!("\n{} ({})", "Fuzzy".yellow().bold(), o.fuzzy_entries.len());
        for (i, e) in o.fuzzy_entries.iter().take(20).enumerate() {
            println!("  {:>3}. {}", i + 1, truncate(&e.msgid, 100));
        }
        if o.fuzzy_entries.len() > 20 {
            println!("  ... {} more", o.fuzzy_entries.len() - 20);
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}...")
    }
}
