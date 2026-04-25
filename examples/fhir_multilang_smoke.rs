//! End-to-end smoke for the FR/IT translation merge.
//!
//! Run with:
//!   cargo run --release --example fhir_multilang_smoke -- /tmp/fhir_de.ndjson /tmp/fhir_fr.ndjson /tmp/fhir_it.ndjson

use rust2xml::fhir_support::{merge_translations, FhirExtractor};
use std::fs;

fn main() -> anyhow::Result<()> {
    let mut args = std::env::args().skip(1);
    let de_path = args.next().expect("de path");
    let fr_path = args.next().expect("fr path");
    let it_path = args.next().expect("it path");

    let de = FhirExtractor::new(fs::read_to_string(&de_path)?).to_hash()?;
    let fr = FhirExtractor::new_with_lang(fs::read_to_string(&fr_path)?, "fr").to_hash()?;
    let it = FhirExtractor::new_with_lang(fs::read_to_string(&it_path)?, "it").to_hash()?;
    println!("Bags: de={}, fr={}, it={}", de.len(), fr.len(), it.len());

    let mut merged = de;
    merge_translations(&mut merged, fr);
    merge_translations(&mut merged, it);

    let mut both = 0usize;
    let mut de_only = 0usize;
    for item in merged.values() {
        for pkg in item.packages.values() {
            for lim in &pkg.limitations {
                if !lim.desc_fr.is_empty() && !lim.desc_it.is_empty() {
                    both += 1;
                } else if !lim.desc_de.is_empty() {
                    de_only += 1;
                }
            }
        }
    }
    println!("Limitations with all 3 languages: {both}");
    println!("Limitations with only DE:         {de_only}");

    if let Some(item) = merged.values().find(|i| {
        i.packages
            .values()
            .any(|p| p.limitations.iter().any(|l| !l.desc_it.is_empty()))
    }) {
        println!("\nSample DE/FR/IT trio:");
        for pkg in item.packages.values() {
            for lim in &pkg.limitations {
                if lim.desc_fr.is_empty() || lim.desc_it.is_empty() {
                    continue;
                }
                println!("  GTIN: {}", pkg.ean13);
                println!("  DSCRD: {}", &lim.desc_de.chars().take(80).collect::<String>());
                println!("  DSCRF: {}", &lim.desc_fr.chars().take(80).collect::<String>());
                println!("  DSCIT: {}", &lim.desc_it.chars().take(80).collect::<String>());
                return Ok(());
            }
        }
    }
    Ok(())
}
