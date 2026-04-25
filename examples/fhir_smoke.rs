//! End-to-end smoke test for the FHIR pipeline → SQLite export.
//!
//! Run from a directory with `fhir_package_bundle.ndjson` in place
//! (or whatever path the FhirDownloader caches at):
//!     cargo run --release --example fhir_smoke

use rust2xml::fhir_support::FhirExtractor;
use std::fs;

fn main() -> anyhow::Result<()> {
    let path = std::env::args().nth(1).unwrap_or_else(|| "fhir_package_bundle.ndjson".into());
    println!("Reading {path}");
    let ndjson = fs::read_to_string(&path)?;
    let extractor = FhirExtractor::new(ndjson);
    let start = std::time::Instant::now();
    let bag = extractor.to_hash()?;
    println!("Extracted {} BagItems in {:.2}s", bag.len(), start.elapsed().as_secs_f64());

    // Spot-check: how many packages have prices and limitations?
    let mut with_exf = 0usize;
    let mut with_pub = 0usize;
    let mut with_lim = 0usize;
    let mut total_packages = 0usize;
    for item in bag.values() {
        for pkg in item.packages.values() {
            total_packages += 1;
            if !pkg.prices.exf_price.price.is_empty() {
                with_exf += 1;
            }
            if !pkg.prices.pub_price.price.is_empty() {
                with_pub += 1;
            }
            if !pkg.limitations.is_empty() {
                with_lim += 1;
            }
        }
    }
    println!("Packages: {total_packages}");
    println!("  with ex-factory price: {with_exf}");
    println!("  with public price:     {with_pub}");
    println!("  with limitations:      {with_lim}");

    // Count RegulatedAuthorization resources in the source data.
    use rust2xml::fhir_support::FhirResource;
    let mut ra_total = 0usize;
    let mut ra_to_mpd = 0usize;
    let mut ra_with_reimb = 0usize;
    let ndjson = fs::read_to_string(&path)?;
    for (lineno, line) in ndjson.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let bundle: FhirResource = match serde_json::from_str(line) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("parse line {}: {e}", lineno + 1);
                continue;
            }
        };
        for entry in &bundle.entry {
            if entry.resource.resource_type == "RegulatedAuthorization" {
                ra_total += 1;
                if entry.resource.subject.iter().any(|s| {
                    s.reference
                        .as_deref()
                        .map(|r| r.contains("MedicinalProductDefinition"))
                        .unwrap_or(false)
                }) {
                    ra_to_mpd += 1;
                }
                if entry
                    .resource
                    .extension
                    .iter()
                    .any(|e| e.url.ends_with("/reimbursementSL"))
                {
                    ra_with_reimb += 1;
                }
            }
        }
    }
    println!("\nRegulatedAuthorization stats:");
    println!("  total:                {ra_total}");
    println!("  targeting an MPD:     {ra_to_mpd}");
    println!("  with reimbursementSL: {ra_with_reimb}");

    // Also stash a one-row sample for visual inspection.
    if let Some(item) = bag.values().find(|i| {
        i.packages
            .values()
            .any(|p| !p.limitations.is_empty() || !p.prices.exf_price.price.is_empty())
    }) {
        println!("\nSample item with prices/limitations:");
        println!("  name_de = {}", item.name_de);
        println!("  atc     = {}", item.atc_code);
        if let Some(pkg) = item.packages.values().next() {
            println!("  ean13   = {}", pkg.ean13);
            println!("  exf     = {} ({})", pkg.prices.exf_price.price, pkg.prices.exf_price.price_code);
            println!("  pub     = {} ({})", pkg.prices.pub_price.price, pkg.prices.pub_price.price_code);
            for (i, lim) in pkg.limitations.iter().enumerate() {
                let snippet: String = lim.desc_de.chars().take(120).collect();
                println!("  lim[{i}] type={} vdate={} desc={snippet}…", lim.r#type, lim.vdate);
            }
        }
    }

    Ok(())
}
