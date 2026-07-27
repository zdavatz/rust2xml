//! Preferred German article names from the "Rogger Mediliste" — port of
//! `lib/oddb2xml/rogger_names.rb`.
//!
//! The list is the name-conflict catalogue maintained by Frau Rogger
//! (Vitabyte / Zur Rose, task #OX-5985-1594). Its source of truth is the
//! shared Google Sheet "Rogger Mediliste" (`GTIN,Mediname`);
//! [`crate::downloader::RoggerDownloader`] fetches that sheet's CSV export
//! directly, so edits reach the feeds without a release step. A bundled copy
//! under `data/rogger_liste.csv` is embedded via `include_str!` and serves as
//! the offline fallback (refresh it at release time when the sheet changed).
//!
//! A response that is not the expected CSV — e.g. the Google sign-in page you
//! get when the sheet is no longer shared as "anyone with the link can view" —
//! is rejected by [`looks_like_rogger_csv`] and the bundled copy engages.
//!
//! Activated with `-r` / `--rogger`: for every GTIN on the list the German
//! description coming from Refdata is replaced by the list's `Mediname`. The
//! list is German-only, so `DSCRF` / `DSCRI` are left untouched. [`apply`]
//! runs right after [`crate::refdata_cleanup::apply`] in [`crate::builder::Builder::new`],
//! so the list sees — and wins over — the issue-#112 Refdata cleanups.

use crate::builder::Inputs;
use std::collections::HashMap;

/// Bundled fallback copy (offline / download-blocked hosts).
pub const BUNDLED_ROGGER: &str = include_str!("../data/rogger_liste.csv");

/// GTIN (13-digit, zero-padded) → preferred German article name.
pub type RoggerMap = HashMap<String, String>;

/// Pick the usable source and parse it: the freshly downloaded sheet when it
/// really is the expected CSV, otherwise the bundled copy. Mirrors Ruby's
/// `RoggerNames.source` + `.load`. Never fails — a malformed source yields
/// fewer (or zero) entries so the rest of the build proceeds.
pub fn load(downloaded: Option<&str>) -> RoggerMap {
    let content = match downloaded {
        Some(text) if looks_like_rogger_csv(text) => text,
        _ => {
            crate::util::log("RoggerNames: using bundled rogger_liste.csv");
            BUNDLED_ROGGER
        }
    };
    let map = parse(content);
    crate::util::log(format!("RoggerNames: {} preferred names loaded", map.len()));
    map
}

/// True when the content is the expected sheet export: a CSV whose header row
/// carries the `GTIN` and `Mediname` columns. Rejects empty bodies and the
/// HTML sign-in / error pages Google serves for non-shared sheets.
pub fn looks_like_rogger_csv(content: &str) -> bool {
    let header = strip_bom(content).lines().next().unwrap_or_default();
    let header = header.to_lowercase();
    header.contains("gtin") && header.contains("mediname")
}

/// Parse the two-column CSV into the GTIN → name map. Rows whose GTIN is not
/// 13 digits (after zero-padding) or whose name is empty are skipped, exactly
/// like the Ruby `parse`.
pub fn parse(csv_string: &str) -> RoggerMap {
    let mut map = RoggerMap::new();
    if csv_string.trim().is_empty() {
        return map;
    }
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(strip_bom(csv_string).as_bytes());
    let headers = match rdr.headers() {
        Ok(h) => h.clone(),
        Err(_) => return map,
    };
    let gtin_i = match header_index(&headers, "gtin") {
        Some(i) => i,
        None => return map,
    };
    let name_i = match header_index(&headers, "mediname") {
        Some(i) => i,
        None => return map,
    };
    for record in rdr.records().flatten() {
        let gtin = pad_gtin(record.get(gtin_i).unwrap_or_default().trim());
        let name = record.get(name_i).unwrap_or_default().trim();
        if name.is_empty() {
            continue;
        }
        if gtin.len() != 13 || !gtin.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        map.insert(gtin, name.to_string());
    }
    map
}

/// Replace the German Refdata description with the list's preferred name for
/// every listed GTIN, across both the pharma and non-pharma Refdata maps.
/// No-op on an empty map, so it is safe to call unconditionally (Ruby gates
/// the same way — the CLI hands the builder `{}` unless `-r` was given).
pub fn apply(inputs: &mut Inputs) {
    if inputs.rogger_names.is_empty() {
        return;
    }
    let names = inputs.rogger_names.clone();
    let mut count = 0usize;
    for map in [&mut inputs.refdata_pharma, &mut inputs.refdata_nonpharma] {
        for item in map.values_mut() {
            let name = match names.get(&pad_gtin(&item.ean13)) {
                Some(n) => n,
                None => continue,
            };
            if name.is_empty() || &item.desc_de == name {
                continue;
            }
            item.desc_de = name.clone();
            count += 1;
        }
    }
    if count > 0 {
        crate::util::log(format!(
            "RoggerNames: overrode {count} German description(s)"
        ));
    }
}

