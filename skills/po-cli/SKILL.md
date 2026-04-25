---
name: po-cli
description: Django gettext .po dosyalarını analiz edip eksik/fuzzy çevirileri AI ile tamamlayan workflow skill. Kullanıcı "po dosyasını çevir", "eksik çevirileri tamamla", "django translation güncelle", "fuzzy entry'leri düzelt", ".po dosyasını analiz et", "/po-cli" dediğinde tetiklenir. po-cli Rust CLI'sini doğru komutlarla çağırır - analyze ile untranslated_entries listesini alır, Claude çevirir, validate-update ile dry-run + apply yapar, son adımda compilemessages önerir.
when_to_use: Django/gettext lokalizasyon görevleri, .po dosyası workflow, çeviri tamamlama, fuzzy entry temizleme. Tetikleme cümleleri - "django.po çevir", "tr/LC_MESSAGES güncelle", "untranslated entries", "po-cli ile çevir", "djangof7 çevirileri tamamla".
allowed-tools: Bash(po-cli *) Bash(cargo *) Bash(python3 *) Read Write Edit Glob
---

# po-cli Translation Workflow Skill

Django gettext `.po` dosyalarını analiz et + eksik msgstr'leri çevir + dosyayı güvenle güncelle.

## Önkoşul: Binary Var Mı?

Skill çalışmadan önce `po-cli` binary kurulu olmalı. Yoksa kur:

```bash
# Repo root'unda
cargo install --path .
# veya release build ile
cargo build --release && ln -sf "$(pwd)/target/release/po-cli" ~/.local/bin/po-cli
```

Doğrula:

```bash
po-cli --version
```

Komut bulunmazsa kullanıcıya `cargo install --path .` çalıştırmasını söyle, sonra devam et.

## Akış

### 1. Hedef PO Dosyasını Bul

Kullanıcı path verdiyse direkt kullan. Vermediyse Glob ile tara:

```
locale/*/LC_MESSAGES/django.po
locale/*/LC_MESSAGES/djangojs.po
locale/*/LC_MESSAGES/djangof7.po
```

Birden fazla dosya varsa `AskUserQuestion` ile hangisi sor.

### 2. Analyze

```bash
po-cli --json analyze <po_file_path> > /tmp/po-cli-analysis.json
```

Çıktı şeması:

```json
{
  "file_path": "...",
  "statistics": { "translated": N, "untranslated": N, "fuzzy": N, "total": N },
  "untranslated_entries": [{ "msgid": "...", "msgstr": "", "context": null }],
  "fuzzy_entries": [...]
}
```

İstatistiği kullanıcıya kısa bildir: `2881 translated, 2 untranslated, 0 fuzzy`.

`untranslated_entries.length === 0 && fuzzy_entries.length === 0` ise dur, "tüm entry'ler çevrili" raporla.

### 3. Çeviriyi Üret

`untranslated_entries` (ve isteğe bağlı `fuzzy_entries`) üzerinde çevir. Hedef dil dosya path'inden çıkar (`locale/tr/...` → Türkçe).

**Kritik kurallar** (validation'a takılmamak için):

- `%(name)s`, `{var}`, `{0}`, `{{ var }}`, `{% tag %}` gibi placeholder'ları **olduğu gibi** koru
- HTML tag'leri (`<b>`, `<a href="...">`) **olduğu gibi** koru
- URL'ler **birebir aynı** olsun (`https://example.com` değişmesin)
- JS kod parçaları (`console.log(...)`) **birebir aynı** olsun
- msgstr boş bırakma

Translation JSON formatı (`/tmp/po-cli-translations.json`):

```json
[
  { "msgid": "Departure Place ID", "msgstr": "Kalkış Yeri ID", "context": null },
  { "msgid": "Arrival Place ID", "msgstr": "Varış Yeri ID", "context": null }
]
```

`context` alanı msgctxt ile aynı olmalı - analyze çıktısından birebir kopyala.

### 4. Dry-Run Validate + Update

```bash
po-cli update <po_file_path> -t /tmp/po-cli-translations.json --dry-run
```

İnceleme:

- `validation.valid === true` → güvenli, gerçek update'e geç
- `validation.invalids` doluysa **her invalid entry için** issue'yu oku ve çeviriyi düzelt:
  - "Missing variables: %(name)s" → msgstr'ye placeholder eklemeyi unuttun
  - "Missing HTML tags: <b>" → tag'i ekle
  - "URL changed or missing" → URL'i kopyala
- Düzeltip translations.json'u güncelle, dry-run'ı tekrar çalıştır

`--no-strict` kullanma — validation'ı bypass etmek hatalı msgstr yazılmasına yol açar.

### 5. Onay Al

`AskUserQuestion` ile sor:

- **header**: "Update"
- **question**: "X çeviri valid. PO dosyası güncellensin mi?"
- **options**: ["Evet, uygula", "Önce çevirileri göster", "İptal"]

### 6. Gerçek Update

```bash
po-cli update <po_file_path> -t /tmp/po-cli-translations.json
```

`update.success === true` ve `update.errors` boşsa raporla:

```
Updated N translations.
```

### 7. compilemessages Öner

Django projesi ise `manage.py compilemessages` çalıştırmayı öner:

```bash
python manage.py compilemessages
```

`djangof7` domain'i varsa (Diji projeleri kuralı, bkz `~/.claude/rules/django.md`):

```bash
python manage.py makemessagesf7 -d djangof7 --all
python manage.py compilemessages
python manage.py update_translation_fields
```

`AskUserQuestion` ile çalıştırmamı ister misin sor.

## Hata Durumları

- `PO file not found` → path doğru mu kontrol et, kullanıcıdan teyit al
- `Translations JSON must be an array` → `/tmp/po-cli-translations.json` formatı bozuk, yeniden yaz
- `Failed to parse PO file` → dosya bozuk olabilir, `--no-strict` denemek yerine kullanıcıya bildir
- `Validation failed: N invalid` → çevirileri düzelt, dry-run'ı tekrar çalıştır

## Toplu Çeviri (Çok Sayıda Entry)

50+ entry varsa parça parça çevir, her batch için ayrı `--dry-run` + apply döngüsü çalıştır. Her batch sonrası `analyze` ile kalan entry sayısını doğrula.

## Komut Referansı

```bash
po-cli --help                              # Genel yardım
po-cli analyze <po>                        # İnsan okunaklı output
po-cli --json analyze <po>                 # JSON output (skill için)
po-cli update <po> -t <json>               # Validate + apply
po-cli update <po> -t <json> --dry-run     # Validate, dosya yazma
po-cli update <po> -t <json> --force       # Invalid'lere rağmen yaz (önerilmez)
po-cli update <po> -t <json> --no-strict   # Pattern check kapalı (önerilmez)
```

## İlgili Kaynaklar

- Repo README: `${CLAUDE_SKILL_DIR}/../../README.md`
- Validation kuralları: README "Validation Checks" bölümü
- Django F7 çeviri sistemi: `~/.claude/rules/django.md`
