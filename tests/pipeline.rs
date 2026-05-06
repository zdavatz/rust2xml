//! End-to-end-ish tests: feed canned fixtures through extractor → builder
//! without touching the network.

use rust2xml::builder::{Builder, Inputs};
use rust2xml::extractor::BagXmlExtractor;
use rust2xml::options::Options;

#[test]
fn bag_xml_to_product_xml_contains_sha256_and_name() {
    let fixture = include_str!("fixtures/preparations_minimal.xml");
    let bag = BagXmlExtractor::new(fixture.to_string()).to_hash().unwrap();
    assert_eq!(bag.len(), 1, "one preparation should parse");

    let item = bag.values().next().unwrap();
    assert_eq!(item.name_de, "AspirinCardio 100");
    assert_eq!(item.atc_code, "B01AC06");
    assert_eq!(item.substances.len(), 1);
    assert_eq!(item.packages.len(), 1);

    let inputs = Inputs {
        bag,
        release_date: "2026-04-24".into(),
        ..Default::default()
    };
    let builder = Builder::new(Options::default(), inputs);

    let article = builder.build_article().unwrap();
    assert!(article.contains("<ARTICLE"), "has <ARTICLE root");
    assert!(article.contains("SHA256="), "has SHA256 attribute");
    assert!(article.contains("7680551230013"), "has EAN-13");

    let product = builder.build_product().unwrap();
    assert!(product.contains("<PRODUCT"));
    assert!(product.contains("55123"));

    let substance = builder.build_substance().unwrap();
    assert!(substance.contains("acidum acetylsalicylicum"));
}

#[test]
fn fhir_extracts_indikationscodes_from_cyramza_bundle() {
    use rust2xml::fhir_support::FhirExtractor;
    use std::collections::HashSet;

    let ndjson = include_str!("fixtures/cyramza.ndjson");
    let data = FhirExtractor::new(ndjson.to_string()).to_hash().unwrap();
    assert!(!data.is_empty(), "CYRAMZA fixture should yield at least one item");

    // Expected per BAG Rundschreiben 2026-02-19: FOPHDossierNumber=20403,
    // CUDs CYRAMZA.01 and CYRAMZA.02 → codes 20403.01, 20403.02.
    let mut all_codes: HashSet<String> = HashSet::new();
    for item in data.values() {
        for ic in &item.indication_codes {
            all_codes.insert(ic.code.clone());
        }
        for pkg in item.packages.values() {
            // Item-level and package-level lists must agree (same bundle).
            let pkg_codes: Vec<String> = pkg.indication_codes.iter().map(|c| c.code.clone()).collect();
            let item_codes: Vec<String> = item.indication_codes.iter().map(|c| c.code.clone()).collect();
            assert_eq!(pkg_codes, item_codes);
        }
    }
    assert!(all_codes.contains("20403.01"), "expected 20403.01 in {:?}", all_codes);
    assert!(all_codes.contains("20403.02"), "expected 20403.02 in {:?}", all_codes);
}
