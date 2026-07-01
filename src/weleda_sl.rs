//! Weleda / WALA Kapitel-70 SL recovery — port of `lib/oddb2xml/weleda_sl.rb`.
//!
//! Recovers the SL reimbursement flag and the public price for the Swiss
//! "Kapitel 70" complementary medicines (Homöopathika / Anthroposophika /
//! Phytotherapeutika) that are *not* present in the BAG FHIR feed. These
//! magistral Weleda products (GTIN prefix `7611916…`) and WALA products
//! (`7640187…`) otherwise arrive via ZurRose with no SL flag and a
//! zeroed/absent public price (issue #117 / #121).
//!
//! Three CSVs close the gap (downloaded from github.com/zdavatz/oddb2xml_files
//! at runtime, with a bundled `data/` copy embedded as an offline fallback):
//!
//!   * `weleda_arzneimittel.csv`  GTIN → abgabekategorie ("… / SL" flag) and
//!                                `csl` (= Pharma-Gruppen-Code).
//!   * `bag_sl_group_prices.csv`  Pharma-Gruppen-Code → public price (CHF incl.
//!                                VAT), extracted from the BAG SL definition PDF.
//!   * `wala_arzneimittel.csv`    Same gap for WALA; ";"-separated with a BOM,
//!                                SL when it carries a CSL-Code, price given
//!                                inline (already multiplied for the pack size).
//!
//! Weleda join: GTIN → csl → price. The csl may carry a package multiplier as
//! "N x <code>", so the price is `N * price[<code>]`. The FHIR feed always wins:
//! this enrichment is applied only to GTINs absent from the feed (see the
//! Artikelstamm builder).

use once_cell::sync::Lazy;
use regex::Regex;
use std::collections::HashMap;

/// Bundled fallback copies (offline / download-blocked hosts).
pub const BUNDLED_WELEDA: &str = include_str!("../data/weleda_arzneimittel.csv");
pub const BUNDLED_WALA: &str = include_str!("../data/wala_arzneimittel.csv");
pub const BUNDLED_BAG_SL_GROUP_PRICES: &str = include_str!("../data/bag_sl_group_prices.csv");

/// One recovered SL product: public price plus the group code it came from.
#[derive(Debug, Clone, Default)]
pub struct WeledaEntry {
    pub sl: bool,
    /// Public price as "NN.NN"; `None` when the group price could not be resolved.
    pub price: Option<String>,
    pub csl: String,
    pub abgabe: String,
}

/// Build the GTIN → [`WeledaEntry`] map (keyed by 13-digit GTIN) from the three
/// CSV sources. Only rows carrying a "/ SL" Abgabekategorie (Weleda) or a
/// CSL-Code (WALA) are included. Never panics — a malformed source yields fewer
/// (or zero) entries so the rest of the build proceeds.
pub fn load(weleda_csv: &str, wala_csv: &str, prices_csv: &str) -> HashMap<String, WeledaEntry> {
    let prices = parse_prices(prices_csv);
    let mut map = build_map(weleda_csv, &prices);
    let weleda_size = map.len();
    for (gtin, entry) in build_wala_map(wala_csv) {
        map.entry(gtin).or_insert(entry); // Weleda wins on the (unlikely) collision
    }
    crate::util::log(format!(
        "WeledaSL: {} SL products with prices loaded (Weleda {}, WALA {})",
        map.len(),
        weleda_size,
        map.len() - weleda_size
    ));
    map
}

