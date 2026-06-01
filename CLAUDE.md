# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Rust port of **oddb2xml** — the Ruby tool (~14,261 LOC across 20 modules) that generates Swiss Healthcare Public Domain data files (XML, SQLite, CSV, DAT). All 20 Ruby modules have a corresponding Rust module; the crate builds clean on stable Rust, 60 unit tests + 3 integration tests pass.

Current released version: **v3.1.18** — FHIR is now the default source for `-e`/`--extended` and `-b`/`--firstbase` since 01.06.2026. The clap raw struct gains a `--no-fhir` flag; after the implied-flag cascade, `options.rs` sets `opts.fhir = true` when `extended || firstbase` unless the user passed `--fhir` (already true) or `--no-fhir`. Plain runs (no `-e`/`-b`) are unchanged and stay non-FHIR. The GUI already hard-wired `--fhir` for both Run buttons, so this only changes CLI defaults. Four new unit tests cover `-e`/`-b` defaulting on, `--no-fhir` opting out, and plain runs staying off. Mirrors oddb2xml 3.0.9.

Previously v3.1.17 — fixes empty `<DSCRD>` / `<DSCRF>` / `<DSCRI>` on every `<Limitation>` in `oddb_limitation.xml` under `--fhir`. The live BAG FHIR feed never carries the limitation text inline on the `regulatedAuthorization-limitation` extension; it holds a `limitationIndication` reference to a `ClinicalUseDefinition` whose `indication.diseaseSymptomProcedure.concept.text` is the actual text — one per language. `parse_limitation` now captures the reference (stripping the `ClinicalUseDefinition/` prefix) into a new `BagLimitation::cud_ref`. The bundle loop in `FhirExtractor::to_hash` builds a per-bundle `cud_id → text` map covering all indication CUDs (`.NN` or not) and patches `desc_de` on each pack's limitations once both the RAs and CUDs of the bundle have been parsed. The FR/IT extraction path goes through the same code, so the language-rotation post-pass moves the resolved text into `desc_fr` / `desc_it`. `merge_translations` now matches by `cud_ref` first (falling back to positional index when the ref is absent) so the merge is robust against re-ordering. Live FOPH coverage: 5,963 / 5,963 limitations (100%), up from 0 / 5,963. New integration test `tests/pipeline.rs::cyramza_fhir_fills_limitation_descriptions_in_all_three_languages` rewrites the CUD text of the CYRAMZA fixture per language via `serde_json::Value` and asserts the merge end-to-end. Aligned with oddb2xml 3.0.8 — see issue [#116](https://github.com/zdavatz/oddb2xml/issues/116).

Previously v3.1.16 — release-pipeline fixes layered on top of v3.1.15.  Two changes, neither touches the binary behaviour:

