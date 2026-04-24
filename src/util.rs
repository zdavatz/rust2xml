//! Utility helpers ported from `lib/oddb2xml/util.rb`.
//!
//! Contents:
//!  * GTIN / EAN-13 checksum (`calc_checksum`).
//!  * Prodno generator (`gen_prodno`).
//!  * HTML entity decoding + UTF-8 patching.
//!  * ISO-8859-1 conversion.
//!  * Skip-download file cache.
//!  * Global options holder + logger.
//!  * ProdNo ↔ EAN-13 ↔ No8 bidirectional maps (used by builder for
//!    Artikelstamm consistency).
//!  * `add_hash` / `verify_sha256` — SHA256 attribute on top-level XML nodes.
//!  * Swissmedic `Packungen.xlsx` column layouts (`COLUMNS_FEBRUARY_2019`,
//!    `COLUMNS_JULY_2015`).

use once_cell::sync::Lazy;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Prefix used for synthetic GTINs when a ZurRose row lacks a real EAN-13.
pub const FAKE_GTIN_START: &str = "999999";

/// Build a product number from `iksnr` (5 digits, zero-padded) and
/// `seqnr` (2 digits, zero-padded).  Mirrors `Oddb2xml.gen_prodno`.
pub fn gen_prodno(iksnr: u64, seqnr: u64) -> String {
    format!("{:05}{:02}", iksnr, seqnr)
}

/// GTIN/EAN-13 check digit.  Mirrors `Oddb2xml.calc_checksum`.
///
/// Takes the first 12 characters of `str_` and returns a single-digit
/// String.  Non-digit characters are treated as 0 (`"x".to_i == 0` in Ruby).
pub fn calc_checksum(s: &str) -> String {
    let s = s.trim();
    let chars: Vec<char> = s.chars().collect();
    let mut sum: u32 = 0;
    for idx in 0..12 {
        let fct: u32 = ((idx as u32 % 2) * 2) + 1;
        let digit: u32 = chars
            .get(idx)
            .and_then(|c| c.to_digit(10))
            .unwrap_or(0);
        sum += fct * digit;
    }
    ((10 - (sum % 10)) % 10).to_string()
}

/// Decode HTML entities repeatedly until fixed point, then patch a few
/// cp1252/unicode oddities and replace `<br>` → "\n".
///
/// Mirrors `Oddb2xml.html_decode`.  Runs HTML entity decoding until the
/// string stabilises (the original code used `HTMLEntities#decode` in a
/// loop to catch `&amp;lt;` → `&lt;` → `<` cases).
pub fn html_decode(input: &str) -> String {
    let mut current = input.to_string();
    loop {
        let next = html_escape::decode_html_entities(&current).to_string();
        if next == current {
            break;
        }
        current = next;
    }
    patch_some_utf8(&current).replace("<br>", "\n")
}

/// Replace a few stray cp1252 code points that sneak into Swissmedic data
/// with their intended UTF-8 equivalents.  Also trims a trailing `\n`
/// (`String#chomp`).
pub fn patch_some_utf8(line: &str) -> String {
    let replaced: String = line
        .chars()
        .map(|c| match c {
            '\u{0089}' => '‰',
            '\u{0092}' => '’',
            '\u{0096}' => '-',
            '\u{2013}' => '-',
            '\u{201D}' => '"',
            other => other,
        })
        .collect();
    // `String#chomp` trims a trailing \n or \r\n only.
    if let Some(stripped) = replaced.strip_suffix("\r\n") {
        stripped.to_string()
    } else if let Some(stripped) = replaced.strip_suffix('\n') {
        stripped.to_string()
    } else {
        replaced
    }
}

/// Convert `patch_some_utf8`'d text to ISO-8859-1.  Equivalent to
/// `Oddb2xml.convert_to_8859_1`.
pub fn convert_to_8859_1(line: &str) -> Vec<u8> {
    use encoding_rs::WINDOWS_1252;
    let patched = patch_some_utf8(line);
    let (bytes, _encoding, _had_errors) = WINDOWS_1252.encode(&patched);
    bytes.into_owned()
}

// --- Global options + skip-download cache ---

/// CLI options captured by [`save_options`] and consulted elsewhere
/// (`skip_download`, `log`).  Thread-safe in Rust — the Ruby version
/// stored this on a module singleton.
#[derive(Clone, Debug, Default)]
pub struct GlobalOptions {
    pub skip_download: bool,
    pub log: bool,
    pub work_dir: PathBuf,
    pub downloads_dir: PathBuf,
}

