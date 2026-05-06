//! One-off probe: run the FHIR feed through Builder + sqlite_export
//! and count Indikationscodes in the resulting SQLite tables.
//!
//!     cargo run --release --example indc_sqlite_count -- /path/to/foph-sl-export-latest-de.ndjson

use rust2xml::builder::{Builder, Inputs};
use rust2xml::fhir_support::FhirExtractor;
use rust2xml::options::Options;
use rust2xml::sqlite_export::write_sqlite;
use rusqlite::Connection;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let ndjson_path = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "foph-sl-export-latest-de.ndjson".into());
    let ndjson = fs::read_to_string(&ndjson_path)?;
    println!("Reading {} ({} bytes)", ndjson_path, ndjson.len());

    let bag = FhirExtractor::new(ndjson).to_hash()?;
    println!("Extractor: {} BagItems", bag.len());

    // BagItem-level counts (what the builder will see).
    let mut items_with_codes = 0usize;
    let mut total_codes = 0usize;
    let mut codes_with_text = 0usize;
    let mut distinct_codes = HashSet::new();
    for item in bag.values() {
        if !item.indication_codes.is_empty() {
            items_with_codes += 1;
        }
        for ic in &item.indication_codes {
            total_codes += 1;
            distinct_codes.insert(ic.code.clone());
            if !ic.text.is_empty() {
                codes_with_text += 1;
            }
        }
    }
    println!("BagItem stats:");
    println!("  items with IndC : {items_with_codes}");
    println!("  total codes     : {total_codes}");
    println!("  distinct codes  : {}", distinct_codes.len());
    println!("  codes with text : {codes_with_text}");

    // Run Builder + write SQLite.
    let inputs = Inputs {
        bag,
        release_date: "2026-05-06".into(),
        ..Default::default()
    };
    let builder = Builder::new(Options::default(), inputs);
    let path = PathBuf::from("/tmp/indc_count.sqlite");
    let _ = fs::remove_file(&path);
    write_sqlite(&builder, &path)?;
    println!("\nWrote {}", path.display());

    let conn = Connection::open(&path)?;

    for table in &["products", "articles", "limitations"] {
        let total: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM {table}"),
            [],
            |row| row.get(0),
        )?;
        let with_code: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM {table} WHERE INDIKATIONSCODE IS NOT NULL AND INDIKATIONSCODE <> ''"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1);
        let with_text: i64 = conn
            .query_row(
                &format!(
                    "SELECT COUNT(*) FROM {table} WHERE INDIKATIONSCODE_TEXT IS NOT NULL AND INDIKATIONSCODE_TEXT <> ''"
                ),
                [],
                |row| row.get(0),
            )
            .unwrap_or(-1);
        println!("  {table:<12} : {total} rows, {with_code} with IndC, {with_text} with IndC text");
    }

    // Distinct codes that survived into the products.INDIKATIONSCODE column.
    let mut stmt = conn.prepare(
        "SELECT INDIKATIONSCODE FROM products WHERE INDIKATIONSCODE IS NOT NULL AND INDIKATIONSCODE <> ''",
    )?;
    let mut sqlite_codes: HashSet<String> = HashSet::new();
    for row in stmt.query_map([], |row| row.get::<_, String>(0))? {
        for code in row?.split(',') {
            let s = code.trim();
            if !s.is_empty() {
                sqlite_codes.insert(s.to_string());
            }
        }
    }
    println!("\nDistinct codes in products.INDIKATIONSCODE: {}", sqlite_codes.len());

    let only_in_bag: HashSet<_> = distinct_codes.difference(&sqlite_codes).cloned().collect();
    let only_in_sqlite: HashSet<_> = sqlite_codes.difference(&distinct_codes).cloned().collect();
    println!("  in BagItems but not in SQLite : {}", only_in_bag.len());
    println!("  in SQLite but not in BagItems : {}", only_in_sqlite.len());

    Ok(())
}
