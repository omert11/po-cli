use anyhow::{Context, Result};
use colored::Colorize;
use polib::message::{MessageMutView, MessageView};
use polib::po_file;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::Path;

use crate::parser::po_parser;
use crate::types::{TranslationEntry, UpdateResult, ValidateAndUpdateOutput, ValidationOutput};
use crate::util::truncate;
use crate::validator;

pub fn run(
    po_file: &Path,
    translations_file: &Path,
    strict: bool,
    dry_run: bool,
    force: bool,
    json: bool,
) -> Result<()> {
    let raw = fs::read_to_string(translations_file).with_context(|| {
        format!(
            "Failed to read translations file: {}",
            translations_file.display()
        )
    })?;
    let translations: Vec<TranslationEntry> = serde_json::from_str(&raw)
        .context("Translations JSON must be an array of {msgid, msgstr, context}")?;

    let validation = validator::validate(&translations, strict);
    let should_update = validation.valid || force;

    if !should_update {
        let output = ValidateAndUpdateOutput {
            message: format!(
                "Validation failed: {} invalid. Use --force to update anyway.",
                validation.invalids.len()
            ),
            validation,
            update: None,
        };
        return emit(&output, dry_run, json);
    }

    let to_apply: Vec<TranslationEntry> = if force {
        translations.clone()
    } else {
        let invalid_msgids: HashSet<&str> = validation
            .invalids
            .iter()
            .map(|r| r.msgid.as_str())
            .collect();
        translations
            .iter()
            .filter(|t| !invalid_msgids.contains(t.msgid.as_str()))
            .cloned()
            .collect()
    };

    let update = apply_to_po(po_file, &to_apply, dry_run);
    let message = build_message(&validation, &update, dry_run, force);

    let output = ValidateAndUpdateOutput {
        validation,
        update: Some(update),
        message,
    };
    emit(&output, dry_run, json)
}

fn emit(output: &ValidateAndUpdateOutput, dry_run: bool, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(output)?);
    } else {
        print_human(output, dry_run);
    }
    Ok(())
}

fn apply_to_po(po_file: &Path, translations: &[TranslationEntry], dry_run: bool) -> UpdateResult {
    let mut errors: Vec<String> = Vec::new();

    let mut catalog = match po_parser::parse_cleaned(po_file) {
        Ok(c) => c,
        Err(e) => {
            return UpdateResult {
                success: false,
                updated_entries: 0,
                file_path: po_file.display().to_string(),
                errors: vec![format!("{e:#}")],
            };
        }
    };

    let mut map: HashMap<(&str, &str), &TranslationEntry> = HashMap::new();
    for t in translations {
        let ctx = t.context.as_deref().unwrap_or("");
        map.insert((ctx, t.msgid.as_str()), t);
    }

    let mut updated = 0usize;
    for mut message in catalog.messages_mut() {
        let ctx = message.msgctxt().unwrap_or("");
        let msgid = message.msgid();
        let Some(t) = map.get(&(ctx, msgid)).copied() else {
            continue;
        };
        if !message.is_singular() {
            continue;
        }
        if let Err(e) = message.set_msgstr(t.msgstr.clone()) {
            errors.push(format!("Failed to set msgstr for '{}': {e}", t.msgid));
            continue;
        }
        if message.is_fuzzy() {
            message.flags_mut().remove_flag(po_parser::fuzzy_flag());
        }
        updated += 1;
    }

    if !dry_run {
        if let Err(e) = po_file::write_to_file(&catalog, po_file) {
            errors.push(format!("Failed to write PO file: {e}"));
            return UpdateResult {
                success: false,
                updated_entries: updated,
                file_path: po_file.display().to_string(),
                errors,
            };
        }
    }

    UpdateResult {
        success: errors.is_empty(),
        updated_entries: updated,
        file_path: po_file.display().to_string(),
        errors,
    }
}

fn build_message(
    validation: &ValidationOutput,
    update: &UpdateResult,
    dry_run: bool,
    force: bool,
) -> String {
    if validation.valid {
        if dry_run {
            format!(
                "All {} valid. Would update {} entries.",
                validation.total, update.updated_entries
            )
        } else {
            format!("Updated {} translations.", update.updated_entries)
        }
    } else if force {
        if dry_run {
            format!(
                "{} invalid. Would force update {} entries.",
                validation.invalids.len(),
                update.updated_entries
            )
        } else {
            format!(
                "Force updated {} ({} were invalid).",
                update.updated_entries,
                validation.invalids.len()
            )
        }
    } else {
        String::new()
    }
}

fn print_human(o: &ValidateAndUpdateOutput, dry_run: bool) {
    println!("{}", "Validation".bold().underline());
    println!(
        "  Total: {}  Invalid: {}  Valid: {}",
        o.validation.total.to_string().bold(),
        o.validation.invalids.len().to_string().yellow(),
        if o.validation.valid {
            "yes".green().to_string()
        } else {
            "no".red().to_string()
        }
    );

    if !o.validation.invalids.is_empty() {
        println!("\n{}", "Invalid entries".red().bold());
        for (i, r) in o.validation.invalids.iter().take(20).enumerate() {
            println!("  {:>3}. {}", i + 1, truncate(&r.msgid, 80));
            for issue in &r.issues {
                println!("       - {}", issue.red());
            }
        }
        if o.validation.invalids.len() > 20 {
            println!("  ... {} more", o.validation.invalids.len() - 20);
        }
    }

    if let Some(u) = &o.update {
        println!("\n{}", "Update".bold().underline());
        println!("  File: {}", u.file_path.cyan());
        println!(
            "  Updated: {}  Dry-run: {}",
            u.updated_entries.to_string().green(),
            if dry_run {
                "yes".yellow()
            } else {
                "no".green()
            }
        );
        if !u.errors.is_empty() {
            println!("  {}", "Errors:".red().bold());
            for err in &u.errors {
                println!("    - {}", err.red());
            }
        }
    }

    if !o.message.is_empty() {
        println!("\n{} {}", "→".bold(), o.message);
    }
}