1. **Drop unused `suppaftp` dependency** in `Cargo.toml`.  v3.1.13 switched `reqwest` to `rustls-tls` to fix the aarch64 cross-compile but missed `suppaftp = { version = "6", features = ["native-tls"] }`, which still pulled in `openssl-sys` transitively.  ZurRose's `transfer.dat` is downloaded via plain HTTP, so the FTP client has been dead code for a while; removing it eliminates the last `openssl-sys` consumer (`cargo tree -i openssl-sys` now reports "did not match any packages") and unblocks the `aarch64-unknown-linux-gnu` build job that failed on v3.1.13/v3.1.14/v3.1.15 with `Could not find directory of OpenSSL installation`.
2. **Microsoft Store submission API: empty POST body.**  The devcenter API now rejects `POST /submissions` with a body that drops existing `applicationPackages`, returning `Please keep all file entries for existing packages. If you wish to remove a package, mark it as PendingDelete. The following packages are missing in your update: <id>`.  Per [the documented contract](https://learn.microsoft.com/en-us/windows/uwp/monetize/create-an-app-submission), `POST /submissions` takes no body — it clones the last published submission verbatim — and all overrides (listings, pricing, package transitions) belong in the subsequent `PUT`.  v3.1.15 was sending `applicationPackages = @()` in the create body which dropped a package the API expected to be preserved.  `submit to Microsoft Store` step in `.github/workflows/release.yml` now POSTs with no body, then in PUT marks every cloned package `PendingDelete` and adds the new MSIX with `PendingUpload`.

Apple resubmission for Guideline 4: the v3.1.15 `.pkg` was already uploaded successfully to App Store Connect and contains the GUI warning dialog + elapsed-time counter described below.  v3.1.16 will upload another `.pkg` carrying the same UX changes.

Previously v3.1.15 — GUI first-time download warning + elapsed-time counter, addressing Apple App Review Guideline 4 (Design) feedback that the `Run -e (Extended)` job downloaded ~250 MB without warning the user.  Two GUI-only changes in `src/gui.rs`:

1. **One-time confirmation modal.** Clicking **Run -e** or **Run -b** now parks the request in a new `pending_run: Option<RunMode>` state.  On the next frame, if the per-user marker file `~/rust2xml/.gui_warning_ack` is absent, an `egui::Window` (anchored center, non-resizable, non-collapsible) explains the download size (~200–300 MB), expected duration (10–30 min on a typical home connection), what's downloaded (BAG / Swissmedic / Refdata / EPha / LPPV / ZurRose-or-Firstbase), that subsequent runs reuse the cache, and that an internet connection is required.  **Continue** writes the marker file via `util::mark_gui_warning_acknowledged()` and starts the run; **Cancel** clears `pending_run`.  After the first acknowledged click, the modal is skipped on every future launch (the marker survives app restarts).  Removing `~/rust2xml/` resets consent.  New util helpers `gui_warning_acknowledged()` / `mark_gui_warning_acknowledged()` and a private `gui_warning_ack_path()` live in `src/util.rs` next to the home-anchored data-dir helpers.
2. **Elapsed-time counter.** New `run_started_at: Option<Instant>` field captures the wall-clock when `start_run` fires and is cleared in `drain_events` when the run finishes.  The "Running …" label next to the spinner now reads `Running -e (Extended) — elapsed 03:42` (or `1:23:45` past one hour) via a new `format_elapsed(seconds)` helper, so reviewers can immediately tell the worker isn't hung even during the long ZurRose / FHIR phases.

No CLI changes.  No pipeline changes.  Headless `Cli::run_to_sqlite` and the `rust2xml` binary behave identically to v3.1.14 — the warning is purely a GUI-side gate.  All 60 unit tests + 3 integration tests still pass.

Previously v3.1.14 — added a new `--indc-xlsx <PATH>` CLI flag that emits a BAG Indikationscode XLSX export. One row per `(XXXXX.NN code, GTIN)` pair with eight columns: `Indikationscode`, `Markenname`, `GTIN`, `Pack-Beschreibung`, `ATC`, `Preis Ex-Factory`, `Preis Publikum`, `Indikation` (the multi-paragraph limitation text). Sorted by code, brand, GTIN; header row frozen and bold; the indication column wraps. Implies `--fhir`. Implementation lives in the new `src/indc_xlsx.rs` module via a new `rust_xlsxwriter` dependency; `Builder` is reused unchanged because the data already sits on `BagPackage.indication_codes`. Two unit tests cover row collection and workbook write against the CYRAMZA fixture. On the live FOPH feed (May 2026) the workbook contains 1,419 data rows. A committed sample built from that feed lives at `xlsx/indc.xlsx` (~177 KB) so reviewers can inspect the format without running the pipeline. Useful for pharmacists/insurers who want a flat code-and-price table rather than the full XML/SQLite outputs.

Previously v3.1.13 — release-pipeline fixes so end users actually get tarballs/zips on the GitHub Releases page.  Two changes:

1. **`reqwest` switched to `rustls-tls`** in `Cargo.toml`.  v3.1.10–v3.1.12 used the `native-tls` feature, which pulls in `openssl-sys` and breaks the `aarch64-unknown-linux-gnu` cross-compile (the Ubuntu x86_64 runner only ships `gcc-aarch64-linux-gnu`, not arm64 OpenSSL headers — `error: failed to run custom build command for openssl-sys`).  `rustls-tls` is a pure-Rust TLS stack so the cross-compile no longer needs system OpenSSL on any target.  Added `rustls-tls-native-roots` so the client still trusts the platform CA store on macOS / Windows / Linux.
2. **`publish` job gate relaxed** in `.github/workflows/release.yml`.  Was `if: always() && needs.build.result == 'success'`, which required the *entire* build matrix to be green — a single failed cross-compile blocked the GitHub Release for everyone, even targets that built fine.  Now `if: ${{ !cancelled() }}` so `softprops/action-gh-release@v2` runs whenever the workflow wasn't cancelled and publishes whatever artifacts the matrix actually uploaded.  v3.1.10/v3.1.11/v3.1.12 hit this trap — tags are on GitHub, but `gh release view vX.Y.Z` returned "release not found" because the publish step was gated off.

  App Store Connect uploads via `iTMSTransporter`/`altool` were unaffected (the `macos-store` job is independent of `publish`); v3.1.11 made it onto Apple's side fine.

Previously v3.1.12 surfaced the BAG **Indikationscode** *and* its limitation text in the GUI/XML output.  Two fixes layered on top of v3.1.11:

1. **`INDIKATIONSCODE_TEXT` column.**  v3.1.11 emitted the `XXXXX.NN` codes but only the codes — `BagIndicationCode.text` was carried through the in-memory struct and dropped.  v3.1.12 adds an `INDIKATIONSCODE_TEXT` leaf to PRD/ART/LIM rows containing newline-joined `XXXXX.NN: <limitation text>` lines (one per code that carries text), via a new `join_indication_code_texts()` helper in `src/builder.rs`.  `ArtFields.indikationscode_text` mirrors `ArtFields.indikationscodes`; both are emitted only when non-empty.
2. **CUD text parsing fix.**  The CUD's indication text actually lives at `indication.diseaseSymptomProcedure.concept.text` (CodeableReference shape), not under `indication.extension[url=limitationText].valueString` as v3.1.11 assumed.  The old path was always empty for ClinicalUseDefinition resources, so `BagIndicationCode.text` was always blank — making the `INDIKATIONSCODE_TEXT` column blank in the GUI.  `src/fhir_support.rs:557-580` now reads `disease_symptom_procedure.concept.text` first, falling back to `indication.extension[limitationText]` so the same path still works for RA-shape resources.

Tests: `tests/pipeline.rs::fhir_extracts_indikationscodes_from_cyramza_bundle` now asserts the per-code texts contain Paclitaxel (CYRAMZA.01) and FOLFIRI (CYRAMZA.02); `cyramza_bundle_emits_indikationscode_into_product_article_limitation` asserts the texts survive into PRD + ART XML output.

GUI behaviour: click-to-expand in the GUI now shows the full multi-paragraph indication text per code in the bottom detail panel.  Useful for the BAG Rundschreiben use case where pharmacists need to see *which* indication a given `XXXXX.NN` covers.

Previously:
- v3.1.11 — first plumbing of the BAG Indikationscode through the builder.  `Builder::product_nodes` added `INDIKATIONSCODE` per PRD row from `pkg.indication_codes` (fallback `item.indication_codes`), comma-joined and deduped via `join_indication_codes()`.  `Builder::article_nodes` added an optional `<INDIKATIONSCODE>` child to ART rows for the BAG/FHIR branch (branches 2-5 — refdata-only, ZurRose-only, Firstbase — pass empty and the leaf is suppressed).  `ArtFields` gained an `indikationscodes: String` field; the `into_nodes()` writer emits the leaf only when non-empty.  `Builder::limitation_records` added `INDIKATIONSCODE` to every LIM row.  Integration test `cyramza_bundle_emits_indikationscode_into_product_article_limitation` ran the CYRAMZA fixture through extractor → builder and asserted `<INDIKATIONSCODE>20403.0...` reached product/article/limitation XML.  Bug: the CUD limitation text was never extracted (wrong JSON path); fixed in v3.1.12.
- v3.1.10 — GUI/CLI download UX overhaul.  Three changes inside `download_as` + the BAG/FHIR job:

1. **Parallel FHIR de/fr/it** (`src/cli.rs:138-191`). The three language bundles now download and extract concurrently via `langs.par_iter()` inside the single BAG (FHIR de/fr/it) job, instead of sequentially DE → FR → IT. DE remains fatal on failure; FR/IT failures are logged and the run continues. Without this, IT was the lonely tail-end download — every other parallel source (Refdata, EPha, LPPV, Swissmedic, ZurRose, Firstbase) had finished by the time IT started, which is why the GUI progress bar appeared to "hang" at 69% with no other activity in the log.
2. **Chunked streaming download with progress logging** (`src/downloader.rs:79-126`). `download_as` now reads the HTTP response body in 64 KB chunks via the `std::io::Read` impl on `reqwest::blocking::Response` instead of `read_to_end`. When `Content-Length` ≥ 5 MB, each 10 MB boundary logs `<file>: <mb> MB / <total> MB (<pct>%)`; if the server omits Content-Length (e.g. Firstbase), each 10 MB boundary logs `<file>: <mb> MB downloaded (no Content-Length)`. Files under 5 MB stay quiet.
3. **`util::progress_label(label)`** (`src/util.rs:210-235`). New API that updates the GUI progress bar's caption without moving the fraction, so big downloads can publish live status without inflating the per-job progress accounting. The chunked download loop calls it on every logged milestone — the bar now reads e.g. `69% — foph-sl-export-latest-de.ndjson: 60 MB / 89 MB (67%)` while data streams in, instead of staying frozen on the previous job's "done (5/6)" caption. `set_progress_sink(None)` resets the stored fraction so a stale value doesn't leak into the next run.

  Note: `download_as` short-circuits via `util::skip_download_cached` when `~/rust2xml/downloads/<file>` exists — on cached runs the streaming branch is never reached and no MB lines appear (the `skip_download: reused cached <path>` line is the signal). To exercise the streaming code, blow away the cache: `rm -rf ~/rust2xml/downloads/* ~/rust2xml/foph-*.ndjson ~/rust2xml/transfer.zip ~/rust2xml/Refdata.Articles.zip`. Both `-e` and `-b` modes go through the same `cli.rs:280` `jobs.par_iter()` orchestrator; only the job set differs (`-e` includes ZurRose, `-b` includes Firstbase, both include FHIR).

- v3.1.9 — extracts the BAG **Indikationscode** (`XXXXX.NN`) from FHIR `ClinicalUseDefinition` resources for SL price-model drugs. Built bundle-locally by joining the reimbursement RegulatedAuthorization's `FOPHDossierNumber` (`XXXXX`) with each sibling CUD's `.NN` id-suffix (e.g. `CYRAMZA.01`/`CYRAMZA.02` → `20403.01`/`20403.02`). Exposed via a new `BagIndicationCode { code, cud_id, text }` field `indication_codes` on both `BagItem` and `BagPackage`.  v3.1.9 only populated the in-memory structs — surfacing the codes through the builder shipped in v3.1.11.  BAG Rundschreiben 2026-02-19 makes the code mandatory on prescriptions and invoices for ~170 SL price-model drugs from 2026-07-01; from 2027-01-01 insurers may reject invoices without it. See [oddb2xml issue #113](https://github.com/zdavatz/oddb2xml/issues/113). Same change as oddb2xml 3.0.6.

**Mac App Store status (as of 2026-04-26):** v3.1.8 `.pkg` accepted by App Store Connect binary scan — first build to clear the private-API gate.  Listing metadata is populated; macOS screenshots (`screenshots/macos/`, 8 × 2560×1600) ready to upload via the App Store Connect web UI.  Ready to submit for App Review.
- v3.1.8 — App Store private-API compliance (patched `winit` 0.30.13). winit's `set_blur` calls the private CoreGraphics symbol `_CGSSetWindowBackgroundBlurRadius` (winit issue #4538), which Apple's binary scanner rejects. We point eframe → egui-winit → winit at a local fork at `winit-patched/` via `[patch.crates-io] winit = { path = "winit-patched" }` in `Cargo.toml`; the fork's `set_blur` is a no-op and the `objc2::ffi::NSInteger` / `objc2::runtime::AnyObject` imports that only existed for that extern are commented out so CI's `RUSTFLAGS=-Dwarnings` still compiles. Same fix as `swissdamed2sqlite` and `eudamed2firstbase`. App Store metadata is populated bilingually (de-DE + en-US) directly from `/Users/zdavatz/software/storetemplate/json/rust2xml.json` via the App Store Connect REST API.
- v3.1.7 — first end-to-end Mac App Store + Microsoft Store release pipeline. Apple Developer App ID `com.ywesee.rust2xml` (resource `9ZSB7L347R`) registered via `POST /v1/bundleIds`; Mac App Store provisioning profile `Q7J2TQ7QL5` (replaces `TZDK923Q84`) created via `POST /v1/profiles` referencing both `Mac App Distribution` certs `Q9NT43TUTX` + `H9YRY3BBDQ` (the iCloud `.p12` is `Q9NT43TUTX`); App Store Connect app record `6763883287`. Microsoft Partner Center reservation `9NTKS4TBRLF6` (`yweseeGmbH.rust2xml`) wired up via the devcenter REST API — first submission committed but Microsoft requires screenshots + IARC age rating before publication. Workflow validation: replaced unsupported `secrets.X` step-level `if:` predicates with the standard `env:`-then-`if: env.X != ''` pattern, and bumped the deprecated `macos-13` runner to `macos-15-intel`.
- v3.1.6 — every output the CLI and GUI write now lives under `~/rust2xml/`: SQLite snapshots in `~/rust2xml/sqlite/`, XML in `~/rust2xml/xml/`, raw upstream caches in `~/rust2xml/downloads/`. Resolved via `dirs::home_dir()` so the same code path also targets the per-app container under `~/Library/Containers/com.ywesee.rust2xml/Data/` once the Mac App Store sandbox is enabled — no further branching needed. The GUI top panel gained an **📂 Open Data Folder** button that reveals `~/rust2xml/` in Finder / Explorer / `xdg-open`.
- v3.1.5 — GUI per-tab search box (case-insensitive substring across every column); products + articles `DSCRD`/`DSCRF` resolve via refdata-first fallback chain so brand-name searches like `PONSTAN` find rows in FHIR mode.
- v3.1.4 — macOS `.app` bundle with `.icns` icon, Linux `.desktop` + `install-linux.sh`, clickable mailto badge.

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
- `rust2xml-gui` — egui desktop UI; two big buttons for `-e` / `-b` runs, output written to `sqlite/rust2xml_<flag>_HHMM_DD.MM.YYYY.sqlite`, eight tabs query the resulting DB and render every column (horizontal scroll via `egui_extras::TableBuilder`).
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
| `lib/oddb2xml/fhir_support.rb` | `fhir_support` | Bundle-per-line NDJSON downloader + extractor that normalizes into the same `BagItem` shape the builder expects. Default URL: `https://epl.bag.admin.ch/static/fhir/foph-sl-export-latest-de.ndjson`. Walks `Bundle.entry[].resource` and extracts MedicinalProductDefinition / PackagedProductDefinition / Ingredient / RegulatedAuthorization / **ClinicalUseDefinition**. SL prices (`reimbursementSL.productPrice`) and limitation texts (`indication[].extension[regulatedAuthorization-limitation].limitationText`) live on the package-level RA; both are merged into `BagPrices` and `Vec<BagLimitation>` per package. `FhirExtractor::new_with_lang(ndjson, "fr"|"it")` routes the limitation text into `desc_fr`/`desc_it`; `merge_translations(primary, translation)` joins the per-language bundles by EAN-13 + per-package limitation index so `DSCRD`/`DSCRF`/`DSCIT` columns end up populated together. Cache filenames are derived from the URL so the three language files don't clobber each other. **Indikationscode (v3.1.9)**: per-bundle accumulators capture `FOPHDossierNumber` from `RA.extension[reimbursementSL].extension[FOPHDossierNumber]` and the `.NN` suffix from each `ClinicalUseDefinition.id` whose `type == "indication"`; combined codes are stored per `PackagedProductDefinition.id` and copied onto `BagPackage.indication_codes` and `BagItem.indication_codes`. The polymorphic FHIR `type` field (string for Bundle/CUD, CodeableConcept for RA) is now decoded by an `FhirType { concept, text }` wrapper; `indication` is decoded by a `deserialize_one_or_many` helper because RAs deliver an array but CUDs a single object. |
| `lib/oddb2xml/bag_fhir_extractor.rb` | `bag_fhir_extractor` | Re-export alias of `fhir_support`. |
| `lib/oddb2xml/foph_sl_downloader.rb` | `foph_sl_downloader` | Minimal stub (the Ruby file is also a stub). |
| `lib/oddb2xml/compositions_syntax.rb` | `compositions_syntax` + `src/compositions.pest` | Pest grammar (covers common patterns — substance name + dose + unit + q.s./pro/ad/ratio modifiers, comma-separated list). |
| `lib/oddb2xml/parslet_compositions.rb` | `parslet_compositions` | `parse` / `parse_compositions` wrappers around the pest parser. |
| `lib/oddb2xml/refdata_cleanup.rb` | `refdata_cleanup` | Compensates for known Refdata.Articles.xml data-quality issues (currently the doubled-dose template bug). Guarded by a comma-in-`substance_swissmedic` heuristic so real combination products (PHESGO, ATOVAQUON-PROGUANIL, etc.) stay untouched. Applied automatically in `Builder::new`. See [oddb2xml issue #112](https://github.com/zdavatz/oddb2xml/issues/112). |
| `lib/oddb2xml/calc.rb` | `calc` | Static `group_by_form` / `oid_for_form` / `oid_for_group` lookup tables covering 100+ Swissmedic forms across 12 galenic groups. Ordering matters: longer substrings first (e.g. `Filmtablette` before `Tablette`) — enforced by a unit test. |
| `lib/oddb2xml/chapter_70_hack.rb` | `chapter_70_hack` | HTML table scrape producing synthetic GTINs (`FAKE_GTIN_START + pharmacode`). |
| `lib/oddb2xml/semantic_check.rb` | `semantic_check` | `every_product_number_is_unique` + `every_item_number_is_unique` over generated XML. |
| `lib/oddb2xml/builder.rb` | `builder` | 7 XML output shapes (`product`, `article`, `substance`, `limitation`, `interaction`, `code`, `calc`) + `.dat`. Uses an internal `Node` enum so emitters can produce nested children (needed for `<ART>`'s `<ARTBAR>`/`<ARTPRI>`). Each top-level child carries a `SHA256` attribute over the hex digest of its joined descendant text. |
| `lib/oddb2xml/cli.rb` | `cli` + `src/bin/rust2xml.rs` | Parallel download+extract **and** parallel XML build via rayon (`Vec<(name, fn(&Builder) -> Result<String>)>` driven by `par_iter`). FHIR-first path is the default when `--fhir` or `--fhir-url` is set; legacy BAG XML otherwise. Union of BAG + Refdata pharma + Refdata non-pharma + ZurRose + Firstbase feeds all articles. `Cli::run_to_sqlite` is the same pipeline but writes a SQLite DB instead of seven XMLs (used by `rust2xml-gui`). |
| — (new) | `sqlite_export` | Walks `Builder::*_records()` (one method per output kind), unions column names per record, creates one TEXT-typed table per kind in SQLite. Nested children (`<ARTBAR>`, repeated `<ARTPRI>`) are JSON-encoded into a single column. Filename helper `timestamped_filename(flag, now) → rust2xml_e_HHMM_DD.MM.YYYY.sqlite`. |
| — (new) | `gui` + `src/bin/rust2xml-gui.rs` | egui desktop UI. `GuiApp` owns a `crossbeam-channel` for log + progress events. Both `-e` and `-b` buttons hard-wire `opts.fhir = true`. Worker thread runs `Cli::run_to_sqlite`, UI polls events on each frame via `request_repaint_after`. `util::set_log_sink` mirrors every `util::log()` line into the GUI log panel; `util::set_progress_sink` drives an `egui::ProgressBar`. Tabs are produced from `sqlite_master` enumeration; selected tab is loaded into a `Vec<Vec<String>>` cache and rendered with `egui_extras::TableBuilder`. Cell values collapse newlines + show full text on hover so long limitation descriptions stay readable in the 18-px row height. **Click-to-expand:** every cell is wrapped with `Sense::click()`; a click stores `(column_name, full_value)` into `selected_cell`, which renders a resizable bottom panel above the log with the untruncated value in a read-only multiline `TextEdit` (selectable + Copy button). Switching tabs clears the selection. Window icon embedded from `assets/icon.png` via `image::load_from_memory` → `egui::IconData`. |
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
  58 unit + 1 integration Rust tests cover the architectural pieces;
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

Each archive bundles `rust2xml`, `rust2xml-gui`, `compare_v5`,
`check_artikelstamm`, `README.md`, `LICENSE` and ships with a
`.sha256` sidecar.  The
workflow uploads everything to a GitHub Release with auto-generated
notes.  Bumping the patch version is the normal release cadence:
edit `Cargo.toml` version → commit → `git tag vX.Y.Z` → `git push
origin vX.Y.Z`.

The workflow also has a `workflow_dispatch` trigger so releases can
be re-run by hand from the Actions tab if an upload fails midway.

### Store distribution (Mac App Store + Microsoft Store)

Two extra workflow jobs sit alongside the matrix build:

- `macos-store` (gated on `vars.MACOS_STORE_ENABLED == 'true'`):
  builds a universal `rust2xml-gui.app`, signs it with the Developer
  ID Application identity for a notarized DMG, and (when the App
  Store secrets are present) signs again with the Apple Distribution
  identity, runs `productbuild` for a `.pkg`, then uploads to App
  Store Connect via `iTMSTransporter` / `altool`.  Bundle ID is
  `com.ywesee.rust2xml`; entitlements live in `entitlements.plist`
  (Developer ID, hardened runtime + JIT) and
  `entitlements-appstore.plist` (App Sandbox + JIT + network +
  user-selected file r/w).  The team-ID prefix in
  `application-identifier` is substituted at build time from
  `secrets.APPLE_TEAM_ID`.
- `windows-msix` (gated on `vars.MSSTORE_ENABLED == 'true'`): builds
  the GUI, packs `windows/AppxManifest.xml` + `windows/assets/*.png`
  (5 store logos generated from `assets/icon.png` via `sips`) into an
  MSIX with `makeappx`, signs it if `secrets.WINDOWS_CERTIFICATE` is
  present, then uploads + commits a Microsoft Store submission via
  the devcenter REST API when `vars.MSSTORE_APP_ID` and the three
  `MSSTORE_*` Azure secrets are set.

Both store jobs are off by default — flip the variables on per-repo
once the App ID is registered and the secrets are loaded:

```sh
gh variable set MACOS_STORE_ENABLED -R zdavatz/rust2xml -b true
gh variable set MSSTORE_ENABLED     -R zdavatz/rust2xml -b true
gh variable set MSSTORE_APP_ID      -R zdavatz/rust2xml -b "<store app id>"
```

Required secrets (re-set on `rust2xml` from the original sources —
GitHub secrets are write-only, so `gh secret list` on
`swissdamed2sqlite` only shows names):

```
APPLE_TEAM_ID, APPLE_API_KEY_P8, APPLE_API_KEY_ID, APPLE_API_ISSUER_ID,
MACOS_CERTIFICATE (+_PASSWORD),
MACOS_INSTALLER_CERTIFICATE (+_PASSWORD),
MACOS_DEVELOPER_ID_CERTIFICATE (+_PASSWORD),
MACOS_PROVISIONING_PROFILE,
WINDOWS_CERTIFICATE (+_PASSWORD)              # optional MSIX co-sign
MSSTORE_TENANT_ID, MSSTORE_CLIENT_ID, MSSTORE_CLIENT_SECRET
```

If the gate variables are unset the matrix build still produces the
existing five tarballs/zips and the GitHub Release is unchanged.

### Microsoft Store screenshots

`screenshots/windows/` carries the 1366 × 768 PNGs used in the
Microsoft Store submission plus the PowerShell tooling that produces
them:

- `orchestrate.ps1` — end-to-end: launches
  `target/release/rust2xml-gui.exe`, resizes to 1366 × 768, captures
  the empty state, mouse-clicks **Run -e (Extended)**, waits for the
  `~/rust2xml/sqlite/rust2xml_e_*.sqlite` file the GUI writes on
  completion, then captures populated tab views + a search-filtered
  view.  **Always closes the GUI it launched** — leaving the window
  open across sessions is intrusive.  Re-run with
  `pwsh -NoProfile -File screenshots/windows/orchestrate.ps1`.
- `capture.ps1` — single-shot helper.  Pass `-OutputName foo` to grab
  whichever rust2xml-gui window is currently visible; useful when
  manually composing a state the orchestrator can't reach.

Both scripts use Win32 P/Invoke (`SetWindowPos`, `GetWindowRect`,
`mouse_event`, `keybd_event`) because egui draws into a single
client-area surface — UI Automation can't see individual buttons /
tabs / text boxes, so we drive the window by screen coordinates
relative to the client origin.  Button / tab / search-box positions
in `orchestrate.ps1` assume the default 1366 × 768 layout; if the
top-bar widgets shift the offsets need to follow.

### App Store sandbox compatibility

Resolved in v3.1.6.  Every CLI/GUI write goes through
`util::home_data_root()` →
`dirs::home_dir().join("rust2xml")`.  When the binary is run
sandboxed under the Mac App Store entitlements, `home_dir()` returns
`~/Library/Containers/com.ywesee.rust2xml/Data/`, so the same code
that writes to `~/rust2xml/sqlite/...` on a developer machine writes
into the per-app container automatically — no `cfg(sandbox)` branch
needed and no save-panel detour.  The Developer ID DMG path is
unaffected (the sandbox flag isn't set, `home_dir()` still resolves
to `~`).

## Related Rust projects in this workspace

- `fb2sqlite` — GS1 barcode registry + MiGeL (related data source).
- `sdif` — Swiss drug interaction database.
- `swissdamed2sqlite` — Swiss medical device database.
- `pharma2merge` — pharmaceutical data merger.
