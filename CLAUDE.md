# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Rust port of **oddb2xml** — the Ruby tool (~14,261 LOC across 20 modules) that generates Swiss drug database XML / DAT files. All 20 Ruby modules have a corresponding Rust module; the crate builds clean on stable Rust, 41 unit tests + 1 integration test pass.

Current released version: **v3.0.6** (Refdata cleanup parity with oddb2xml 3.0.5).

When bumping the version, keep `Cargo.toml` and `src/version.rs` in sync — they are checked independently and a mismatch will show up in `rust2xml --version`.

### Record-count parity with `oddb2xml -e`

Measured 2026-04-24 against oddb2xml 3.0.4, same live sources:

| File | rust2xml recs | oddb2xml recs | Δ | rust2xml size | oddb2xml size |
|---|---:|---:|---:|---:|---:|
| `oddb_interaction.xml` | 15,920 | 15,920 | **100.0%** | 12.8 MB | 14.6 MB |
| `oddb_code.xml` | 5 | 5 | **100.0%** | 0.5 KB | 1.5 KB |
| `oddb_article.xml` | 180,690 | 180,714 | **100.0%** | 108 MB | 140 MB |
| `oddb_substance.xml` | 1,389 | 1,405 | 98.9% | 0.2 MB | 0.2 MB |
| `oddb_limitation.xml` | 2,295 | 2,368 | 96.9% | 4.6 MB | 4.8 MB |
| `oddb_product.xml` | 18,162 | 17,173 | 105.8% | 13.2 MB | 15.7 MB |
| `oddb_calc.xml` | 18,162 | n/a | — | 12 MB | 41 MB |

Runtime: ~3 s fresh download / ~17 s with ZurRose's 177 K transfer.dat parse.

Schema shapes match Ruby on `<ART>` (nested `<ARTBAR>` with CDTYP / BC /
BCSTAT, multiple `<ARTPRI>` for FACTORY / PUBLIC / ZURROSE /
ZURROSEPUB), `<PRD>`, `<LIM>`, `<CAL>`, `<IX>`, `<SB>` and `<CD>`.
Every top-level child still gets a `SHA256` attribute over its
concatenated descendant text — same contract Ruby consumers rely on
via `Oddb2xml.verify_sha256`.

## Build / test

```sh
cargo build             # library + 3 binaries
cargo test              # unit + integration tests
cargo run --bin rust2xml -- --help
```

Binaries:
- `rust2xml` — main CLI.
- `compare_v5` — diff two Artikelstamm XML files.
- `check_artikelstamm` — run semantic checks on output XML.

The crate itself is named `rust2xml` (both the library and the main
binary). Do not reintroduce `oddb2xml` as a Rust identifier — that
name belongs to the Ruby project.

## Architecture — 1:1 mapping from Ruby modules

