use anyhow::{bail, Context, Result};
use colored::Colorize;
use polib::message::{MessageMutView, MessageView};
use polib::po_file;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use crate::parser::po_parser;
use crate::types::{
    TranslationEntry, UpdateResult, ValidateAndUpdateOutput, ValidationOutput,
};
use crate::validator;

pub fn run(
    po_file: &Path,
    translations_file: &Path,
    strict: bool,
    dry_run: bool,
    force: bool,
    json: bool,
) -> Result<()> {
    if !po_file.exists() {
        bail!("PO file not found: {}", po_file.display());
    }
    if !translations_file.exists() {
        bail!(
            "Translations JSON file not found: {}",
            translations_file.display()
        );
    }

    let raw = fs::read_to_string(translations_file).with_context(|| {
        format!(
            "Failed to read translations file: {}",
            translations_file.display()
        )
    })?;
    let translations: Vec<TranslationEntry> =
        serde_json::from_str(&raw).context("Translations JSON must be an array of {msgid, msgstr, context}")?;

    let validation = validator::validate(&translations, strict);
    let should_update = validation.valid || force;

    let output = if !should_update {
        ValidateAndUpdateOutput {
            message: format!(
                "Validation failed: {} invalid. Use --force to update anyway.",
                validation.invalids.len()
            ),
            validation,
            update: None,
        }
    } else {
        let invalid_msgids: std::collections::HashSet<String> = validation
            .invalids
            .iter()
            .map(|r| r.msgid.clone())
            .collect();

        let to_apply: Vec<TranslationEntry> = if force {
            translations.clone()
        } else {
            translations
                .iter()
                .filter(|t| !invalid_msgids.contains(&t.msgid))
                .cloned()
                .collect()
        };

        let update = apply_to_po(po_file, &to_apply, dry_run);
        let message = build_message(&validation, &update, dry_run, force);

        ValidateAndUpdateOutput {
            validation,
            update: Some(update),
            message,
        }
    };

    if json {
        println!("{}", serde_json::to_string_pretty(&output)?);
    } else {
        print_human(&output, dry_run);
    }
    Ok(())
}

fn apply_to_po(po_file: &Path, translations: &[TranslationEntry], dry_run: bool) -> UpdateResult {
    let mut errors: Vec<String> = Vec::new();

    let raw = match fs::read_to_string(po_file) {
        Ok(s) => s,
        Err(e) => {
            return UpdateResult {
                success: false,
                updated_entries: 0,
                file_path: po_file.display().to_string(),
                errors: vec![format!("Failed to read PO file: {e}")],
            };
        }
    };
    let cleaned = po_parser::preprocess_po_content(&raw);

    let tmp_path = std::env::temp_dir().join(format!(
        "po-cli-update-{}-{}",
        std::process::id(),
        po_file
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "input.po".to_string())
    ));
    if let Err(e) = fs::write(&tmp_path, &cleaned) {
        return UpdateResult {
            success: false,
            updated_entries: 0,
            file_path: po_file.display().to_string(),
            errors: vec![format!("Failed to write temp PO file: {e}")],
        };
    }

    let mut catalog = match po_file::parse(&tmp_path) {
        Ok(c) => c,
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            return UpdateResult {
                success: false,
                updated_entries: 0,
                file_path: po_file.display().to_string(),
                errors: vec![format!("Failed to parse PO file: {e}")],
            };
        }
    };
    let _ = fs::remove_file(&tmp_path);

    let mut map: HashMap<(String, String), &TranslationEntry> = HashMap::new();
    for t in translations {
        let key = (t.context.clone().unwrap_or_default(), t.msgid.clone());
        map.insert(key, t);
    }

    let mut updated = 0usize;
    for mut message in catalog.messages_mut() {
        let context = message.msgctxt().unwrap_or("").to_string();
        let msgid = message.msgid().to_string();
        let key = (context, msgid);
        if let Some(t) = map.get(&key) {
            if message.is_singular() {
                if let Err(e) = message.set_msgstr(t.msgstr.clone()) {
                    errors.push(format!("Failed to set msgstr for '{}': {e}", t.msgid));
                    continue;
                }
                if message.is_fuzzy() {
                    message.flags_mut().remove_flag("fuzzy");
                }
                updated += 1;
            }
        }
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
        println!(
            "  File: {}",
            u.file_path.cyan()
        );
        println!(
            "  Updated: {}  Dry-run: {}",
            u.updated_entries.to_string().green(),
            if dry_run { "yes".yellow() } else { "no".green() }
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let cut: String = s.chars().take(max).collect();
        format!("{cut}...")
    }
}
