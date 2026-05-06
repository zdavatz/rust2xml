//! BAG Indikationscode → XLSX exporter.
//!
//! Walks `Builder::inputs.bag`, expands every `BagPackage.indication_codes`
//! entry into one row, and writes them into an `.xlsx` workbook.  Schema:
//!
//! | Column              | Source |
//! |---------------------|--------|
//! | Indikationscode     | `BagIndicationCode.code` (XXXXX.NN) |
//! | Markenname          | `BagItem.name_de` |
//! | GTIN                | `BagPackage.ean13` |
//! | Pack-Beschreibung   | `BagPackage.desc_de` (falls back to `name_de`) |
//! | ATC                 | `BagItem.atc_code` |
//! | Preis Ex-Factory    | `BagPackage.prices.exf_price.price` |
//! | Preis Publikum      | `BagPackage.prices.pub_price.price` |
//! | Indikation          | `BagIndicationCode.text` |
//!
//! Only packages whose `indication_codes` is non-empty produce rows.  Rows
//! are sorted by code, then by brand name, then by GTIN — gives a stable,
//! pleasant-to-browse spreadsheet.
//!
//! Triggered by the `--indc-xlsx <path>` CLI option (FHIR mode).

use crate::builder::Builder;
use anyhow::{Context, Result};
use rust_xlsxwriter::{Format, Workbook};
use std::path::Path;

const HEADERS: &[&str] = &[
    "Indikationscode",
    "Markenname",
    "GTIN",
    "Pack-Beschreibung",
    "ATC",
    "Preis Ex-Factory",
    "Preis Publikum",
    "Indikation",
];

#[derive(Debug, Clone)]
pub struct Row {
    pub code: String,
    pub brand: String,
    pub gtin: String,
    pub pack_desc: String,
    pub atc: String,
    pub exf: String,
    pub pub_: String,
    pub indication: String,
}

/// Collect rows from the builder's BAG input. One row per
/// (indication code, package) pair.
pub fn collect_rows(builder: &Builder) -> Vec<Row> {
    let mut rows: Vec<Row> = Vec::new();
    for item in builder.inputs.bag.values() {
        for pkg in item.packages.values() {
            if pkg.indication_codes.is_empty() {
                continue;
            }
            for ic in &pkg.indication_codes {
                let pack_desc = if !pkg.desc_de.is_empty() {
                    pkg.desc_de.clone()
                } else {
                    item.name_de.clone()
                };
                rows.push(Row {
                    code: ic.code.clone(),
                    brand: item.name_de.clone(),
                    gtin: pkg.ean13.clone(),
                    pack_desc,
                    atc: item.atc_code.clone(),
                    exf: pkg.prices.exf_price.price.clone(),
                    pub_: pkg.prices.pub_price.price.clone(),
                    indication: ic.text.clone(),
                });
            }
        }
    }
    rows.sort_by(|a, b| {
        a.code
            .cmp(&b.code)
            .then_with(|| a.brand.cmp(&b.brand))
            .then_with(|| a.gtin.cmp(&b.gtin))
    });
    rows
}

/// Write the IndC workbook to `path`.  Returns the number of data rows
/// written (excluding the header).
pub fn write_indc_xlsx(builder: &Builder, path: &Path) -> Result<usize> {
    let rows = collect_rows(builder);
    let mut wb = Workbook::new();
    let sheet = wb.add_worksheet().set_name("Indikationscodes")?;

    let header_fmt = Format::new().set_bold().set_background_color("#DDEBF7");
    let wrap_fmt = Format::new().set_text_wrap();

    for (col, h) in HEADERS.iter().enumerate() {
        sheet.write_string_with_format(0, col as u16, *h, &header_fmt)?;
    }
    sheet.set_freeze_panes(1, 0)?;

    for (i, r) in rows.iter().enumerate() {
        let row = (i + 1) as u32;
        sheet.write_string(row, 0, &r.code)?;
        sheet.write_string(row, 1, &r.brand)?;
        sheet.write_string(row, 2, &r.gtin)?;
        sheet.write_string(row, 3, &r.pack_desc)?;
        sheet.write_string(row, 4, &r.atc)?;
        sheet.write_string(row, 5, &r.exf)?;
        sheet.write_string(row, 6, &r.pub_)?;
        sheet.write_string_with_format(row, 7, &r.indication, &wrap_fmt)?;
    }

    sheet.set_column_width(0, 14)?;
    sheet.set_column_width(1, 32)?;
    sheet.set_column_width(2, 16)?;
    sheet.set_column_width(3, 36)?;
    sheet.set_column_width(4, 12)?;
    sheet.set_column_width(5, 14)?;
    sheet.set_column_width(6, 14)?;
    sheet.set_column_width(7, 80)?;

    wb.save(path)
        .with_context(|| format!("write_indc_xlsx: failed to save {}", path.display()))?;
    Ok(rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builder::Inputs;
    use crate::fhir_support::FhirExtractor;
    use crate::options::Options;

    #[test]
    fn collect_rows_explodes_cyramza_into_two_codes() {
        let ndjson = include_str!("../tests/fixtures/cyramza.ndjson");
        let bag = FhirExtractor::new(ndjson.to_string()).to_hash().unwrap();
        let inputs = Inputs {
            bag,
            release_date: "2026-05-06".into(),
            ..Default::default()
        };
        let builder = Builder::new(Options::default(), inputs);
        let rows = collect_rows(&builder);

        let codes: std::collections::HashSet<String> =
            rows.iter().map(|r| r.code.clone()).collect();
        assert!(codes.contains("20403.01"), "missing 20403.01");
        assert!(codes.contains("20403.02"), "missing 20403.02");

        for r in &rows {
            assert!(!r.gtin.is_empty(), "GTIN must be set");
            assert!(!r.indication.is_empty(), "Indikation text must be set");
            assert!(r.brand.to_uppercase().contains("CYRAMZA"));
        }
    }

    #[test]
    fn write_indc_xlsx_creates_a_non_empty_file() {
        let ndjson = include_str!("../tests/fixtures/cyramza.ndjson");
        let bag = FhirExtractor::new(ndjson.to_string()).to_hash().unwrap();
        let inputs = Inputs {
            bag,
            release_date: "2026-05-06".into(),
            ..Default::default()
        };
        let builder = Builder::new(Options::default(), inputs);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().with_extension("xlsx");
        let written = write_indc_xlsx(&builder, &path).unwrap();
        assert!(written >= 2, "expected ≥ 2 rows, got {written}");
        let meta = std::fs::metadata(&path).unwrap();
        assert!(meta.len() > 1024, "xlsx suspiciously small");
    }
}
