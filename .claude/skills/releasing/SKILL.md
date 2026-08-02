---
name: releasing
description: Release rust2xml — tag a version, the GitHub Actions matrix build, Mac App Store and Microsoft Store submission, store screenshots, and App Store sandbox notes. Use when cutting a release, fixing the release pipeline, or working on store distribution.
---

# Releasing rust2xml

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
edit `Cargo.toml` **and `src/version.rs`** → commit → `git tag vX.Y.Z`
→ `git push origin main vX.Y.Z`.

**Both version files, always.** `src/version.rs` holds a hand-written
`pub const VERSION` that is not derived from `Cargo.toml`, and nothing
in the build or CI cross-checks them. v3.1.31 shipped this way: the
manifest said 3.1.31, the binary said 3.1.30, and because Cargo saw no
change affecting codegen for that constant, `cargo build` finished in
under a second with no recompile and no warning. It is not cosmetic —
`artikelstamm.rs` writes `VERSION` into the generated XML as a
`Produced by rust2xml version …` comment, so a stale constant is
stamped into shipped data.

The workflow will not catch it for you: it derives the release version
from the **tag** (`version="${tag#v}"`), never from the binary, so a
mismatched pair produces a release named for the tag containing a
binary that reports something else. `cargo test --all --release` runs
in CI but never invokes the binary. Verify by hand after building:

```sh
cargo build --release --bin rust2xml
./target/release/rust2xml --version 2>&1   # must match the tag
```

Note the `2>&1`: `--version` and `--help` currently print to **stderr**
and exit **2**, because `options.rs` flattens clap's error with
`.map_err(|e| e.to_string())` and so cannot distinguish `DisplayHelp` /
`DisplayVersion` (clap's success signals, delivered as `Err`) from a
real usage error. Do not use either as a shell success check until that
is fixed.

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
MACOS_DEVELOPER_ID_CERTIFICATE,               # passphrase: reuses MACOS_CERTIFICATE_PASSWORD in release.yml
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