/// Ruby's `to_s.rjust(13, "0")` — the sheet may carry GTINs with a lost
/// leading zero, and Refdata EANs are already 13 digits.
fn pad_gtin(gtin: &str) -> String {
    let gtin = gtin.trim();
    if gtin.len() >= 13 {
        return gtin.to_string();
    }
    format!("{:0>13}", gtin)
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

fn header_index(headers: &csv::StringRecord, want: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim().trim_start_matches('\u{feff}').eq_ignore_ascii_case(want))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractor::RefdataItem;

    #[test]
    fn parses_gtin_and_mediname() {
        let csv = "GTIN,Mediname\n\
                   7680672570037,RINVOQ Ret Tabl 30 mg 28 Stk\n\
                   7680658280011,ESOMEPRAZOL Spirig HC Filmtabl 20 mg 14 Stk\n";
        let map = parse(csv);
        assert_eq!(map.len(), 2);
        assert_eq!(
            map.get("7680672570037").map(String::as_str),
            Some("RINVOQ Ret Tabl 30 mg 28 Stk")
        );
    }

    #[test]
    fn skips_short_gtins_and_empty_names() {
        // A 12-digit GTIN pads to 13 and stays; a 5-digit one pads to 13 too,
        // so the real guard is the digits-only check plus the empty name skip.
        let csv = "GTIN,Mediname\n\
                   7680672570037,\n\
                   not-a-gtin,SOMETHING\n\
                   680672570037,PADDED Tabl 1 Stk\n";
        let map = parse(csv);
        assert_eq!(map.len(), 1);
        assert_eq!(
            map.get("0680672570037").map(String::as_str),
            Some("PADDED Tabl 1 Stk")
        );
    }

    #[test]
    fn rejects_google_sign_in_page_and_falls_back_to_bundled() {
        assert!(!looks_like_rogger_csv("<!DOCTYPE html><html><head><title>Sign in</title>"));
        assert!(!looks_like_rogger_csv(""));
        assert!(looks_like_rogger_csv("GTIN,Mediname\n7680672570037,RINVOQ\n"));
        // An unusable download must still yield the bundled list.
        let map = load(Some("<!DOCTYPE html><html>Sign in</html>"));
        assert!(!map.is_empty(), "bundled rogger_liste.csv should be parsed");
    }

    #[test]
    fn bundled_list_parses() {
        let map = parse(BUNDLED_ROGGER);
        assert!(map.len() >= 50, "bundled list has {} entries", map.len());
        assert_eq!(
            map.get("7680672570037").map(String::as_str),
            Some("RINVOQ Ret Tabl 30 mg 28 Stk")
        );
    }

    #[test]
    fn apply_overrides_german_only() {
        let mut inputs = Inputs::default();
        inputs.refdata_pharma.insert(
            "7680672570037".into(),
            RefdataItem {
                ean13: "7680672570037".into(),
                desc_de: "RINVOQ Ret Tabl 30 mg 28 Stk (old)".into(),
                desc_fr: "RINVOQ cpr ret 30 mg 28 pce".into(),
                desc_it: "RINVOQ cpr 30 mg 28 pce".into(),
                ..Default::default()
            },
        );
        inputs
            .rogger_names
            .insert("7680672570037".into(), "RINVOQ Ret Tabl 30 mg 28 Stk".into());

        apply(&mut inputs);

        let item = &inputs.refdata_pharma["7680672570037"];
        assert_eq!(item.desc_de, "RINVOQ Ret Tabl 30 mg 28 Stk");
        // German-only: FR/IT untouched.
        assert_eq!(item.desc_fr, "RINVOQ cpr ret 30 mg 28 pce");
        assert_eq!(item.desc_it, "RINVOQ cpr 30 mg 28 pce");
    }

    #[test]
    fn apply_is_a_noop_without_the_flag() {
        let mut inputs = Inputs::default();
        inputs.refdata_pharma.insert(
            "7680672570037".into(),
            RefdataItem {
                ean13: "7680672570037".into(),
                desc_de: "ORIGINAL NAME".into(),
                ..Default::default()
            },
        );
        apply(&mut inputs); // rogger_names empty — the -r flag was not given
        assert_eq!(inputs.refdata_pharma["7680672570037"].desc_de, "ORIGINAL NAME");
    }
}
