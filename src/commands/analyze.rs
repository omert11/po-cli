use anyhow::Result;
use colored::Colorize;
use std::path::Path;

use crate::parser::po_parser;
use crate::types::AnalyzeOutput;
use crate::util::truncate;

pub fn run(po_file: &Path, json: bool) -> Result<()> {
    let parsed = po_parser::parse(po_file)?;

    let output = AnalyzeOutput {
        file_path: po_file.display().to_string(),
        statistics: parsed.statistics,
        untranslated_entries: parsed.untranslated_entries,
        fuzzy_entries: parsed.fuzzy_entries,
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
    println!(
        "  Translated:   {}",
        o.statistics.translated.to_string().green()
    );
    println!(
        "  Untranslated: {}",
        o.statistics.untranslated.to_string().yellow()
    );
    println!(
        "  Fuzzy:        {}",
        o.statistics.fuzzy.to_string().yellow()
    );
    println!("  Total:        {}", o.statistics.total.to_string().bold());

    print_section("Untranslated", &o.untranslated_entries);
    print_section("Fuzzy", &o.fuzzy_entries);
}

fn print_section(label: &str, entries: &[crate::types::PoEntry]) {
    if entries.is_empty() {
        return;
    }
    println!("\n{} ({})", label.yellow().bold(), entries.len());
    for (i, e) in entries.iter().take(20).enumerate() {
        println!("  {:>3}. {}", i + 1, truncate(&e.msgid, 100));
    }
    if entries.len() > 20 {
        println!("  ... {} more", entries.len() - 20);
    }
}