/// Pharma-Gruppen-Code → unit price (String, "NN.NN").
fn parse_prices(csv_string: &str) -> HashMap<String, String> {
    let mut prices = HashMap::new();
    if csv_string.trim().is_empty() {
        return prices;
    }
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(strip_bom(csv_string).as_bytes());
    let headers = match rdr.headers() {
        Ok(h) => h.clone(),
        Err(_) => return prices,
    };
    let code_i = header_index(&headers, "pharma_group_code");
    let price_i = header_index(&headers, "price_chf_incl_vat");
    let (Some(code_i), Some(price_i)) = (code_i, price_i) else {
        return prices;
    };
    for rec in rdr.records().flatten() {
        let code = rec.get(code_i).unwrap_or("").trim().to_string();
        let price = rec.get(price_i).unwrap_or("").trim().to_string();
        if !code.is_empty() && !price.is_empty() {
            prices.insert(code, price);
        }
    }
    prices
}

fn build_map(csv_string: &str, prices: &HashMap<String, String>) -> HashMap<String, WeledaEntry> {
    let mut map = HashMap::new();
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
    let (Some(ean_i), Some(abg_i), Some(csl_i)) = (
        header_index(&headers, "ean"),
        header_index(&headers, "abgabekategorie"),
        header_index(&headers, "csl"),
    ) else {
        return map;
    };
    for rec in rdr.records().flatten() {
        let abgabe = rec.get(abg_i).unwrap_or("").trim().to_string();
        if !sl_marker().is_match(&abgabe) {
            continue;
        }
        let gtin = rjust13(rec.get(ean_i).unwrap_or("").trim());
        if !is_gtin13(&gtin) {
            continue;
        }
        let csl = rec.get(csl_i).unwrap_or("").trim().to_string();
        let price = resolve_price(&csl, prices);
        map.insert(
            gtin,
            WeledaEntry {
                sl: true,
                price,
                csl,
                abgabe,
            },
        );
    }
    map
}

/// WALA layout: ";"-separated, BOM, header columns carry trailing spaces. A row
/// is an SL product when it has a CSL-Code (Kapitel-70.01 group code); the
/// public package price is taken verbatim from the inline "CSL 70.01." column
/// (already multiplied for the pack size). Keyed by 13-digit GTIN.
fn build_wala_map(csv_string: &str) -> HashMap<String, WeledaEntry> {
    let mut map = HashMap::new();
    if csv_string.trim().is_empty() {
        return map;
    }
    let mut rdr = csv::ReaderBuilder::new()
        .has_headers(true)
        .delimiter(b';')
        .flexible(true)
        .from_reader(strip_bom(csv_string).as_bytes());
    let headers = match rdr.headers() {
        Ok(h) => h.clone(),
        Err(_) => return map,
    };
    // Headers carry trailing spaces / a trailing "*", so match on the trimmed
    // name (and, for the code column, tolerate the "*" suffix).
    let ean_i = header_index(&headers, "EAN-Code");
    let csl_i = header_index(&headers, "CSL-Code*").or_else(|| header_index(&headers, "CSL-Code"));
    let price_i = header_index(&headers, "CSL 70.01.");
    let kat_i = header_index(&headers, "KAT");
    let (Some(ean_i), Some(csl_i), Some(price_i)) = (ean_i, csl_i, price_i) else {
        return map;
    };
    for rec in rdr.records().flatten() {
        let csl = rec.get(csl_i).unwrap_or("").trim().to_string();
        if csl.is_empty() {
            continue; // no group code => not an SL product
        }
        let gtin = rjust13(rec.get(ean_i).unwrap_or("").trim());
        if !is_gtin13(&gtin) {
            continue;
        }
        let raw_price = rec.get(price_i).unwrap_or("").trim();
        if raw_price.is_empty() {
            continue;
        }
        let price = raw_price.replace(',', ".").parse::<f64>().ok();
        let Some(price) = price else { continue };
        let abgabe = kat_i
            .and_then(|i| rec.get(i))
            .unwrap_or("")
            .trim()
            .to_string();
        map.insert(
            gtin,
            WeledaEntry {
                sl: true,
                price: Some(format!("{price:.2}")),
                csl,
                abgabe,
            },
        );
    }
    map
}

