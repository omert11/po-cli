# po-cli

Django gettext `.po` dosyaları için Rust CLI. Single static binary — analyze stats, validate translations (variables, HTML, URLs, JS), update in place.

## Stack

- **Dil**: Rust 2021 edition
- **Build**: `cargo` (rustc 1.94+)
- **Bağımlılıklar**:
  - `clap` 4.6 (derive feature) — argparse + subcommand
  - `polib` 0.3 — GNU gettext PO read/write
  - `serde` + `serde_json` — JSON I/O
  - `anyhow` — error wrapping
  - `regex` — pattern extraction (variables/HTML/URL/JS)
  - `colored` — terminal renkli output

## Dil

Türkçe iletişim, İngilizce kod yorumu + commit mesajı.

## Komutlar

```bash
cargo build                    # debug build
cargo build --release          # release (LTO + strip + opt-z, ~1.4MB)
cargo run -- analyze <po>      # geliştirme sırasında çalıştır
cargo clippy --all-targets     # lint
cargo fmt --all                # format
cargo test                     # test
```

Binary kullanımı:

```bash
po-cli analyze <po_file>
po-cli --json analyze <po_file>
po-cli update <po_file> -t translations.json [--dry-run|--force|--no-strict]
```

## Proje Yapısı

```
src/
├── main.rs              clap derive parser, --json global flag
├── types.rs             serde structs (PoEntry, ValidationResult, vb.)
├── parser/po_parser.rs  polib wrapper + #~| preprocess
├── validator.rs         regex pattern check (vars/HTML/URL/JS)
└── commands/
    ├── analyze.rs       statistics + untranslated/fuzzy listele
    └── update.rs        validate + apply (dry-run/force destekli)

skills/po-cli/SKILL.md   Claude Code skill (translation workflow)
.github/workflows/       CI (rustfmt + clippy + test) + Release (multi-target binary)
```

## Kod Konvansiyonları

- `cargo fmt` ile formatla, CI bu flag'i zorlar
- `cargo clippy --all-targets --all-features -- -D warnings` temiz olmalı
- Hatalar `anyhow::Result` ile döner, üst katmana `with_context` ile zenginleştirilir
- `polib` `MessageView` / `MessageMutView` trait'leri import edilmeden `Message` üzerinde method çağrılamaz — `use polib::message::{MessageView, MessageMutView}` lazım

## Skill

`skills/po-cli/SKILL.md` Claude Code skill'i:

- Glob ile `.po` dosyalarını bulur
- `po-cli --json analyze` çağırır
- Untranslated/fuzzy entry'leri AI ile çevirir (placeholder/HTML/URL/JS koruma)
- `--dry-run` validate döngüsü → temizse onay alıp uygula
- `manage.py compilemessages` (+ Diji projeleri için `djangof7`) önerir

Tetik: `/po-cli`, "django.po çevir", "fuzzy entry düzelt".

## Release

Tag push → GitHub Actions multi-target build (Linux x86_64/aarch64, macOS x86_64/aarch64, Windows x86_64) + GitHub Release.

```bash
git tag v0.1.0
git push origin v0.1.0
```
