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

    // Apply the valid entries even when some are invalid: a single false-positive
    // should never block the rest of the batch. `--force` additionally applies the
    // invalid ones. If nothing is left to apply, report and bail out.
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

    if to_apply.is_empty() {
        let output = ValidateAndUpdateOutput {
            message: format!(
                "Validation failed: all {} invalid. Use --force to update anyway.",
                validation.invalids.len()
            ),
            validation,
            update: None,
        };
        return emit(&output, dry_run, json);
    }

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

    // Read the raw source once so we can preserve obsolete (`#~`) blocks, which
    // polib drops on write.
    let raw = match fs::read_to_string(po_file) {
        Ok(s) => s,
        Err(e) => {
            return failed(po_file, format!("Failed to read PO file: {e}"));
        }
    };
    let obsolete_block = po_parser::extract_obsolete_block(&raw);

    let mut catalog = match po_parser::parse_cleaned(po_file) {
        Ok(c) => c,
        Err(e) => {
            return failed(po_file, format!("{e:#}"));
        }
    };

    // Deduplicate the requested entries by (context, msgid) so the skipped count
    // reflects distinct keys, not repeated input lines.
    let mut map: HashMap<(&str, &str), &TranslationEntry> = HashMap::new();
    for t in translations {
        let ctx = t.context.as_deref().unwrap_or("");
        map.insert((ctx, t.msgid.as_str()), t);
    }
    let requested = map.len();

    // Track which requested keys we actually wrote, so "not found" keys can be
    // reported separately from plural-form skips.
    let mut matched: HashSet<(String, String)> = HashSet::new();
    let mut updated = 0usize;
    for mut message in catalog.messages_mut() {
        let ctx = message.msgctxt().unwrap_or("");
        let msgid = message.msgid();
        let Some(t) = map.get(&(ctx, msgid)).copied() else {
            continue;
        };
        matched.insert((ctx.to_string(), msgid.to_string()));

        let set_result = if message.is_singular() {
            message
                .set_msgstr(t.msgstr.clone())
                .map_err(anyhow::Error::from)
        } else {
            set_plural_msgstr(&mut message, &t.msgstr)
        };
        if let Err(e) = set_result {
            errors.push(format!("Failed to set msgstr for '{}': {e}", t.msgid));
            continue;
        }
        if message.is_fuzzy() {
            message.flags_mut().remove_flag(po_parser::fuzzy_flag());
        }
        updated += 1;
    }

    // Requested keys whose (context, msgid) was not present in the catalog
    // (stale msgid, wrong context). Distinct from write failures above.
    let not_found = requested.saturating_sub(matched.len());
    let skipped = requested.saturating_sub(updated);

    if !dry_run {
        if let Err(e) = po_file::write_to_file(&catalog, po_file) {
            // Preserve any per-entry errors already collected alongside the write error.
            errors.push(format!("Failed to write PO file: {e}"));
            return UpdateResult {
                success: false,
                updated_entries: updated,
                skipped_entries: skipped,
                not_found_entries: not_found,
                file_path: po_file.display().to_string(),
                errors,
            };
        }
        if let Err(e) = post_process_file(po_file, &obsolete_block) {
            errors.push(format!("Post-process failed: {e}"));
        }
    }

    UpdateResult {
        success: errors.is_empty(),
        updated_entries: updated,
        skipped_entries: skipped,
        not_found_entries: not_found,
        file_path: po_file.display().to_string(),
        errors,
    }
}

/// Write a plural translation whose forms are flattened into `flat` with the
/// plural separator (the same shape `analyze` emits). The number of provided
/// forms must match the catalog's existing form count, else it is a mismatch.
fn set_plural_msgstr(
    message: &mut polib::catalog::MessageMutProxy,
    flat: &str,
) -> Result<(), anyhow::Error> {
    let forms: Vec<String> = flat
        .split(po_parser::plural_separator())
        .map(|s| s.to_string())
        .collect();
    let target = message
        .msgstr_plural_mut()
        .map_err(|e| anyhow::anyhow!("not a plural message: {e}"))?;
    if forms.len() != target.len() {
        anyhow::bail!(
            "plural form count mismatch: got {}, expected {}",
            forms.len(),
            target.len()
        );
    }
    *target = forms;
    Ok(())
}

/// Build a failed result for an early abort (read/parse) where nothing was applied.
fn failed(po_file: &Path, err: String) -> UpdateResult {
    UpdateResult {
        success: false,
        updated_entries: 0,
        skipped_entries: 0,
        not_found_entries: 0,
        file_path: po_file.display().to_string(),
        errors: vec![err],
    }
}