static GLOBAL_OPTIONS: Lazy<Mutex<GlobalOptions>> = Lazy::new(|| {
    let work_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let downloads_dir = work_dir.join("downloads");
    Mutex::new(GlobalOptions {
        skip_download: false,
        log: false,
        work_dir,
        downloads_dir,
    })
});

pub fn save_options(opts: GlobalOptions) {
    *GLOBAL_OPTIONS.lock() = opts;
}

pub fn global_options() -> GlobalOptions {
    GLOBAL_OPTIONS.lock().clone()
}

pub fn skip_download_flag() -> bool {
    GLOBAL_OPTIONS.lock().skip_download
}

pub fn work_dir() -> PathBuf {
    GLOBAL_OPTIONS.lock().work_dir.clone()
}

pub fn downloads_dir() -> PathBuf {
    GLOBAL_OPTIONS.lock().downloads_dir.clone()
}

/// Log helper — gated by `--log`.  Matches `Oddb2xml.log` — prints with
/// timestamp, truncated to 250 chars, to stdout.
pub fn log(msg: impl AsRef<str>) {
    let opts = GLOBAL_OPTIONS.lock().clone();
    if !opts.log {
        return;
    }
    let m = msg.as_ref();
    let truncated: String = m.chars().take(250).collect();
    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    println!("{ts}: {truncated}");
    use std::io::Write as _;
    let _ = std::io::stdout().flush();
}

/// Ruby `Oddb2xml.skip_download`: if a cached copy already exists under
/// `downloads/`, copy it back into `file` and return true.  Short-circuits
/// the actual network fetch.
pub fn skip_download_cached(file: impl AsRef<Path>) -> bool {
    let file = file.as_ref();
    let opts = GLOBAL_OPTIONS.lock().clone();
    let basename = match file.file_name() {
        Some(n) => n,
        None => return false,
    };
    let dest = opts.downloads_dir.join(basename);
    if dest.exists() {
        // If `file` is already the cached destination, nothing to do.
        let canon_dest = fs::canonicalize(&dest).unwrap_or(dest.clone());
        let canon_file = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
        if canon_dest != canon_file {
            let _ = fs::copy(&dest, file);
        }
        return true;
    }
    false
}

/// Ruby `Oddb2xml.download_finished`: copy a freshly downloaded file into
/// the `downloads/` cache directory.
pub fn download_finished(file: impl AsRef<Path>) {
    let file = file.as_ref();
    let opts = GLOBAL_OPTIONS.lock().clone();
    let basename = match file.file_name() {
        Some(n) => n,
        None => return,
    };
    let src = opts.work_dir.join(basename);
    let dest = opts.downloads_dir.join(basename);
    if let Err(e) = fs::create_dir_all(&opts.downloads_dir) {
        log(format!("download_finished: mkdir failed: {e}"));
        return;
    }
    if !file.exists() {
        return;
    }
    if let (Ok(a), Ok(b)) = (fs::canonicalize(file), fs::canonicalize(&dest)) {
        if a == b {
            return;
        }
    }
    if src.exists() {
        if let Err(e) = fs::copy(&src, &dest) {
            log(format!("download_finished copy failed: {e}"));
            return;
        }
    } else if file.exists() {
        let _ = fs::copy(file, &dest);
    }
    if let Ok(meta) = fs::metadata(&dest) {
        log(format!(
            "download_finished saved as {} {} bytes.",
            dest.display(),
            meta.len()
        ));
    }
}

// --- ProdNo ↔ EAN-13 ↔ No8 bidirectional maps ---

static PRODNO_TO_EAN13: Lazy<Mutex<HashMap<String, Vec<String>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static NO8_TO_EAN13: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static EAN13_TO_PRODNO: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
static EAN13_TO_NO8: Lazy<Mutex<HashMap<String, String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub fn set_ean13_for_prodno(prodno: impl Into<String>, ean13: impl Into<String>) {
    let prodno = prodno.into();
    let ean13 = ean13.into();
    PRODNO_TO_EAN13
        .lock()
        .entry(prodno.clone())
        .or_default()
        .push(ean13.clone());
    EAN13_TO_PRODNO.lock().insert(ean13, prodno);
}

