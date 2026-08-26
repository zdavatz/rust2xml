# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Rust port of **oddb2xml** — the Ruby tool (~14,261 LOC across 20 modules) that generates Swiss Healthcare Public Domain data files (XML, SQLite, CSV, DAT). All 20 Ruby modules have a corresponding Rust module; the crate builds clean on stable Rust, 86 unit tests + 8 integration tests pass.

Current version: **v3.1.32**.

v3.1.28 ported oddb2xml's `-r` / `--rogger` flag (the last missing output-affecting option; only `--proxy-check` remains unported). The "Rogger Mediliste" is the `GTIN,Mediname` name-conflict list maintained by Frau Rogger (Vitabyte / Zur Rose, task #OX-5985-1594): with `-r`, every listed GTIN's **German** Refdata description is replaced by the curated name, so `<DSCRD>`/`<SORTD>` in `oddb_article.xml`, `oddb_product.xml` and the Artikelstamm all pick it up. French/Italian are never touched. New `src/rogger_names.rs` mirrors `lib/oddb2xml/rogger_names.rb`: `parse(csv) -> HashMap<gtin13, name>` (CSV headers `GTIN`/`Mediname`, GTIN zero-padded to 13 via Ruby's `rjust(13,"0")`, rows with a non-numeric GTIN or empty name skipped), `looks_like_rogger_csv()` (header must carry both column names — rejects the HTML sign-in page Google serves for a non-shared sheet) and `apply(&mut Inputs)` over both `refdata_pharma` and `refdata_nonpharma`. New `RoggerDownloader` fetches the sheet's CSV export (`docs.google.com/spreadsheets/d/1NXJZ8KYzVsX0OQU767tl_AwCyvFVieHWnTWXqhflwdc/export?format=csv&gid=0`) so sheet edits reach the feeds without a release; an unusable response falls back to the bundled `data/rogger_liste.csv` embedded via `include_str!` (`BUNDLED_ROGGER`, 56 entries). `Inputs` gains `rogger_names`, populated by a new `opts.rogger`-gated cli.rs job; `Builder::new` calls `rogger_names::apply` **after** `refdata_cleanup::apply` so the curated name wins over the issue-#112 cleanups (same ordering as Ruby, where the override is the last step of `apply_refdata_description_cleanups!`). Note the live sheet has drifted from oddb2xml's bundled copy (`EZETIMIB ROSUV Spirig HC Filmtabl …` vs `EZETIMIB ROSUVA Spirig HC Filmtab …`) — that drift is the point of fetching at run time. Six unit tests (parse, padding/skip rules, sign-in-page rejection → bundled fallback, bundled parse, German-only override, no-op without the flag) + an options test + `tests/pipeline.rs::rogger_names_override_german_description_and_win_over_refdata_cleanup`, which feeds a description the double-dose cleanup *would* rewrite and asserts the Rogger name reaches `<DSCRD>` while `<DSCRF>` keeps the Refdata value.

Version history is in git, not here: `git log --oneline`, and
`git show vX.Y.Z` for a release's full rationale (each version is an
annotated tag with a detailed commit message).

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

Runtime: ~3 s fresh download / ~17 s with ZurRose's 177 K transfer.dat parse. Both the download/extract phase and the XML output phase run in parallel via `rayon` (output phase ~0.51 s parallel vs ~0.72 s serial on this machine; ZurRose's serial fixed-width parse still dominates the cached run).

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
- `rust2xml-gui` — egui desktop UI; four run buttons (`-e`, `-b`, Artikelstamm v6, Artikelstamm v5), output written to `sqlite/rust2xml_<flag>_HHMM_DD.MM.YYYY.sqlite` (flag = `e`/`b`/`as6`/`as5`), eight tabs query the resulting DB and render every column (horizontal scroll via `egui_extras::TableBuilder`). Every run also writes the standard `oddb_*.xml` files into `~/rust2xml/xml/`; the Artikelstamm buttons add `artikelstamm_v{5,6}.xml` + `.csv` there too (v5 also emits v6, like `--artikelstamm-v5`).
- `compare_v5` — diff two Artikelstamm XML files.
- `check_artikelstamm` — run semantic checks on output XML.

The crate itself is named `rust2xml` (both the library and the main
binary). Do not reintroduce `oddb2xml` as a Rust identifier — that
name belongs to the Ruby project.