/// `csl` is either "<code>" or "<N> x <code>" (the package multiplier). Returns
/// the public price as a "NN.NN" String, or `None` when it cannot be resolved.
fn resolve_price(csl: &str, prices: &HashMap<String, String>) -> Option<String> {
    let csl = csl.trim();
    if csl.is_empty() {
        return None;
    }
    static RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?i)^(?:(\d+)\s*[x×]\s*)?(\d{7})$").unwrap());
    let caps = RE.captures(csl)?;
    let multiplier: f64 = caps
        .get(1)
        .and_then(|m| m.as_str().parse::<f64>().ok())
        .unwrap_or(1.0);
    let base: f64 = prices.get(&caps[2])?.parse().ok()?;
    Some(format!("{:.2}", base * multiplier))
}

fn sl_marker() -> &'static Regex {
    static RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bSL\b").unwrap());
    &RE
}

/// Case-insensitive header lookup on the trimmed column name.
fn header_index(headers: &csv::StringRecord, name: &str) -> Option<usize> {
    headers
        .iter()
        .position(|h| h.trim().eq_ignore_ascii_case(name))
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

fn rjust13(gtin: &str) -> String {
    if gtin.len() >= 13 {
        gtin.to_string()
    } else {
        format!("{gtin:0>13}")
    }
}

fn is_gtin13(gtin: &str) -> bool {
    gtin.len() == 13 && gtin.bytes().all(|b| b.is_ascii_digit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weleda_join_resolves_group_price_with_multiplier() {
        let prices = "pharma_group_code,price_chf_incl_vat,description,limitation\n\
                      2069591,26.95,Urtinktur,\"\"\n\
                      2070631,3.00,Globuli,\"\"\n";
        let weleda = "id,name,darreichung,status,verweis,artikelnummer,pharmacode,ean,abgabekategorie,zulassungsnummer,csl\n\
                      1,\"Absinthium\",Tropfen,Lieferbar,,124755,1019849,7611916162404,FM / SL,,2069591\n\
                      2,\"Multi\",Globuli,Lieferbar,,124756,1019850,7611916162405,B / SL,,8 x 2070631\n\
                      3,\"NotSL\",Tropfen,Lieferbar,,124757,1019851,7611916162406,D,,2069591\n";
        let map = load(weleda, "", prices);
        assert_eq!(map.len(), 2, "only the two /SL rows are kept");
        assert_eq!(map["7611916162404"].price.as_deref(), Some("26.95"));
        // 8 x 3.00 = 24.00
        assert_eq!(map["7611916162405"].price.as_deref(), Some("24.00"));
        assert!(!map.contains_key("7611916162406"), "non-SL row excluded");
    }

    #[test]
    fn wala_takes_inline_price_verbatim() {
        let wala = "\u{feff}EAN-Code;Pharmacode;Bezeichnung ;Galenische Form | PGR;KAT ;CSL-Code*;CSL 70.01.\n\
                    7640187361278;4228473;Aconitum comp.;Globuli velati 20 g;D;2070358;18.35\n\
                    7640187360974;4227315;Aconitum comp.;Solutio ad inj. 10 x 1 ml;B;2070588;39\n\
                    7680687430012;1098334;Aconit;Öl 50 ml;D;;\n";
        let map = load("", wala, "");
        assert_eq!(map.len(), 2, "only rows with a CSL-Code are SL");
        assert_eq!(map["7640187361278"].price.as_deref(), Some("18.35"));
        assert_eq!(map["7640187360974"].price.as_deref(), Some("39.00"));
        assert!(!map.contains_key("7680687430012"), "no CSL-Code => skipped");
    }

    #[test]
    fn bundled_csvs_parse() {
        let map = load(BUNDLED_WELEDA, BUNDLED_WALA, BUNDLED_BAG_SL_GROUP_PRICES);
        assert!(map.len() > 500, "bundled data should yield >500 SL products, got {}", map.len());
    }
}