/// After polib writes the catalog, strip any trailing empty `msgid ""`/`msgstr ""`
/// artifact (defensive — a duplicate header crashes `msgfmt`) and re-append the
/// obsolete block that polib does not round-trip.
fn post_process_file(po_file: &Path, obsolete_block: &str) -> Result<()> {
    let mut content = fs::read_to_string(po_file)
        .with_context(|| format!("re-read after write: {}", po_file.display()))?;

    content = strip_trailing_empty_entry(&content);

    // Re-append the obsolete block only if polib didn't already round-trip it.
    // Guards against doubling if a future polib version preserves `#~` entries.
    if !obsolete_block.is_empty() && !content.contains("#~") {
        let trimmed = content.trim_end();
        content = format!("{trimmed}\n\n{obsolete_block}\n");
    }

    fs::write(po_file, content)
        .with_context(|| format!("write after post-process: {}", po_file.display()))?;
    Ok(())
}

/// Remove a dangling empty entry (`[#, flags]\nmsgid ""\nmsgstr ""`) at end of file.
/// The leading header is never the last entry, so anchoring to EOF is safe.
fn strip_trailing_empty_entry(content: &str) -> String {
    let re = trailing_empty_entry_re();
    re.replace(content, "\n").into_owned()
}

fn trailing_empty_entry_re() -> &'static regex::Regex {
    use std::sync::OnceLock;
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    // Match any number of leading comment lines (`#, fuzzy`, `#. ...`, `#: ...`,
    // `#| ...`) before the empty entry, so the artifact is caught regardless of
    // which flags polib attached to it.
    RE.get_or_init(|| regex::Regex::new(r#"\n(?:#[^\n]*\n)*msgid ""\nmsgstr ""\n\s*$"#).unwrap())
}

fn build_message(
    validation: &ValidationOutput,
    update: &UpdateResult,
    dry_run: bool,
    force: bool,
) -> String {
    let verb = if dry_run { "Would update" } else { "Updated" };
    let not_found = update.not_found_entries;
    // skipped_entries is the total unwritten; the remainder after not-found are
    // form/plural mismatches.
    let mismatch = update.skipped_entries.saturating_sub(not_found);
    let skipped_note = match (mismatch, not_found) {
        (0, 0) => String::new(),
        (m, 0) => format!(" Skipped {m} (plural/form mismatch)."),
        (0, n) => format!(" Skipped {n} (msgid not found)."),
        (m, n) => format!(
            " Skipped {} ({m} plural/form mismatch, {n} not found).",
            m + n
        ),
    };

    if validation.valid {
        format!(
            "All {} valid. {verb} {} entries.{skipped_note}",
            validation.total, update.updated_entries
        )
    } else if force {
        format!(
            "{verb} {} ({} were invalid, forced).{skipped_note}",
            update.updated_entries,
            validation.invalids.len()
        )
    } else {
        format!(
            "{verb} {} valid entries, skipped {} invalid (use --force to apply those).{skipped_note}",
            update.updated_entries,
            validation.invalids.len()
        )
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
            "  Updated: {}  Skipped: {}  Dry-run: {}",
            u.updated_entries.to_string().green(),
            u.skipped_entries.to_string().yellow(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_po(content: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "po-cli-update-test-{}-{}.po",
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

    const HEADER: &str = "msgid \"\"\nmsgstr \"\"\n\"Content-Type: text/plain; charset=UTF-8\\n\"\n\"Plural-Forms: nplurals=2; plural=(n != 1);\\n\"\n\n";

    #[test]
    fn strip_removes_trailing_fuzzy_empty_entry() {
        let c = format!(
            "{HEADER}msgid \"Hi\"\nmsgstr \"Selam\"\n\n#, fuzzy\nmsgid \"\"\nmsgstr \"\"\n"
        );
        let out = strip_trailing_empty_entry(&c);
        assert_eq!(out.matches("msgid \"\"").count(), 1, "only header remains");
    }

    #[test]
    fn strip_removes_trailing_python_format_empty_entry() {
        // Bug A variant: artifact carries `#, python-format` instead of `#, fuzzy`.
        let c = format!(
            "{HEADER}msgid \"Hi\"\nmsgstr \"Selam\"\n\n#, python-format\nmsgid \"\"\nmsgstr \"\"\n"
        );
        let out = strip_trailing_empty_entry(&c);
        assert_eq!(out.matches("msgid \"\"").count(), 1);
    }

    #[test]
    fn strip_keeps_clean_file_untouched() {
        let c = format!("{HEADER}msgid \"Hi\"\nmsgstr \"Selam\"\n");
        let out = strip_trailing_empty_entry(&c);
        assert_eq!(out.matches("msgid \"\"").count(), 1);
        assert!(out.contains("msgid \"Hi\""));
    }

    #[test]
    fn apply_preserves_obsolete_block() {
        // Bug A (real): obsolete `#~` blocks must survive an update.
        let po = format!(
            "{HEADER}msgid \"Save\"\nmsgstr \"\"\n\n#~ msgid \"Old\"\n#~ msgstr \"Eski\"\n"
        );
        let path = write_temp_po(&po);
        let t = vec![TranslationEntry {
            msgid: "Save".to_string(),
            msgstr: "Kaydet".to_string(),
            context: None,
        }];
        let res = apply_to_po(&path, &t, false);
        let written = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert!(res.success, "errors: {:?}", res.errors);
        assert_eq!(res.updated_entries, 1);
        assert!(
            written.contains("#~ msgid \"Old\""),
            "obsolete block lost:\n{written}"
        );
        assert_eq!(written.matches("msgid \"\"").count(), 1, "no dup header");
    }

    #[test]
    fn apply_writes_plural_forms() {
        // Bug C fix: plural translations (separator-joined) are actually written.
        let sep = po_parser::plural_separator();
        let po = format!(
            "{HEADER}msgid \"%(n)s item\"\nmsgid_plural \"%(n)s items\"\nmsgstr[0] \"\"\nmsgstr[1] \"\"\n"
        );
        let path = write_temp_po(&po);
        let t = vec![TranslationEntry {
            msgid: "%(n)s item".to_string(),
            msgstr: format!("{}{}{}", "%(n)s öğe", sep, "%(n)s öğe"),
            context: None,
        }];
        let res = apply_to_po(&path, &t, false);
        let written = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(res.updated_entries, 1, "plural should be written");
        assert_eq!(res.skipped_entries, 0);
        assert!(
            written.contains("msgstr[0] \"%(n)s öğe\""),
            "got:\n{written}"
        );
        assert!(written.contains("msgstr[1] \"%(n)s öğe\""));
    }

    #[test]
    fn apply_plural_form_count_mismatch_is_skipped() {
        // Providing the wrong number of plural forms is reported, not silently written.
        let po = format!(
            "{HEADER}msgid \"%(n)s item\"\nmsgid_plural \"%(n)s items\"\nmsgstr[0] \"\"\nmsgstr[1] \"\"\n"
        );
        let path = write_temp_po(&po);
        let t = vec![TranslationEntry {
            msgid: "%(n)s item".to_string(),
            msgstr: "%(n)s öğe".to_string(), // single form, catalog expects 2
            context: None,
        }];
        let res = apply_to_po(&path, &t, false);
        let _ = fs::remove_file(&path);

        assert_eq!(res.updated_entries, 0);
        assert_eq!(res.skipped_entries, 1);
        assert_eq!(res.not_found_entries, 0, "it was found, just mismatched");
        assert!(!res.errors.is_empty(), "mismatch should surface an error");
    }

    #[test]
    fn duplicate_input_does_not_inflate_skipped() {
        // Bug: skipped was translations.len() - updated; duplicate keys broke it.
        let po = format!("{HEADER}msgid \"Hi\"\nmsgstr \"\"\n");
        let path = write_temp_po(&po);
        let t = vec![
            TranslationEntry {
                msgid: "Hi".to_string(),
                msgstr: "Selam".to_string(),
                context: None,
            },
            TranslationEntry {
                msgid: "Hi".to_string(),
                msgstr: "Selam".to_string(),
                context: None,
            },
        ];
        let res = apply_to_po(&path, &t, false);
        let _ = fs::remove_file(&path);

        assert_eq!(res.updated_entries, 1);
        assert_eq!(
            res.skipped_entries, 0,
            "duplicate input must not count as skipped"
        );
        assert_eq!(res.not_found_entries, 0);
    }

    #[test]
    fn not_found_msgid_is_reported_separately() {
        let po = format!("{HEADER}msgid \"Hi\"\nmsgstr \"\"\n");
        let path = write_temp_po(&po);
        let t = vec![TranslationEntry {
            msgid: "Nonexistent".to_string(),
            msgstr: "Yok".to_string(),
            context: None,
        }];
        let res = apply_to_po(&path, &t, false);
        let _ = fs::remove_file(&path);

        assert_eq!(res.updated_entries, 0);
        assert_eq!(res.not_found_entries, 1);
        assert_eq!(res.skipped_entries, 1);
    }

    #[test]
    fn obsolete_not_doubled_if_already_present() {
        // B4: guard against double-append if the written file already has #~.
        let block = "#~ msgid \"Old\"\n#~ msgstr \"Eski\"";
        let content = format!("{HEADER}msgid \"Hi\"\nmsgstr \"Selam\"\n\n{block}\n");
        let path = write_temp_po(&content);
        // Simulate post-process when polib already kept the obsolete block.
        post_process_file(&path, block).unwrap();
        let after = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(
            after.matches("#~ msgid \"Old\"").count(),
            1,
            "obsolete must not be doubled:\n{after}"
        );
    }

    #[test]
    fn apply_singular_writes_and_clears_fuzzy() {
        let po = format!("{HEADER}#, fuzzy\nmsgid \"Hi\"\nmsgstr \"eski\"\n");
        let path = write_temp_po(&po);
        let t = vec![TranslationEntry {
            msgid: "Hi".to_string(),
            msgstr: "Selam".to_string(),
            context: None,
        }];
        let res = apply_to_po(&path, &t, false);
        let written = fs::read_to_string(&path).unwrap();
        let _ = fs::remove_file(&path);

        assert_eq!(res.updated_entries, 1);
        assert!(written.contains("msgstr \"Selam\""));
        assert!(
            !written.contains("#, fuzzy"),
            "fuzzy flag should be cleared"
        );
    }
}
