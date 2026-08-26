# rust2xml

Swiss Healthcare Public Domain data generator (XML, SQLite, CSV, …)
— pulls from public sources (Refdata, BAG/FOPH FHIR, Swissmedic,
ZurRose, EPha, Migel, Firstbase) and emits a bundle of XML files
plus an optional legacy `.dat`.

Functional successor to the [oddb2xml](https://github.com/zdavatz/oddb2xml)
Ruby gem, written in Rust. Current version: **v3.1.31**. Release
history is on the [Releases
page](https://github.com/zdavatz/rust2xml/releases) and in `git log`;
the notes below cover selected earlier versions.

v3.1.17 fixes empty
`DSCRD` / `DSCRF` / `DSCRI` on every `<Limitation>` in `oddb_limitation.xml`
under `--fhir`. The live BAG FHIR feed does not carry limitation text
inline; the `regulatedAuthorization-limitation` extension only holds a
`limitationIndication` reference to a `ClinicalUseDefinition` whose
`indication.diseaseSymptomProcedure.concept.text` is the actual text
(one per language). The parser now captures the reference, resolves DE
from the same bundle's CUDs, and merges FR/IT from the per-language
NDJSON bundles by the same CUD id. Coverage on the live FOPH feed
jumped from 0/5,963 (0%) to 5,963/5,963 (100%). Aligned with
oddb2xml 3.0.8 — see issue
[#116](https://github.com/zdavatz/oddb2xml/issues/116). v3.1.14 added a
new `--indc-xlsx <PATH>` CLI flag that writes a BAG Indikationscode
XLSX export to the given path.  One row per (XXXXX.NN code, GTIN)
pair with eight columns (`Indikationscode`, `Markenname`, `GTIN`,
`Pack-Beschreibung`, `ATC`, `Preis Ex-Factory`, `Preis Publikum`,
`Indikation`) ready for browsing/filtering in Excel.  Sorted by
code, brand, GTIN; header row frozen and bold; the indication
column wraps so multi-paragraph limitation texts stay readable.
Implies `--fhir`.  On the live FOPH feed (May 2026) the workbook
contains 1,419 data rows.  Built on top of the new `rust_xlsxwriter`
dependency.  A pre-generated sample lives at
[`xlsx/indc.xlsx`](xlsx/indc.xlsx) — open it in Excel / LibreOffice
to browse the codes without running the pipeline.  v3.1.13 shipped
release-pipeline fixes so end users
actually get tarballs on the GitHub Releases page (`reqwest` now
uses `rustls-tls` instead of `native-tls`, removing the
`openssl-sys` cross-compile failure that broke the
`aarch64-unknown-linux-gnu` build for v3.1.10/v3.1.11/v3.1.12; the
`publish` job is relaxed via `if: !cancelled()` so a single failed
cross-compile target no longer blocks the GitHub Release for the
targets that did build).  Previously v3.1.12 surfaced the BAG
**Indikationscode**
(`XXXXX.NN`) *and* its multi-paragraph limitation text in the GUI
and XML output. v3.1.11 added the
`INDIKATIONSCODE` column but the companion text was always empty
because the CUD parser looked for `indication.extension[limitationText]`
when the FOPH FHIR feed actually carries it under
`indication.diseaseSymptomProcedure.concept.text` (CodeableReference
shape). v3.1.12 fixes the path and adds an `INDIKATIONSCODE_TEXT`
column to the **products** / **articles** / **limitations** tabs
with newline-joined `XXXXX.NN: <text>` entries per code — click-to-
expand shows the full indication paragraph in the GUI's detail
panel. Mandatory on prescriptions and invoices for SL price-model
drugs from 2026-07-01 — BAG Rundschreiben 2026-02-19, oddb2xml issue
[#113](https://github.com/zdavatz/oddb2xml/issues/113). v3.1.10
shipped GUI/CLI download UX (parallel FHIR de/fr/it bundles inside
the BAG/FHIR job, chunked streaming with 10 MB progress logging for
files ≥ 5 MB, and a `util::progress_label()` API that updates the
GUI progress bar's caption live during big downloads).

## Parity with oddb2xml -e

Measured on 2026-04-24 against oddb2xml 3.0.4 using the same live data
sources. Record counts are the primary signal; sizes track roughly with
how much per-record content each file carries.

| File | rust2xml records | oddb2xml records | Delta | rust2xml size | oddb2xml size |
|---|---:|---:|---:|---:|---:|
| `oddb_interaction.xml` | 15,920 | 15,920 | **100.0%** | 12.8 MB | 14.6 MB |
| `oddb_code.xml` | 5 | 5 | **100.0%** | 0.5 KB | 1.5 KB |
| `oddb_article.xml` | 180,690 | 180,714 | **100.0%** | 108 MB | 140 MB |
| `oddb_substance.xml` | 1,389 | 1,405 | 98.9% | 0.2 MB | 0.2 MB |
| `oddb_limitation.xml` | 2,295 | 2,368 | 96.9% | 4.6 MB | 4.8 MB |
| `oddb_product.xml` | 18,162 | 17,173 | 105.8% | 13.2 MB | 15.7 MB |
| `oddb_calc.xml` | 18,162 | n/a | — | 12 MB | 41 MB |

Runtime: **~3 s** fresh download, **~17 s** including ZurRose's 177 K
transfer.dat parse. Well under a minute end-to-end. Downloads/extract
and the seven XML builds both run in parallel via `rayon` — the
output phase alone is ~30 % faster than the serial equivalent.

Schema shapes match Ruby where it matters:
- `<ART>` uses Ruby's nested `<ARTBAR>` (CDTYP / BC / BCSTAT) and one
  `<ARTPRI>` per price type (FACTORY / PUBLIC / ZURROSE / ZURROSEPUB).
- `<PRD>` carries GTIN, PRODNO, ATC, IT, CPT, PackGrSwissmedic,
  EinheitSwissmedic, SubstanceSwissmedic, CompositionSwissmedic.
- `<LIM>` carries SwissmedicNo5, IT, LIMTYP, LIMVAL, LIMNAMEBAG,
  LIMNIV, DSCRD, DSCRF, VDAT.
- `<CAL>` carries GTIN, PHAR, NAMD, NAMF, ATC, IT, PACKSIZE, UNIT,
  FORM, GROUP, OID, SUBSTANCE, COMPOSITION.
- Every top-level child has a `SHA256` attribute over the
  concatenated descendant text — same contract Ruby consumers rely on
  via `Oddb2xml.verify_sha256`.

## Build

```sh
cargo build --release
```

Four binaries land in `target/release/`:

- `rust2xml` — main CLI.
- `rust2xml-gui` — desktop UI (Linux / macOS / Windows) with `-e` /
  `-b` buttons and a SQLite-backed table viewer (see *Desktop UI* below).
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

# Elexis Artikelstamm v6 (Elexis ≥ 3.1) → artikelstamm_v6.xml + .csv
./target/release/rust2xml --artikelstamm

# Additionally emit the legacy v5 (no <ARTSL>) → artikelstamm_v5.xml + .csv
./target/release/rust2xml --artikelstamm-v5

# Cache downloads — re-uses files already under ./downloads/
./target/release/rust2xml -e --skip-download --log
```

## Desktop UI (`rust2xml-gui`)

Cross-platform egui app. Two big buttons drive the same pipeline as
the CLI but write the result into a single SQLite database instead of
seven XML files:

```sh
./target/release/rust2xml-gui
```

- **Always FHIR.** The GUI hard-wires `--fhir` for both buttons and
  pulls from the FOPH ePL feed
  (`https://epl.bag.admin.ch/static/sl/publication/fhir/foph-sl-publication-latest-de.ndjson`).
  Ex-factory + retail prices and limitation texts come straight out of
  the package-level `RegulatedAuthorization` resources.
- `Run -e (Extended)` and `Run -b (Firstbase)` start the
  download/extract pipeline in a worker thread (UI stays responsive,
  the FHIR download/parse log streams live in the bottom panel).
- A progress bar reports per-job completion (BAG/FHIR, Refdata,
  Swissmedic, EPha, LPPV, ZurRose, Firstbase) plus the builder + SQLite
  write phases. The bar's caption updates live during the active
  download (`foph-sl-publication-latest-de.ndjson: 30 MB / 89 MB (34%)`)
  so a long single-file fetch — typically the 90+ MB FHIR NDJSON or
  the 150 MB Firstbase CSV — never looks like a hang.
- The three FHIR language bundles (`-de.ndjson` / `-fr.ndjson` /
  `-it.ndjson`) download + extract in parallel via `rayon` inside the
  BAG/FHIR job, instead of sequentially DE → FR → IT. DE failure is
  fatal; FR/IT failures are logged and the run still completes.
- Output lands at
  `~/rust2xml/sqlite/rust2xml_<flag>_HHMM_DD.MM.YYYY.sqlite`
  (e.g. `~/rust2xml/sqlite/rust2xml_e_1430_25.04.2026.sqlite`).
  CLI XML output goes to `~/rust2xml/xml/`, raw upstream caches to
  `~/rust2xml/downloads/`.  The path resolves via
  `dirs::home_dir()` so a sandboxed Mac App Store build writes into
  its per-app container automatically.
- An **📂 Open Data Folder** button next to the run buttons reveals
  `~/rust2xml/` in Finder / Explorer / `xdg-open` so you always know
  where the SQLite snapshots and XML output live.
- After the run, eight tabs (`articles`, `calc`, `codes`,
  `interactions`, `limitations`, `meta`, `products`, `substances`)
  let you browse the data — every column is shown, columns are
  resizable, the table scrolls horizontally for wide records, and
  long cell values truncate with hover-text for the full content.
- **Click any cell** to open a detail panel above the log showing the
  full untruncated value (with newlines preserved) plus the column
  name, character count, and a Copy button.  Useful for reading
  multi-paragraph German limitation descriptions or copying the
  flattened price / barcode columns out of the table.
- A search box above the table does case-insensitive substring
  matching across **every column** of the selected tab.  Each row's
  values are joined into a single lowercased haystack at load time so
  filtering 180 K-row tables stays responsive on every keystroke;
  switching tabs resets the query, and the row counter reads
  `X of Y rows match × N cols` while filtering.
- Article + product `DSCRD` / `DSCRF` resolve through a refdata-first
  fallback chain (refdata.desc_de → Swissmedic xlsx `sequence_name`
  → BAG `desc_*` → BAG `name_*`) so brand-name searches like
  `PONSTAN` / `INDERAL` find rows even in FHIR mode where BAG only
  carries Marketing-Authorisation names.
- Nested fields are flattened into real columns:
  `ARTBAR_E13_BC` / `ARTBAR_E13_BCSTAT` for barcodes,
  `ARTPRI_FACTORY` / `ARTPRI_PUBLIC` / `ARTPRI_ZURROSE` /
  `ARTPRI_ZURROSEPUB` for the four price tiers — no JSON in cells.
- Limitations carry trilingual descriptions: `DSCRD` (German),
  `DSCRF` (French) and `DSCIT` (Italian).  The GUI fetches all three
  FOPH FHIR exports (`-de.ndjson`, `-fr.ndjson`, `-it.ndjson`) and
  merges the per-package limitation list by index.
- Window icon is embedded into the binary so the app shows up
  branded in the taskbar / Dock on Linux, macOS and Windows.  On
  Windows the .ico is also linked into the .exe via `winresource`,
  so Explorer / Start menu show the icon on disk too.

The SQLite file is plain — open it with `sqlite3`, DBeaver, etc.
Each run creates a fresh timestamped file; old runs stay on disk.

## Generated files (XML mode)

- `oddb_product.xml`
- `oddb_article.xml`
- `oddb_substance.xml`
- `oddb_limitation.xml`
- `oddb_interaction.xml`
- `oddb_code.xml`
- `oddb_calc.xml` (when `-e` / `--calc` / `--firstbase` / `--artikelstamm`)
- `artikelstamm_v6.xml` + `artikelstamm_v6.csv` (when `--artikelstamm`) —
  Elexis Artikelstamm v6 (`<PRODUCTS>`/`<LIMITATIONS>`/`<ITEMS>`, with the
  per-item `<ARTSL>` BAG Indikationscode block); validates against
  `Elexis_Artikelstamm_v6.xsd`
- `artikelstamm_v5.xml` + `artikelstamm_v5.csv` (when `--artikelstamm-v5`) —
  legacy v5 shape (no `<ARTSL>`); validates against `Elexis_Artikelstamm_v5.xsd`

By default the Artikelstamm carries only German (`<DSCR>`) and French
(`<DSCRF>`) descriptions — the strict upstream Elexis v6/v5 XSD (the one
the Elexis importer validates against) has no `<DSCRI>` element, so
emitting Italian would make the import fail with
`cvc-complex-type.2.4.a`. Pass `-it` / `--italian` to additionally emit
the Italian `<DSCRI>` leaves (for consumers using the oddb2xml-extended
XSD).

Every GTIN registered in the Swissmedic register (Packungen.xlsx) is
emitted as a full pharma `<ITEM>` even when it is missing from the BAG
SL feed: the register supplies `PKG_SIZE`, `MEASURE`, `DOSAGE_FORM` /
`DOSAGE_FORMF`, `IKSCAT`, `PRODNO` and the company `NAME`, while Refdata
contributes the descriptions and company `GLN` and ZurRose the `PHAR`
and prices (mirroring oddb2xml's `@packs[no8].merge` pharma branch —
e.g. vaccines like TWINRIX that were dropped from the SL). Vaccine packs
(`ATC ^J07`, except `J07AX`) without an own PRODNO borrow the PRODNO of
a register pack with the same ATC, so the Elexis vaccination list keeps
resolving them.

### Rogger Mediliste (`-r` / `--rogger`)

The "Rogger Mediliste" is the name-conflict list maintained by Frau
Rogger (Vitabyte / Zur Rose): a curated `GTIN,Mediname` mapping of
preferred German article names. With `-r` / `--rogger`, every listed
GTIN gets its German Refdata description replaced by the list's name —
in `oddb_article.xml` (`<DSCRD>` / `<SORTD>`), `oddb_product.xml` and
the Artikelstamm alike. The list is German-only: `<DSCRF>` / `<DSCRI>`
are never touched.

The source of truth is the shared Google Sheet, fetched as its CSV
export at run time, so edits there reach the feeds without a release.
A bundled `data/rogger_liste.csv` is embedded in the binary and used
whenever the download is unavailable (offline, allow-list proxy) or
does not look like the expected CSV — e.g. the sign-in page Google
serves once the sheet stops being shared as "anyone with the link can
view". The override is applied *after* the [issue #112][i112] Refdata
cleanups, so the curated name always wins.

[i112]: https://github.com/zdavatz/oddb2xml/issues/112

The `artikelstamm_*` files carry **no** `SHA256` attribute (matching
oddb2xml). Every top-level element in the `oddb_*.xml` files carries a
`SHA256` attribute whose
value is the hex digest of the element's text content, so consumers can
detect unchanged nodes between runs (same contract as the Ruby gem).

## Option parity with the Ruby gem

Every flag from `lib/oddb2xml/options.rb` has a 1:1 Rust equivalent
except `--proxy-check`, including optimist's auto-assigned short flags:

| Flag | Short | Purpose |
|---|---|---|
| `--append` | `-a` | Additional target nonpharma |
| `--artikelstamm` | | Create Elexis Artikelstamm v6 (`artikelstamm_v6.xml` + `.csv`) |
| `--artikelstamm-v5` | | Additionally emit the legacy v5 (no `<ARTSL>`); implies `--artikelstamm` |
| `--italian` (`--it`) | | Include the Italian `<DSCRI>` in the Artikelstamm (off by default; strict Elexis XSD has no `<DSCRI>`) |
| `--compress-ext <FMT>` | `-c` | `tar.gz` or `zip` |
| `--extended` | `-e` | Pharma + non-pharma + ZurRose + `oddb_calc.xml` |
| `--fhir` | | Use FOPH/BAG FHIR NDJSON feed (default ON for `-e`/`-b` since 01.06.2026) |
| `--no-fhir` | | Use the legacy SL XML instead (opt out of the `-e`/`-b` FHIR default) |
| `--fhir-url <URL>` | | Custom FHIR NDJSON URL (implies `--fhir`) |
| `--format <FMT>` | `-f` | `xml` (default) or `dat` |
| `--include` | `-i` | EAN14 for `dat` format |
| `--increment <PCT>` | `-I` | Price increment %; forces `-f dat -p zurrose` |
| `--fi` | `-o` | Optional Fachinfo output |
| `--price [<SRC>]` | `-p` | Price source (default `zurrose`) |
| `--tag-suffix <S>` | `-t` | XML tag suffix + filename prefix |
| `--context <CTX>` | `-x` | `product` (default) or `address` |
| `--calc` | | Only `oddb_calc.xml` |
| `--skip-download` | | Reuse files already in `~/rust2xml/downloads/` (absent ones are still fetched). Without it every source is downloaded fresh. |
| `--log` | | Log important actions |
| `--use-ra11zip <PATH>` | | Use a zipped `transfer.dat` from Galexis |
| `--firstbase` | `-b` | NONPHARMA via GS1 Switzerland CSV |
| `--rogger` | `-r` | Prefer the German article names from the "Rogger Mediliste" |

Implied-flag cascade (same behaviour as Ruby):
- `--increment N` → sets `nonpharma`, `price=zurrose`, `ean14=true`, `percent=N`
- `--firstbase` → sets `nonpharma`, `calc`, and (since 01.06.2026) `fhir` unless `--no-fhir`
- `--extended` → sets `nonpharma`, `price=zurrose`, `calc`, and (since 01.06.2026) `fhir` unless `--no-fhir`
- `--artikelstamm` → sets `extended`, `price=zurrose`
- `--artikelstamm-v5` → sets `artikelstamm` (and thus `extended`, `price=zurrose`)
- `--fhir-url` → sets `fhir`
- `-f xml` → forces `ean14=true`
- `-x address` / `-x addr` → `address=true`

## Test

```sh
cargo test              # unit + integration
```

58 unit tests + 1 integration test:

- 23 option-parity tests (one per Ruby flag + every implied-flag
  cascade rule).
- `util` tests for GTIN checksum, HTML decode, EAN ↔ ProdNo ↔ No8
  bidirectional maps, CRLF handling.
- `calc` tests including the ordering invariant
  (Filmtablette substring matches before Tablette) and the
  "every-form-has-an-OID" structural check.
- Composition-grammar parse tests (single substance, comma-separated
  list, multi-line).
- Extractor tests for LPPV text files and EPha CSV.
- Builder tests confirming SHA256 attribute emission.
- Integration test that roundtrips a BAG XML fixture through extractor
  → builder and asserts the SHA256 / content plumbing.

## Refdata data-quality compensation

Refdata.Articles.xml ships with recurring data-quality issues that
otherwise propagate into downstream output unchanged. rust2xml mirrors
the cleanups added in oddb2xml 3.0.5 (see
[issue #112](https://github.com/zdavatz/oddb2xml/issues/112)).

Currently active (`src/refdata_cleanup.rs`):

* **Doubled dose token** — when Refdata emits the strength twice in
  `<FullName>` (e.g. `MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette`)
  and the matching Swissmedic entry shows a single active substance,
  the duplicate token is collapsed to a single occurrence. Real
  combination products like `PHESGO 600 mg / 600 mg / 10 ml`
  (pertuzumab + trastuzumab) are detected via the comma in
  `substance_swissmedic` and left untouched.

The cleanup is wired into `Builder::new` and is idempotent — every
rule is guarded by a Swissmedic-side heuristic so genuine data is
never altered.

## Architecture

See `CLAUDE.md` for the full 1:1 Ruby → Rust module mapping, the
replacement crates for each Ruby gem, and the documented porting debt.

## Releases

Pre-built binaries for **Linux (x86_64 + aarch64)**, **macOS (Intel +
Apple Silicon)** and **Windows (x86_64)** are attached to every GitHub
Release. Each archive contains `rust2xml`, `compare_v5`,
`check_artikelstamm`, README and LICENSE, plus a `.sha256` file.

**macOS archives** ship a proper `rust2xml-gui.app` bundle (with
embedded `.icns`) — drag it into `/Applications` and launch from
Finder/Spotlight.

**Linux archives** ship `rust2xml-gui.desktop`, `icon.png` and an
`install-linux.sh` helper.  Run `./install-linux.sh` after unpacking
to drop the binaries into `~/.local/bin` and register the launcher
with your desktop environment.

**Windows archives** carry the icon embedded directly in
`rust2xml-gui.exe` so Explorer / Start menu show it on disk.

### Cutting a release

Bump `version` in `Cargo.toml` **and** the `VERSION` constant in
`src/version.rs` (keep them in sync), commit, then push a `vX.Y.Z`
tag:

```sh
# bump patch version in Cargo.toml + src/version.rs, commit, then:
git tag v3.1.13
git push origin v3.1.13
```

The current released version is **v3.1.13** — release-pipeline fixes:
`reqwest` switched to `rustls-tls` (no more `openssl-sys` cross-compile
breakage) and the `publish` job now runs whenever the workflow wasn't
cancelled, so a single failed target no longer blocks the GitHub
Release.  Release archives ship a macOS `rust2xml-gui.app` bundle
(with `.icns` icon generated via `sips` + `iconutil`) and a Linux
`.desktop` launcher + icon + installer script.  Bump the patch
(`v3.1.14`), minor (`v3.2.0`) or major (`v4.0.0`) segment depending
on the nature of the change.

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

### Mac App Store + Microsoft Store

Two opt-in jobs run alongside the matrix build for store
distribution:

- **Mac App Store + notarized DMG** (`macos-store`, gated on
  `vars.MACOS_STORE_ENABLED == 'true'`).  Builds a universal
  `rust2xml-gui.app`, signs it with the Developer ID Application
  identity for a notarized DMG (uploaded as a release artefact), and
  — when the App Store secrets are present — signs again with the
  Apple Distribution identity, runs `productbuild` for a `.pkg`, and
  uploads to App Store Connect via `iTMSTransporter` / `altool`.
  Bundle ID `com.ywesee.rust2xml`; entitlements files
  (`entitlements.plist` and `entitlements-appstore.plist`) live at
  the repo root.
- **Microsoft Store** (`windows-msix`, gated on
  `vars.MSSTORE_ENABLED == 'true'`).  Packs the GUI binary +
  `windows/AppxManifest.xml` + 5 store logos under `windows/assets/`
  into an MSIX with `makeappx`, optionally signs it with
  `secrets.WINDOWS_CERTIFICATE`, then uploads + commits a Microsoft
  Store submission via the devcenter REST API when
  `vars.MSSTORE_APP_ID` and the three `MSSTORE_*` Azure secrets are
  set.

Both jobs are off by default — `gh variable set MACOS_STORE_ENABLED
-b true` (and `MSSTORE_ENABLED`, `MSSTORE_APP_ID`) flips them on
once the App ID is registered and the corresponding secrets are
loaded via `gh secret set`.  See the **Store distribution** section
in `CLAUDE.md` for the full secret list.

#### Microsoft Store screenshots

`screenshots/windows/` holds the seven 1366 × 768 PNGs uploaded with
the Microsoft Store submission (empty state, run-in-progress with
progress bar + log, populated tab views, search filter).  They are
generated end-to-end by `screenshots/windows/orchestrate.ps1`, which:

1. launches `target/release/rust2xml-gui.exe`,
2. resizes the window to 1366 × 768 (Microsoft Store's recommended
   minimum),
3. captures the empty state, then mouse-clicks **Run -e (Extended)**,
4. waits for `~/rust2xml/sqlite/rust2xml_e_*.sqlite` to appear,
5. captures populated tab views + a search-filtered view,
6. closes the GUI it launched.

To regenerate them after a UI change:

```pwsh
pwsh -NoProfile -File screenshots/windows/orchestrate.ps1
```

## License

GPL-3.0-only, inherited from oddb2xml.