## Architecture — 1:1 mapping from Ruby modules

| Ruby file | Rust module | Notes |
|---|---|---|
| `lib/oddb2xml/version.rb` | `version` | Hand-written `pub const VERSION`, **not** derived from `Cargo.toml` — bump both in the same commit. Nothing cross-checks them, and `artikelstamm.rs` stamps `VERSION` into the generated XML, so a stale constant reaches shipped data. See the `releasing` skill. |
| `lib/oddb2xml/util.rb` | `util` | GTIN checksum, HTML decode, global options, EAN ↔ ProdNo ↔ No8 maps, SHA256 hashing, Swissmedic column layout. |
| `lib/oddb2xml/options.rb` | `options` | clap-based; preserves the implied-flag cascade (`--extended` → nonpharma+calc+zurrose, `--artikelstamm` → extended+zurrose, `--fhir-url` → fhir, etc.). |
| `lib/oddb2xml/xml_definitions.rb` | `xml_definitions` | serde-quick-xml bindings matching the SAX-machine shapes. Field names stay PascalCase — `#![allow(non_snake_case)]`. |
| `lib/oddb2xml/compressor.rb` | `compressor` | tar.gz (flate2+tar) and zip (zip crate) outputs. |
| `lib/oddb2xml/downloader.rb` | `downloader` | BagXml, Refdata, Epha, LPPV, Firstbase, Swissmedic xlsx (scrapes direct URL off `listen_neu.html`), SwissmedicInfo (replays the two-step Accept.aspx form POST), Medregbm, Migel, ZurRose (zip-over-HTTP → ISO-8859-14 → UTF-8), Rogger (Google Sheet CSV export). |
| `lib/oddb2xml/extractor.rb` | `extractor` | All 11 extractors: BagXml, Refdata, LPPV, Epha CSV, Swissmedic xlsx (calamine), Swissmedic-Info HTML fragments, ZurRose fixed-width, Medreg TSV (Company/Person), Firstbase CSV. |
| `lib/oddb2xml/fhir_support.rb` | `fhir_support` | Bundle-per-line NDJSON downloader + extractor that normalizes into the same `BagItem` shape the builder expects. Default URL: `https://epl.bag.admin.ch/static/sl/publication/fhir/foph-sl-publication-latest-de.ndjson` (BAG moved the export there on 24.08.2026; the old `/static/fhir/foph-sl-export-*` answers 404 for `-latest-` while the old dated snapshots remain, so the failure looks like a run that downloads nothing). Walks `Bundle.entry[].resource` and extracts MedicinalProductDefinition / PackagedProductDefinition / Ingredient / RegulatedAuthorization / **ClinicalUseDefinition**. SL prices (`reimbursementSL.productPrice`) and limitation texts (`indication[].extension[regulatedAuthorization-limitation].limitationText`) live on the package-level RA; both are merged into `BagPrices` and `Vec<BagLimitation>` per package. `FhirExtractor::new_with_lang(ndjson, "fr"|"it")` routes the limitation text into `desc_fr`/`desc_it`; `merge_translations(primary, translation)` joins the per-language bundles by EAN-13 + per-package limitation index so `DSCRD`/`DSCRF`/`DSCIT` columns end up populated together. Cache filenames are derived from the URL so the three language files don't clobber each other. **Indikationscode (v3.1.9)**: per-bundle accumulators capture `FOPHDossierNumber` from `RA.extension[reimbursementSL].extension[FOPHDossierNumber]` and the `.NN` suffix from each `ClinicalUseDefinition.id` whose `type == "indication"`; combined codes are stored per `PackagedProductDefinition.id` and copied onto `BagPackage.indication_codes` and `BagItem.indication_codes`. The polymorphic FHIR `type` field (string for Bundle/CUD, CodeableConcept for RA) is now decoded by an `FhirType { concept, text }` wrapper; `indication` is decoded by a `deserialize_one_or_many` helper because RAs deliver an array but CUDs a single object. **Limitation keys (2026-07)**: the live feed dropped ClinicalUseDefinition resources entirely — limitation texts now sit inline under `RA.indication[].extension[regulatedAuthorization-limitation].limitationText` — so `cud_ref` is synthesized per bundle as `<PRODUCT-BASE>.<NN>` (first word of the German MPD productName uppercased + `.NN` of the Indikationscode, e.g. `ABEVMY.01`; bare base name for uncoded texts). That key feeds `LIMNAMEBAG`/`LIMCD` everywhere (`lim_code()`), `merge_translations` falls back to positional matching when refs disagree, and `build_artikelstamm(6)` registers every ARTSL-referenced code into `used_lims` so `<LIMITATIONS>` carries the per-Indikationscode descriptions (was: empty section + empty `<LIMCD/>`). Fixtures `tests/fixtures/abevmy_{de,fr}.ndjson` pin the new feed shape. |
| `lib/oddb2xml/bag_fhir_extractor.rb` | `bag_fhir_extractor` | Re-export alias of `fhir_support`. |
| `lib/oddb2xml/foph_sl_downloader.rb` | `foph_sl_downloader` | Minimal stub (the Ruby file is also a stub). |
| `lib/oddb2xml/compositions_syntax.rb` | `compositions_syntax` + `src/compositions.pest` | Pest grammar (covers common patterns — substance name + dose + unit + q.s./pro/ad/ratio modifiers, comma-separated list). |
| `lib/oddb2xml/parslet_compositions.rb` | `parslet_compositions` | `parse` / `parse_compositions` wrappers around the pest parser. |
| `lib/oddb2xml/rogger_names.rb` | `rogger_names` | `-r`/`--rogger`: curated German article names from the "Rogger Mediliste" Google Sheet (GTIN → Mediname), with a bundled `data/rogger_liste.csv` fallback. Applied to both Refdata maps in `Builder::new`, after `refdata_cleanup` so the list wins. German-only. |
| `lib/oddb2xml/refdata_cleanup.rb` | `refdata_cleanup` | Compensates for known Refdata.Articles.xml data-quality issues (currently the doubled-dose template bug). Guarded by a comma-in-`substance_swissmedic` heuristic so real combination products (PHESGO, ATOVAQUON-PROGUANIL, etc.) stay untouched. Applied automatically in `Builder::new`. See [oddb2xml issue #112](https://github.com/zdavatz/oddb2xml/issues/112). |
| `lib/oddb2xml/calc.rb` | `calc` | Static `group_by_form` / `oid_for_form` / `oid_for_group` lookup tables covering 100+ Swissmedic forms across 12 galenic groups. Ordering matters: longer substrings first (e.g. `Filmtablette` before `Tablette`) — enforced by a unit test. |
| `lib/oddb2xml/chapter_70_hack.rb` | `chapter_70_hack` | HTML table scrape producing synthetic GTINs (`FAKE_GTIN_START + pharmacode`). |
| `lib/oddb2xml/semantic_check.rb` | `semantic_check` | `every_product_number_is_unique` + `every_item_number_is_unique` over generated XML. |
| `lib/oddb2xml/builder.rb` | `builder` | 7 XML output shapes (`product`, `article`, `substance`, `limitation`, `interaction`, `code`, `calc`) + `.dat`. Uses an internal `Node` enum so emitters can produce nested children (needed for `<ART>`'s `<ARTBAR>`/`<ARTPRI>`). Each top-level child carries a `SHA256` attribute over the hex digest of its joined descendant text. |
| `lib/oddb2xml/cli.rb` | `cli` + `src/bin/rust2xml.rs` | Parallel download+extract **and** parallel XML build via rayon (`Vec<(name, fn(&Builder) -> Result<String>)>` driven by `par_iter`). FHIR-first path is the default when `--fhir` or `--fhir-url` is set; legacy BAG XML otherwise. Union of BAG + Refdata pharma + Refdata non-pharma + ZurRose + Firstbase feeds all articles. `Cli::run_to_sqlite` is the same pipeline but writes a SQLite DB instead of seven XMLs (used by `rust2xml-gui`). |
| — (new) | `sqlite_export` | Walks `Builder::*_records()` (one method per output kind), unions column names per record, creates one TEXT-typed table per kind in SQLite. Nested children (`<ARTBAR>`, repeated `<ARTPRI>`) are JSON-encoded into a single column. Filename helper `timestamped_filename(flag, now) → rust2xml_e_HHMM_DD.MM.YYYY.sqlite`. |
| — (new) | `gui` + `src/bin/rust2xml-gui.rs` | egui desktop UI. `GuiApp` owns a `crossbeam-channel` for log + progress events. All four run buttons (`-e`, `-b`, Artikelstamm v6/v5) hard-wire `opts.fhir = true`; the Artikelstamm modes mirror the CLI's implied-flag cascade. `Cli::run_to_sqlite` writes the SQLite DB and then the same XML files as the CLI via the shared `Cli::write_xml_files` helper (oddb_*.xml + Artikelstamm when requested). Worker thread runs `Cli::run_to_sqlite`, UI polls events on each frame via `request_repaint_after`. `util::set_log_sink` mirrors every `util::log()` line into the GUI log panel; `util::set_progress_sink` drives an `egui::ProgressBar`. Tabs are produced from `sqlite_master` enumeration; selected tab is loaded into a `Vec<Vec<String>>` cache and rendered with `egui_extras::TableBuilder`. Cell values collapse newlines + show full text on hover so long limitation descriptions stay readable in the 18-px row height. **Click-to-expand:** every cell is wrapped with `Sense::click()`; a click stores `(column_name, full_value)` into `selected_cell`, which renders a resizable bottom panel above the log with the untruncated value in a read-only multiline `TextEdit` (selectable + Copy button). Switching tabs clears the selection. Window icon embedded from `assets/icon.png` via `image::load_from_memory` → `egui::IconData`. |
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
- **Artikelstamm v6 emitter is data-source-pragmatic, not a 1:1 port
  of Ruby's merge.** `src/artikelstamm.rs` (v3.1.20) produces a
  schema-valid v6 (and legacy v5) output reusing the Rust `Inputs`
  union, but does not replicate every edge case of Ruby's
  `emit_items`/`add_missing_products_from_swissmedic` merge (e.g. the
  chapter-70 / Weleda-SL recovery, vaccination PRODNO patching, the
  `emit_salecd` NINCD logic). Cross-consistency semantic checks
  (`everyPharmaArticleHasAProductItem`) are not yet guaranteed.
- **RSpec port.** 16 spec files / ~6,500 lines of RSpec. Currently
  86 unit + 8 integration Rust tests cover the architectural pieces;
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
  Since v3.1.30 the downloader also has oddb2xml 3.0.29's hardening:
  `FirstbaseDownloader::is_firstbase_csv` rejects the HTML / "403 -
  Forbidden" body GS1 returns when the GetFirstbaseHealthcare export is
  unavailable, so a bad response can no longer overwrite (or truncate)
  the cached `downloads/firstbase.csv`; the last-good copy is reused
  instead and only a failure *without* a cache is an error. A present
  cache is no longer reused blindly either — that now needs
  `--skip-download`, so a normal run always tries GS1 for fresh data.

## The download cache and `--skip-download`

`~/rust2xml/downloads/` caches every raw upstream file. Since v3.1.31
`util::skip_download_cached` consults it **only under `--skip-download`**;
without the flag every source is fetched fresh.

This differs from the Ruby original on purpose. Ruby's `Oddb2xml.skip_download`
does not check the flag, because it does not have to: its `DOWNLOADS` is
`./downloads` and `Cli#run` wipes that at the start of every run *unless* the
flag is set (`cli.rb:51`), so an unflagged Ruby run always meets an empty
cache. Ours is a persistent directory under `$HOME` that nothing ever wipes, so
porting the check literally meant a file, once cached, was reused **forever** —
the nightly Artikelstamm silently rebuilt from the previous day's sources.
`scripts/run_artikelstamm.sh` papered over it with `rm -rf`, and
`FirstbaseDownloader` / `SwissmedicInfo` each grew a private
`skip_download_flag()` guard; the gate in `skip_download_cached` makes that the
rule for every downloader.

The deploy pattern is therefore the same as the Ruby nightly build's
(`run_oddb2xml.sh` `seed_downloads`): wipe the cache, drop in the one file you
want reused (the ZurRose `transfer.zip` from `get_transfer.sh`), and run with
`--skip-download` — the seed is reused and everything absent is downloaded
fresh.

## Releasing

Release mechanics — tagging, the GitHub Actions matrix build, Mac App
Store and Microsoft Store submission, store screenshots and App Store
sandbox notes — live in the `releasing` skill
(`.claude/skills/releasing/SKILL.md`), loaded on demand.

## Related Rust projects in this workspace

- `fb2sqlite` — GS1 barcode registry + MiGeL (related data source).
- `sdif` — Swiss drug interaction database.
- `swissdamed2sqlite` — Swiss medical device database.
- `pharma2merge` — pharmaceutical data merger.
