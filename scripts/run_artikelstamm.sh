#!/usr/bin/env bash
#
# run_artikelstamm.sh — build the Elexis Artikelstamm (v6 + legacy v5) with
# rust2xml from the live sources and publish it to the mediupdatexml.oddb.org
# download area, served at https://mediupdatexml.oddb.org/artikelstamm/rust2xml/
#
# Meant to run from /etc/crontab as user zdavatz. rust2xml downloads fresh
# every run (no --skip-download), so a daily run always reflects the current
# FOPH/BAG feed. Output filenames are undated (artikelstamm_v6.xml / _v5.xml
# + companion .csv), so each run overwrites in place.
#
# Configurable via environment:
#   REPO_DIR   rust2xml checkout      (default /home/zdavatz/software/rust2xml)
#   DEST_DIR   publish target dir     (default /home/zdavatz/oddb2xml/artikelstamm/rust2xml)
#   CARGO_ENV  cargo env script       (default $HOME/.cargo/env)
#
set -euo pipefail

REPO_DIR="${REPO_DIR:-/home/zdavatz/software/rust2xml}"
DEST_DIR="${DEST_DIR:-/home/zdavatz/oddb2xml/artikelstamm/rust2xml}"
CARGO_ENV="${CARGO_ENV:-$HOME/.cargo/env}"
SRC_DIR="$HOME/rust2xml/xml"

log() { printf '%s %s\n' "$(date '+%Y-%m-%d %H:%M:%S')" "$*"; }

# Make cargo/rustc available under cron's minimal PATH.
[[ -f "$CARGO_ENV" ]] && source "$CARGO_ENV"

cd "$REPO_DIR"

log "Building rust2xml (release)"
cargo build --release --bin rust2xml

# Reset the cache so everything but the ZurRose seed below is fetched fresh.
# This is the same wipe-then-seed-then---skip-download pattern the Ruby nightly
# build uses (run_oddb2xml.sh seed_downloads): --skip-download reuses only what
# is already in downloads/ and still fetches everything else.
# The wipe is what makes the run fresh. rust2xml's cache lives in
# ~/rust2xml/downloads/ and, unlike Ruby's ./downloads, is never cleared for
# us; before v3.1.x skip_download_cached also ignored the flag, so a nightly
# run silently rebuilt from the previous day's sources (observed: every file
# in ~/rust2xml/downloads still from the day before).
# Same file list as documented in CLAUDE.md (v3.1.10).
log "Clearing download cache"
rm -rf "$HOME/rust2xml/downloads"
rm -f "$HOME"/rust2xml/foph-*.ndjson "$HOME/rust2xml/transfer.zip" \
      "$HOME/rust2xml/Refdata.Articles.zip"

# ZurRose: seed transfer.zip from the local get_transfer mirror.
# get_transfer.sh (crontab 00:30) downloads transfer.dat straight from
# zurrose.ch on THIS host and uploads the zip to pillbox.oddb.org — fetching
# it back from pillbox is a needless detour and a single point of failure
# (2026-07-02: pillbox refused connections at 01:00 and killed the Ruby
# nightly build). With the seed in downloads/, the --skip-download run below
# reuses it; if the file is missing, the normal pillbox download runs.
GET_TRANSFER_ZIP="${GET_TRANSFER_ZIP:-/home/zdavatz/software/get_transfer/TRANSFER.ZIP}"
if [[ -s "$GET_TRANSFER_ZIP" ]]; then
  mkdir -p "$HOME/rust2xml/downloads"
  cp -p "$GET_TRANSFER_ZIP" "$HOME/rust2xml/downloads/transfer.zip"
  log "Seeded ZurRose transfer.zip from $GET_TRANSFER_ZIP ($(date -r "$GET_TRANSFER_ZIP" '+%Y-%m-%d %H:%M'))"
else
  log "WARNING: $GET_TRANSFER_ZIP missing - falling back to pillbox.oddb.org download"
fi

# --skip-download applies to the seeded transfer.zip only: the cache was just
# wiped, so every other source is absent and gets downloaded fresh.
log "Generating Artikelstamm v6 + v5 (fresh download, seeded ZurRose)"
./target/release/rust2xml --artikelstamm-v5 --skip-download --log

# Verify the four expected outputs exist.
for f in artikelstamm_v6.xml artikelstamm_v6.csv artikelstamm_v5.xml artikelstamm_v5.csv; do
  [[ -s "$SRC_DIR/$f" ]] || { log "ERROR: missing output $SRC_DIR/$f"; exit 1; }
done

log "Publishing to $DEST_DIR"
mkdir -p "$DEST_DIR"
cp -p "$SRC_DIR"/artikelstamm_v6.xml "$SRC_DIR"/artikelstamm_v6.csv \
      "$SRC_DIR"/artikelstamm_v5.xml "$SRC_DIR"/artikelstamm_v5.csv "$DEST_DIR"/
chmod 644 "$DEST_DIR"/artikelstamm_v*.*

# Refresh the README with the current record counts.
f6="$DEST_DIR/artikelstamm_v6.xml"
PROD=$(grep -c '<PRODUCT>' "$f6" || true)
LIM=$(grep -c '<LIMITATION>' "$f6" || true)
ITEM=$(grep -c '<ITEM ' "$f6" || true)
ARTSL=$(grep -c '<ARTSL>' "$f6" || true)
VERSION=$(sed -n 's/.*pub const VERSION: &str = "\([^"]*\)".*/\1/p' "$REPO_DIR/src/version.rs")
cat > "$DEST_DIR/README.txt" <<EOF
Elexis Artikelstamm - generiert mit rust2xml
============================================

Diese Dateien wurden mit rust2xml (Rust-Port von oddb2xml) erzeugt,
Version ${VERSION}, aus denselben Live-Quellen wie oddb2xml
(BAG/FOPH SL FHIR-Feed, Swissmedic, Refdata, ZurRose, LPPV, EPha).
Taeglich neu generiert um 03:00 Uhr.

Dateien:
  artikelstamm_v6.xml   Elexis Artikelstamm v6 (mit <ARTSL> BAG-Indikationscodes)
  artikelstamm_v6.csv   Begleit-CSV zu v6
  artikelstamm_v5.xml   Elexis Artikelstamm v5 (legacy, ohne <ARTSL>)
  artikelstamm_v5.csv   Begleit-CSV zu v5

  Validiert gegen Elexis_Artikelstamm_v6.xsd bzw. _v5.xsd.
  CSV-Spalten: gtin, name, pkg_size, galenic_form, price_ex_factory,
               price_public, prodno, atc_code, active_substance,
               original, it-code, sl-liste

Inhalt (identisch in v6 und v5):
  PRODUCT     $PROD
  LIMITATION  $LIM
  ITEM        $ITEM
  ARTSL       $ARTSL   (nur v6)

Hinweis: generiert mit rust2xml (https://github.com/zdavatz/rust2xml).
Das DATA_SOURCE-Attribut bleibt laut XSD-Enum auf "oddb2xml"; die
Herkunft rust2xml steht im Kopf-Kommentar der XML-Dateien.
EOF
chmod 644 "$DEST_DIR/README.txt"

log "Done. Published $PROD products, $LIM limitations, $ITEM items to $DEST_DIR"