| Ruby file | Rust module | Notes |
|---|---|---|
| `lib/oddb2xml/version.rb` | `version` | |
| `lib/oddb2xml/util.rb` | `util` | GTIN checksum, HTML decode, global options, EAN ↔ ProdNo ↔ No8 maps, SHA256 hashing, Swissmedic column layout. |
| `lib/oddb2xml/options.rb` | `options` | clap-based; preserves the implied-flag cascade (`--extended` → nonpharma+calc+zurrose, `--artikelstamm` → extended+zurrose, `--fhir-url` → fhir, etc.). |
| `lib/oddb2xml/xml_definitions.rb` | `xml_definitions` | serde-quick-xml bindings matching the SAX-machine shapes. Field names stay PascalCase — `#![allow(non_snake_case)]`. |
| `lib/oddb2xml/compressor.rb` | `compressor` | tar.gz (flate2+tar) and zip (zip crate) outputs. |
| `lib/oddb2xml/downloader.rb` | `downloader` | BagXml, Refdata, Epha, LPPV, Firstbase, Swissmedic xlsx (scrapes direct URL off `listen_neu.html`), SwissmedicInfo (replays the two-step Accept.aspx form POST), Medregbm, Migel, ZurRose (zip-over-HTTP → ISO-8859-14 → UTF-8). |
| `lib/oddb2xml/extractor.rb` | `extractor` | All 11 extractors: BagXml, Refdata, LPPV, Epha CSV, Swissmedic xlsx (calamine), Swissmedic-Info HTML fragments, ZurRose fixed-width, Medreg TSV (Company/Person), Firstbase CSV. |
| `lib/oddb2xml/fhir_support.rb` | `fhir_support` | NDJSON downloader + extractor that normalizes into the same `BagItem` shape the builder expects. Default URL: `https://epl.bag.admin.ch/fhir-ch-em-epl/PackageBundle.ndjson`. |
| `lib/oddb2xml/bag_fhir_extractor.rb` | `bag_fhir_extractor` | Re-export alias of `fhir_support`. |
| `lib/oddb2xml/foph_sl_downloader.rb` | `foph_sl_downloader` | Minimal stub (the Ruby file is also a stub). |
| `lib/oddb2xml/compositions_syntax.rb` | `compositions_syntax` + `src/compositions.pest` | Pest grammar (covers common patterns — substance name + dose + unit + q.s./pro/ad/ratio modifiers, comma-separated list). |
| `lib/oddb2xml/parslet_compositions.rb` | `parslet_compositions` | `parse` / `parse_compositions` wrappers around the pest parser. |
| `lib/oddb2xml/refdata_cleanup.rb` | `refdata_cleanup` | Compensates for known Refdata.Articles.xml data-quality issues (currently the doubled-dose template bug). Guarded by a comma-in-`substance_swissmedic` heuristic so real combination products (PHESGO, ATOVAQUON-PROGUANIL, etc.) stay untouched. Applied automatically in `Builder::new`. See [oddb2xml issue #112](https://github.com/zdavatz/oddb2xml/issues/112). |
| `lib/oddb2xml/calc.rb` | `calc` | Static `group_by_form` / `oid_for_form` / `oid_for_group` lookup tables covering 100+ Swissmedic forms across 12 galenic groups. Ordering matters: longer substrings first (e.g. `Filmtablette` before `Tablette`) — enforced by a unit test. |
| `lib/oddb2xml/chapter_70_hack.rb` | `chapter_70_hack` | HTML table scrape producing synthetic GTINs (`FAKE_GTIN_START + pharmacode`). |
| `lib/oddb2xml/semantic_check.rb` | `semantic_check` | `every_product_number_is_unique` + `every_item_number_is_unique` over generated XML. |
| `lib/oddb2xml/builder.rb` | `builder` | 7 XML output shapes (`product`, `article`, `substance`, `limitation`, `interaction`, `code`, `calc`) + `.dat`. Uses an internal `Node` enum so emitters can produce nested children (needed for `<ART>`'s `<ARTBAR>`/`<ARTPRI>`). Each top-level child carries a `SHA256` attribute over the hex digest of its joined descendant text. |
| `lib/oddb2xml/cli.rb` | `cli` + `src/bin/rust2xml.rs` | Parallel download+extract via rayon; FHIR-first path is the default when `--fhir` or `--fhir-url` is set; legacy BAG XML otherwise. Union of BAG + Refdata pharma + Refdata non-pharma + ZurRose + Firstbase feeds all articles. |
| `lib/oddb2xml/compare.rb` | `compare` + `src/bin/compare_v5.rs` | GTIN-keyed diff of two output XMLs. |

## Hard-problem mapping

| Ruby technology | Rust replacement |
|---|---|
| `nokogiri` / `sax-machine` | `quick-xml` + `serde` (+ `strip_default_namespace` helper) |
| `optimist` | `clap` with derive |
| `mechanize` | `reqwest` with `cookie_store` |
| `rubyXL` + `spreadsheet` | `calamine` (one crate, both xls/xlsx) |
| `rubyzip` / `minitar` | `zip` crate + `tar` + `flate2` |
| `parslet` | `pest` grammar in `src/compositions.pest` |
| `htmlentities` | `html-escape` |
| Ruby threads + Mutex in CLI | `rayon::par_iter` over `Mutex<Inputs>` |
| ISO-8859-14 transfer.dat | `encoding_rs::WINDOWS_1252` |

## Known limitations vs. the Ruby gem (to-do list)

- **Composition grammar is permissive.** The pest grammar accepts the
  common patterns in Swissmedic's `Zusammensetzung` column but does
  not reproduce every Parslet quirk (fix-coded identifiers like
  `F.E.I.B.A.`, radio isotopes like `Xenonum (133-Xe)`, etc.).
- **No NTLM / SOAP.** `MedregbmDownloader` uses plain HTTP; if the
  endpoint regresses to NTLM we need an `ntlm-auth` crate dance.
- **No Artikelstamm v3/v5 XML emitter.** The `--artikelstamm` flag
  wires up the right inputs (extended + ZurRose) but the actual
  `Elexis_Artikelstamm_v5.xsd`-compliant output shape is not yet
  produced.
- **RSpec port.** 16 spec files / ~6,500 lines of RSpec. Currently
  41 unit + 1 integration Rust tests cover the architectural pieces;
  per-file RSpec parity is not yet complete.
- **`oddb_calc.xml` content density still trails Ruby** (12 MB vs
  41 MB). Record count is in the right ballpark; the gap is in
  composition richness — the Ruby builder pulls composition detail
  from several extra sources.

### Resolved (prior debt)

- ART schema — now uses Ruby's nested `<ARTBAR>`/`<ARTPRI>` shape.
- Galenic form table — expanded from ~20 entries to 100+ across 12
  groups (Tabletten, Kapseln, Parenteralia, Oralia flüssig,
  Ophthalmica, Otica, Nasalia, Externa, Suppositorien, Vaginalia,
  Pulver, Inhalanda).
- ZurRose loading — CRLF handling bug fixed; all 177 K transfer.dat
  rows now extract correctly.
- Firstbase — wired into `-b` pipeline as the 5th article source.

## Releasing

Release pipeline lives in `.github/workflows/release.yml`. It triggers
on any tag matching `vX.Y.Z` (or `vX.Y.Z-rc.N` for pre-releases) and
produces archives for:

- `x86_64-unknown-linux-gnu`
- `aarch64-unknown-linux-gnu` (cross-compiled)
- `x86_64-apple-darwin` (native on `macos-13`)
- `aarch64-apple-darwin` (native on `macos-latest`)
- `x86_64-pc-windows-msvc`

Each archive bundles `rust2xml`, `compare_v5`, `check_artikelstamm`,
`README.md`, `LICENSE` and ships with a `.sha256` sidecar.  The
workflow uploads everything to a GitHub Release with auto-generated
notes.  Bumping the patch version is the normal release cadence:
edit `Cargo.toml` version → commit → `git tag vX.Y.Z` → `git push
origin vX.Y.Z`.

The workflow also has a `workflow_dispatch` trigger so releases can
be re-run by hand from the Actions tab if an upload fails midway.

## Related Rust projects in this workspace

- `fb2sqlite` — GS1 barcode registry + MiGeL (related data source).
- `sdif` — Swiss drug interaction database.
- `swissdamed2sqlite` — Swiss medical device database.
- `pharma2merge` — pharmaceutical data merger.
