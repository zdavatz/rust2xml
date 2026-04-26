# rust2xml

Swiss Healthcare Public Domain data generator (XML, SQLite, CSV, …)
— pulls from public sources (Refdata, BAG/FOPH FHIR, Swissmedic,
ZurRose, EPha, Migel, Firstbase) and emits a bundle of XML files
plus an optional legacy `.dat`.

Functional successor to the [oddb2xml](https://github.com/zdavatz/oddb2xml)
Ruby gem, written in Rust. Current version: **v3.1.7**.

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

# Artikelstamm v3/v5 (Elexis ≥ 3.1)
./target/release/rust2xml --artikelstamm

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
  (`https://epl.bag.admin.ch/static/fhir/foph-sl-export-latest-de.ndjson`).
  Ex-factory + retail prices and limitation texts come straight out of
  the package-level `RegulatedAuthorization` resources.
- `Run -e (Extended)` and `Run -b (Firstbase)` start the
  download/extract pipeline in a worker thread (UI stays responsive,
  the FHIR download/parse log streams live in the bottom panel).
- A progress bar reports per-job completion (BAG/FHIR, Refdata,
  Swissmedic, EPha, LPPV, ZurRose, Firstbase) plus the builder + SQLite
  write phases.
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
git tag v3.1.5
git push origin v3.1.5
```

The current released version is **v3.1.5** — adds an in-tab
search box and a refdata-first fallback chain for article + product
descriptions.  Release archives ship a macOS `rust2xml-gui.app`
bundle (with `.icns` icon generated via `sips` + `iconutil`) and a
Linux `.desktop` launcher + icon + installer script.  Bump the patch
(`v3.1.6`), minor (`v3.2.0`) or major (`v4.0.0`) segment depending
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

## License

GPL-3.0-only, inherited from oddb2xml.