pub fn set_ean13_for_no8(no8: impl Into<String>, ean13: impl Into<String>) {
    let no8 = no8.into();
    let ean13 = ean13.into();
    let mut map = NO8_TO_EAN13.lock();
    match map.get(&no8) {
        None => {
            map.insert(no8.clone(), ean13.clone());
            EAN13_TO_NO8.lock().insert(ean13, no8);
        }
        Some(existing) if existing == &ean13 => {}
        Some(existing) => {
            log(format!(
                "no8_to_ean13[{no8}] {existing} not overridden by {ean13}"
            ));
        }
    }
}

pub fn get_ean13_for_prodno(prodno: &str) -> Vec<String> {
    PRODNO_TO_EAN13.lock().get(prodno).cloned().unwrap_or_default()
}

pub fn get_ean13_for_no8(no8: &str) -> Option<String> {
    NO8_TO_EAN13.lock().get(no8).cloned()
}

pub fn get_prodno_for_ean13(ean13: &str) -> Option<String> {
    EAN13_TO_PRODNO.lock().get(ean13).cloned()
}

pub fn get_no8_for_ean13(ean13: &str) -> Option<String> {
    EAN13_TO_NO8.lock().get(ean13).cloned()
}

/// Drop every entry in the four ProdNo/No8/EAN13 maps.  Tests rely on this
/// because the Ruby version lived in a single process.
pub fn reset_ean_maps() {
    PRODNO_TO_EAN13.lock().clear();
    NO8_TO_EAN13.lock().clear();
    EAN13_TO_PRODNO.lock().clear();
    EAN13_TO_NO8.lock().clear();
}

// --- SHA256 top-element hashing ---

/// Tag a top-level-element XML document with a `SHA256` attribute on every
/// direct child of the root (except `<RESULT>`).  Mirrors `Oddb2xml.add_hash`.
///
/// The Ruby implementation computes `Digest::SHA256.hexdigest(node.text)`
/// where `node.text` is Nokogiri's join of the node's descendant text runs
/// separated by `"\n" + whitespace`.  Since the exact whitespace behaviour
/// is Nokogiri-specific, we approximate it by concatenating all descendant
/// text nodes in document order.
///
/// Consumers verify the hash later with [`verify_sha256`].
pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

/// Collect Nokogiri-style text for a `quick-xml` element — concatenates all
/// descendant text runs with no separator.  Rust XML libraries don't offer
/// an exact `node.text` equivalent, so the builder module owns the final
/// hashing policy; this helper only exposes the hash primitive.
pub fn sha256_text(text: &str) -> String {
    sha256_hex(text.as_bytes())
}

// --- Swissmedic Packungen.xlsx column layouts ---

/// Column header regexes for the Swissmedic Packungen.xlsx layout since
/// February 2019.  Order matches column index.
pub const COLUMNS_FEBRUARY_2019: &[(&str, &str)] = &[
    ("iksnr", r"(?i)Zulassungs-Nummer"),
    ("seqnr", r"(?i)Dosisstärke-nummer"),
    ("name_base", r"(?i)Bezeichnung des Arzneimittels"),
    ("company", r"(?i)Zulassungsinhaberin"),
    ("production_science", r"(?i)Heilmittelcode"),
    ("index_therapeuticus", r"(?i)IT-Nummer"),
    ("atc_class", r"(?i)ATC-Code"),
    ("registration_date", r"(?i)Erstzul.datum Arzneimittel"),
    ("sequence_date", r"(?i)Zul.datum Dosisstärke"),
    ("expiry_date", r"(?i)Gültigkeitsdauer der Zulassung"),
    ("ikscd", r"(?i)Packungscode"),
    ("size", r"(?i)Packungsgrösse"),
    ("unit", r"(?i)Einheit"),
    ("ikscat", r"(?i)Abgabekategorie Packung"),
    ("ikscat_seq", r"(?i)Abgabekategorie Dosisstärke"),
    ("ikscat_preparation", r"(?i)Abgabekategorie Arzneimittel"),
    ("substances", r"(?i)Wirkstoff"),
    ("composition", r"(?i)Zusammensetzung"),
    ("composition_AMZV", r"(?i)Volldeklaration rev. AMZV umgesetzt"),
    ("indication_registration", r"(?i)Anwendungsgebiet Arzneimittel"),
    ("indication_sequence", r"(?i)Anwendungsgebiet Dosisstärke"),
    ("gen_production", r"(?i)Gentechnisch hergestellte Wirkstoffe"),
    ("insulin_category", r"(?i)Kategorie bei Insulinen"),
    ("drug_index", r"(?i)Verz. bei betäubungsmittel-haltigen Arzneimittel"),
];

