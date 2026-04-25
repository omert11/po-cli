# po-cli

Rust CLI for analyzing and updating Django gettext `.po` files. Sibling of [po-mcp](../../mcp/po-mcp) with the same validation rules and update semantics, but as a standalone binary.

## Features

- `analyze` — statistics (translated / untranslated / fuzzy / total) + entries
- `update` — validate translations (variables, HTML, URLs, JS) and update the file
- Pretty terminal output (colored) or `--json` for piping
- Strict validation, `--dry-run`, `--force` flags
- Single static binary (release ~1MB, no runtime)

## Build

```bash
cargo build --release
# binary: ./target/release/po-cli
```

## Install

```bash
cargo install --path .
```

## Usage

### Analyze

```bash
po-cli analyze /path/to/locale/tr/LC_MESSAGES/django.po
po-cli --json analyze /path/to/django.po | jq '.statistics'
```

### Update

Translations JSON file format:

```json
[
  {"msgid": "Departure Place ID", "msgstr": "Kalkış Yeri ID", "context": null},
  {"msgid": "Arrival Place ID",   "msgstr": "Varış Yeri ID",  "context": null}
]
```

```bash
po-cli update /path/to/django.po -t translations.json
po-cli update /path/to/django.po -t translations.json --dry-run
po-cli update /path/to/django.po -t translations.json --force
po-cli update /path/to/django.po -t translations.json --no-strict
```

## Validation Checks (strict mode, default)

- **Variables**: `%(name)s`, `{0}`, `{name}`, `{{ var }}`, `{% tag %}`
- **HTML tags**: `<tag>`, `</tag>`, `<tag attr="...">`
- **URLs**: `https://...`, `www....` (must be identical)
- **JavaScript**: `console.log(...)`, `module.method(...)` (must be identical)
- **Empty translations** flagged

Use `--no-strict` to skip pattern checks.

## Workflow

```
1. po-cli --json analyze django.po > analysis.json
2. (AI translates the untranslated_entries → translations.json)
3. po-cli update django.po -t translations.json --dry-run
4. po-cli update django.po -t translations.json
```

## Claude Code Skill

This repo bundles a `skills/po-cli/SKILL.md` that drives the full translation workflow inside Claude Code. When the repo is opened (or the `skills/` dir is on a discovery path), invoke with `/po-cli` or describe the task ("django.po dosyasındaki eksikleri çevir").

Skill scope:

- Detects `.po` files via Glob
- Calls `po-cli --json analyze` and inspects untranslated/fuzzy entries
- Generates translations preserving placeholders / HTML / URLs / JS
- Runs `po-cli update --dry-run` until validation is clean
- Applies the update after user approval
- Suggests `python manage.py compilemessages` (with `djangof7` for Diji projects)

Path: [`skills/po-cli/SKILL.md`](skills/po-cli/SKILL.md)

## Dependencies

- [`clap`](https://crates.io/crates/clap) — argparse with derive macros
- [`polib`](https://crates.io/crates/polib) — GNU gettext PO read/write
- [`serde` / `serde_json`](https://crates.io/crates/serde) — JSON I/O
- [`anyhow`](https://crates.io/crates/anyhow) — error handling
- [`regex`](https://crates.io/crates/regex) — pattern extraction
- [`colored`](https://crates.io/crates/colored) — terminal colors

## License

MIT
