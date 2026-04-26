#!/bin/bash
# Drive rust2xml-gui through its main flows and capture App Store screenshots.
#
# - Launches the release binary.
# - Resizes the window to 1440×900 logical (= 2880×1800 pixels on a
#   Retina display, matching one of Apple's App Store sizes).
# - Captures the empty initial state.
# - Clicks "Run -e (Extended)", waits for SQLite to be written.
# - Captures the populated tab view + a few tab switches.
# - Types into the search box, captures the filtered view.
#
# Output: 2880×1800 PNGs in this directory (1440×900 on a non-Retina
# display).  Run from anywhere; the script self-locates.
#
# Requires: cliclick (`brew install cliclick`).  cliclick + screencapture
# need Accessibility + Screen-Recording permission for Terminal — grant
# them interactively on first run if macOS prompts.

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
GUI_BIN="${GUI_BIN:-$REPO_ROOT/target/release/rust2xml-gui}"
OUT_DIR="$SCRIPT_DIR"

WX=40
WY=40
WW=1280
WH=800
RUN_TIMEOUT=600

if [[ ! -x "$GUI_BIN" ]]; then
  echo "ERROR: GUI binary not found at $GUI_BIN" >&2
  echo "       Run: cargo build --release --bin rust2xml-gui" >&2
  exit 1
fi
if ! command -v cliclick >/dev/null 2>&1; then
  echo "ERROR: cliclick not found.  Install with: brew install cliclick" >&2
  exit 1
fi

cap() {
  local name="$1"
  screencapture -x -R "${WX},${WY},${WW},${WH}" "$OUT_DIR/${name}.png"
  local dims
  dims=$(sips -g pixelWidth -g pixelHeight "$OUT_DIR/${name}.png" 2>/dev/null \
           | awk '/pixelWidth/ {w=$2} /pixelHeight/ {h=$2} END {print w "x" h}')
  echo "  saved ${name}.png  (${dims})"
}

# Snapshot existing SQLite files so we can detect a fresh one later.
DB_DIR="$HOME/rust2xml/sqlite"
mkdir -p "$DB_DIR"
INITIAL_DBS=$(ls "$DB_DIR"/rust2xml_e_*.sqlite 2>/dev/null | sort -u || true)

echo "killing any existing rust2xml-gui..."
pkill -f rust2xml-gui 2>/dev/null || true
sleep 1

echo "launching $GUI_BIN"
"$GUI_BIN" >/dev/null 2>&1 &
GUI_PID=$!

cleanup() {
  kill "$GUI_PID" 2>/dev/null || true
  pkill -f rust2xml-gui 2>/dev/null || true
}
trap cleanup EXIT

echo "waiting for window..."
for i in $(seq 1 60); do
  if osascript -e 'tell application "System Events" to exists window 1 of (first process whose name is "rust2xml-gui")' 2>/dev/null | grep -q true; then
    break
  fi
  sleep 0.5
done

echo "resizing window to ${WW}x${WH} at (${WX}, ${WY})"
osascript <<EOF
tell application "System Events"
  tell (first process whose name is "rust2xml-gui")
    set frontmost to true
    set position of window 1 to {${WX}, ${WY}}
    set size of window 1 to {${WW}, ${WH}}
  end tell
end tell
EOF
sleep 1

# Geometry of clickable elements (window-relative logical points).
# Values mirror the Windows orchestrator: top panel padding 6px, button
# row 36px, second row of controls 28px below, tab strip below that.
TITLE_BAR=28
RUN_E_X=$((WX + 130))                          # center of "Run -e" button
RUN_E_Y=$((WY + TITLE_BAR + 6 + 18))           # padding + half of button height
TABS_Y=$((WY + TITLE_BAR + 130))               # ~30 px below the controls panel
SEARCH_Y=$((WY + TITLE_BAR + 200))             # search row below tab strip + separator

echo
echo "01 — empty initial state"
cap "01-empty"

echo "clicking Run -e (Extended) at ($RUN_E_X, $RUN_E_Y)"
cliclick "c:${RUN_E_X},${RUN_E_Y}"
sleep 3

echo "02 — running (download/parse in progress)"
cap "02-running"

echo "waiting for fresh SQLite to land in $DB_DIR (≤ ${RUN_TIMEOUT}s)"
DEADLINE=$(($(date +%s) + RUN_TIMEOUT))
SQLITE=""
while [ "$(date +%s)" -lt "$DEADLINE" ]; do
  current=$(ls "$DB_DIR"/rust2xml_e_*.sqlite 2>/dev/null | sort -u || true)
  fresh=$(comm -13 <(echo "$INITIAL_DBS") <(echo "$current") | grep -v '^$' || true)
  if [ -n "$fresh" ]; then
    candidate=$(echo "$fresh" | head -1)
    # Wait until file size stops growing (worker has flushed final pages).
    prev=0
    while :; do
      now=$(stat -f %z "$candidate" 2>/dev/null || echo 0)
      if [ "$now" = "$prev" ] && [ "$now" -gt 0 ]; then break; fi
      prev=$now
      sleep 1
    done
    SQLITE="$candidate"
    break
  fi
  sleep 2
done
[ -n "$SQLITE" ] || { echo "ERROR: run did not finish within ${RUN_TIMEOUT}s" >&2; exit 1; }
echo "DB ready: $SQLITE"
sleep 4

echo "03 — articles tab loaded"
cap "03-tabs-loaded"

echo "switching tabs"
cliclick "c:$((WX + 130)),$TABS_Y"  # second tab (calc)
sleep 2
cap "04-tab-calc"

cliclick "c:$((WX + 270)),$TABS_Y"  # interactions
sleep 2
cap "05-tab-interactions"

cliclick "c:$((WX + 360)),$TABS_Y"  # limitations
sleep 2
cap "06-tab-limitations"

cliclick "c:$((WX + 500)),$TABS_Y"  # products
sleep 2
cap "07-tab-products"

echo "search box demo"
cliclick "c:$((WX + 400)),$SEARCH_Y"
sleep 0.3
cliclick "t:PONSTAN"
sleep 2
cap "08-search-ponstan"

echo
echo "Done.  Screenshots in $OUT_DIR"
ls -la "$OUT_DIR"/*.png 2>/dev/null
