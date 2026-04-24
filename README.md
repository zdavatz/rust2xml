# rust2xml

Swiss drug database XML / DAT generator — pulls from public sources
(Refdata, BAG/FOPH FHIR, Swissmedic, ZurRose, EPha, Migel, Firstbase)
and emits a bundle of XML files plus an optional legacy `.dat`.

Functional successor to the [oddb2xml](https://github.com/zdavatz/oddb2xml)
Ruby gem, written in Rust.

## Record-count parity with oddb2xml -e

Measured on 2026-04-24 against oddb2xml 3.0.4, same live data sources.

| File | rust2xml | oddb2xml | Delta |
|---|---:|---:|---:|
| `oddb_interaction.xml` | 15,920 | 15,920 | **100.0%** |
| `oddb_code.xml` | 5 | 5 | **100.0%** |
| `oddb_article.xml` | 180,690 | 180,714 | **100.0%** |
| `oddb_substance.xml` | 1,389 | 1,405 | 98.9% |
| `oddb_limitation.xml` | 2,295 | 2,368 | 96.9% |
| `oddb_product.xml` | 18,162 | 17,173 | 105.8% |

Runtime: **~3 s** fresh download, **~17 s** including ZurRose's 177 K
transfer.dat parse. Well under a minute end-to-end.

## Build

```sh
cargo build --release
```

Three binaries land in `target/release/`:

- `rust2xml` — main CLI.
- `compare_v5` — diff two Artikelstamm-style XML files.
- `check_artikelstamm` — validate unique PRODNO/GTIN in an output XML.

## Quick start

```sh
# XML (default)
./target/release/rust2xml

# Extended pharma + non-pharma + ZurRose prices + oddb_calc.xml
./target/release/rust2xml -e

# Use the new FHIR NDJSON feed instead of BAG XML
./target/release/rust2xml --fhir

# Artikelstamm v3/v5 (Elexis ≥ 3.1)
./target/release/rust2xml --artikelstamm

# Cache downloads — re-uses files already under ./downloads/
./target/release/rust2xml -e --skip-download --log
```

## Generated files (XML mode)

- `oddb_product.xml`
- `oddb_article.xml`
- `oddb_substance.xml`
- `oddb_limitation.xml`
- `oddb_interaction.xml`
- `oddb_code.xml`
- `oddb_calc.xml` (when `-e` / `--calc` / `--firstbase` / `--artikelstamm`)

Every top-level element in each file carries a `SHA256` attribute whose
value is the hex digest of the element's text content, so consumers can
detect unchanged nodes between runs (same contract as the Ruby gem).

## Option parity with the Ruby gem

Every flag from `lib/oddb2xml/options.rb` has a 1:1 Rust equivalent,
including optimist's auto-assigned short flags:

| Flag | Short | Purpose |
|---|---|---|
| `--append` | `-a` | Additional target nonpharma |
| `--artikelstamm` | | Create Artikelstamm v3/v5 for Elexis ≥ 3.1 |
| `--compress-ext <FMT>` | `-c` | `tar.gz` or `zip` |
| `--extended` | `-e` | Pharma + non-pharma + ZurRose + `oddb_calc.xml` |
| `--fhir` | | Use FOPH/BAG FHIR NDJSON feed |
| `--fhir-url <URL>` | | Custom FHIR NDJSON URL (implies `--fhir`) |
| `--format <FMT>` | `-f` | `xml` (default) or `dat` |
| `--include` | `-i` | EAN14 for `dat` format |
| `--increment <PCT>` | `-I` | Price increment %; forces `-f dat -p zurrose` |
| `--fi` | `-o` | Optional Fachinfo output |
| `--price [<SRC>]` | `-p` | Price source (default `zurrose`) |
| `--tag-suffix <S>` | `-t` | XML tag suffix + filename prefix |
| `--context <CTX>` | `-x` | `product` (default) or `address` |
| `--calc` | | Only `oddb_calc.xml` |
| `--skip-download` | | Reuse cached downloads |
| `--log` | | Log important actions |
| `--use-ra11zip <PATH>` | | Use a zipped `transfer.dat` from Galexis |
| `--firstbase` | `-b` | NONPHARMA via GS1 Switzerland CSV |

Implied-flag cascade (same behaviour as Ruby):
- `--increment N` → sets `nonpharma`, `price=zurrose`, `ean14=true`, `percent=N`
- `--firstbase` → sets `nonpharma`, `calc`
- `--extended` → sets `nonpharma`, `price=zurrose`, `calc`
- `--artikelstamm` → sets `extended`, `price=zurrose`
- `--fhir-url` → sets `fhir`
- `-f xml` → forces `ean14=true`
- `-x address` / `-x addr` → `address=true`

## Test

```sh
cargo test              # unit + integration
```

23+ option-parity tests, per-module unit tests, and an end-to-end
integration test that runs a canned BAG XML fixture through the
extractor → builder chain and asserts SHA256 attributes are emitted.

## Architecture

See `CLAUDE.md` for the full 1:1 Ruby → Rust module mapping, the
replacement crates for each Ruby gem, and the documented porting debt.

## Releases

Pre-built binaries for **Linux (x86_64 + aarch64)**, **macOS (Intel +
Apple Silicon)** and **Windows (x86_64)** are attached to every GitHub
Release. Each archive contains `rust2xml`, `compare_v5`,
`check_artikelstamm`, README and LICENSE, plus a `.sha256` file.

### Cutting a release

Bump `version` in `Cargo.toml` (e.g. `3.0.4` → `3.0.5`), commit, then
push a `vX.Y.Z` tag:

```sh
# bump patch version in Cargo.toml, commit, then:
git tag v3.0.5
git push origin v3.0.5
```

The `.github/workflows/release.yml` pipeline then:
1. runs `cargo test --all --release` on Linux,
2. builds release binaries on all five targets in parallel,
3. packages them as `.tar.gz` (Unix) / `.zip` (Windows) with
   accompanying `.sha256` files,
4. creates (or updates) a GitHub Release for the tag with
   auto-generated release notes.

Pre-release tags (e.g. `v3.0.5-rc.1`) are marked as pre-release
automatically. The workflow can also be dispatched manually from the
Actions tab.

## License

GPL-3.0-only, inherited from oddb2xml.
