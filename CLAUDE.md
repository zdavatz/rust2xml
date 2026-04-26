# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project status

Rust port of **oddb2xml** — the Ruby tool (~14,261 LOC across 20 modules) that generates Swiss Healthcare Public Domain data files (XML, SQLite, CSV, DAT). All 20 Ruby modules have a corresponding Rust module; the crate builds clean on stable Rust, 58 unit tests + 1 integration test pass.

Current released version: **v3.1.8** (App Store private-API compliance — patched `winit` 0.30.13). winit's `set_blur` calls the private CoreGraphics symbol `_CGSSetWindowBackgroundBlurRadius` (winit issue #4538), which Apple's binary scanner rejects. We point eframe → egui-winit → winit at a local fork at `winit-patched/` via `[patch.crates-io] winit = { path = "winit-patched" }` in `Cargo.toml`; the fork's `set_blur` is a no-op and the `objc2::ffi::NSInteger` / `objc2::runtime::AnyObject` imports that only existed for that extern are commented out so CI's `RUSTFLAGS=-Dwarnings` still compiles. Same fix as `swissdamed2sqlite` and `eudamed2firstbase`. App Store metadata (subtitle, description, keywords, marketing/support URL, promotional text, copyright, privacy policy URL) is now populated bilingually (de-DE + en-US) directly from `/Users/zdavatz/software/storetemplate/json/rust2xml.json` via the App Store Connect REST API (`appInfoLocalizations`, `appStoreVersionLocalizations`, `appStoreVersions`).

Previously:
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
| `lib/oddb2xml/fhir_support.rb` | `fhir_support` | Bundle-per-line NDJSON downloader + extractor that normalizes into the same `BagItem` shape the builder expects. Default URL: `https://epl.bag.admin.ch/static/fhir/foph-sl-export-latest-de.ndjson`. Walks `Bundle.entry[].resource` and extracts MedicinalProductDefinition / PackagedProductDefinition / Ingredient / RegulatedAuthorization. SL prices (`reimbursementSL.productPrice`) and limitation texts (`indication[].extension[regulatedAuthorization-limitation].limitationText`) live on the package-level RA; both are merged into `BagPrices` and `Vec<BagLimitation>` per package. `FhirExtractor::new_with_lang(ndjson, "fr"|"it")` routes the limitation text into `desc_fr`/`desc_it`; `merge_translations(primary, translation)` joins the per-language bundles by EAN-13 + per-package limitation index so `DSCRD`/`DSCRF`/`DSCIT` columns end up populated together. Cache filenames are derived from the URL so the three language files don't clobber each other. |
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
| — (new) | `gui` + `src/bin/rust2xml-gui.rs` | egui desktop UI. `GuiApp` owns a `crossbeam-channel` for log + progress events. Both `-e` and `-b` buttons hard-wire `opts.fhir = true`. Worker thread runs `Cli::run_to_sqlite`, UI polls events on each frame via `request_repaint_after`. `util::set_log_sink` mirrors every `util::log()` line into the GUI log panel; `util::set_progress_sink` drives an `egui::ProgressBar`. Tabs are produced from `sqlite_master` enumeration; selected tab is loaded into a `Vec<Vec<String>>` cache and rendered with `egui_extras::TableBuilder`. Cell values collapse newlines + show full text on hover so long limitation descriptions stay readable in the 18-px row height. Window icon embedded from `assets/icon.png` via `image::load_from_memory` → `egui::IconData`. |
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
