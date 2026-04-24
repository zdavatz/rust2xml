//! End-to-end-ish tests: feed canned fixtures through extractor → builder
//! without touching the network.

use oddb2xml::builder::{Builder, Inputs};
use oddb2xml::extractor::BagXmlExtractor;
use oddb2xml::options::Options;

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