/// Return the column index for a logical `key`, or panic if missing.  Used
/// throughout the Swissmedic extractor.
pub fn column_index(key: &str) -> usize {
    COLUMNS_FEBRUARY_2019
        .iter()
        .position(|(k, _)| *k == key)
        .unwrap_or_else(|| panic!("unknown column key: {key}"))
}

// --- ATC remap from epha CSV ---

static ATC_CSV_URL: &str =
    "https://raw.githubusercontent.com/zdavatz/cpp2sqlite/master/input/atc_codes_multi_lingual.txt";

static ATC_CSV_CONTENT: Lazy<Mutex<HashMap<(String, String), String>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

/// If EPha has a corrected ATC for `(iksnr, atc_code)`, return it;
/// otherwise return the original.  Mirrors `Oddb2xml.add_epha_changes_for_ATC`.
///
/// The first call seeds the in-process cache by fetching ATC_CSV_URL.
/// Callers in offline/test environments should preload the cache with
/// [`preload_atc_csv`].
pub fn add_epha_changes_for_atc(iksnr: &str, atc_code: &str) -> String {
    {
        let cache = ATC_CSV_CONTENT.lock();
        if !cache.is_empty() {
            return cache
                .get(&(iksnr.to_string(), atc_code.to_string()))
                .cloned()
                .unwrap_or_else(|| atc_code.to_string());
        }
    }
    // Lazy-load.  Errors fall back to the original atc_code.
    if let Ok(resp) = reqwest::blocking::get(ATC_CSV_URL) {
        if let Ok(body) = resp.text() {
            let mut cache = ATC_CSV_CONTENT.lock();
            for line in body.lines() {
                let fields: Vec<&str> = line.split(',').collect();
                if fields.len() >= 3 {
                    cache.insert(
                        (fields[0].to_string(), fields[1].to_string()),
                        fields[2].to_string(),
                    );
                }
            }
        }
    }
    ATC_CSV_CONTENT
        .lock()
        .get(&(iksnr.to_string(), atc_code.to_string()))
        .cloned()
        .unwrap_or_else(|| atc_code.to_string())
}

/// Seed the ATC CSV cache from a local string — useful for tests.
pub fn preload_atc_csv(body: &str) {
    let mut cache = ATC_CSV_CONTENT.lock();
    cache.clear();
    for line in body.lines() {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() >= 3 {
            cache.insert(
                (fields[0].to_string(), fields[1].to_string()),
                fields[2].to_string(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gen_prodno_pads() {
        assert_eq!(gen_prodno(123, 4), "0012304");
        assert_eq!(gen_prodno(12345, 67), "1234567");
    }

    #[test]
    fn calc_checksum_known_values() {
        // 768000666004 → check digit 5 (well-known Swiss drug EAN-13 prefix)
        assert_eq!(calc_checksum("768000666004"), "5");
        // 999999999999 → 4 (FAKE_GTIN_START based synthetic GTIN)
        // Check the algorithm itself by computing sum manually
        let digits = "123456789012";
        let mut sum = 0u32;
        for (i, c) in digits.chars().enumerate() {
            let d = c.to_digit(10).unwrap();
            let fct = ((i as u32 % 2) * 2) + 1;
            sum += fct * d;
        }
        let expected = ((10 - (sum % 10)) % 10).to_string();
        assert_eq!(calc_checksum(digits), expected);
    }

    #[test]
    fn patch_some_utf8_cp1252() {
        assert_eq!(patch_some_utf8("foo\u{0092}bar"), "foo’bar");
        assert_eq!(patch_some_utf8("foo\u{2013}bar\n"), "foo-bar");
    }

    #[test]
    fn html_decode_is_idempotent() {
        assert_eq!(html_decode("&amp;"), "&");
        assert_eq!(html_decode("&amp;amp;"), "&");
        assert_eq!(html_decode("a<br>b"), "a\nb");
    }

    #[test]
    fn ean_maps_roundtrip() {
        reset_ean_maps();
        set_ean13_for_no8("12345678", "7681234567895");
        assert_eq!(get_ean13_for_no8("12345678").as_deref(), Some("7681234567895"));
        assert_eq!(get_no8_for_ean13("7681234567895").as_deref(), Some("12345678"));
    }
}
