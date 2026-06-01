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

    // Expected per BAG Rundschreiben 2026-02-19: the explicit
    // `indicationCode` extension on each limitation carries 20403.01 /
    // 20403.02 (read directly, not reconstructed from the CUD id suffix).
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

    // Each indication code should also carry its CUD limitation text
    // (read from `indication.diseaseSymptomProcedure.concept.text`).
    let mut texts: Vec<String> = Vec::new();
    for item in data.values() {
        for ic in &item.indication_codes {
            if !ic.text.is_empty() {
                texts.push(ic.text.clone());
            }
        }
    }
    assert!(
        texts.iter().any(|t| t.contains("Paclitaxel")),
        "expected CYRAMZA.01 text mentioning Paclitaxel; got texts: {:?}",
        texts.iter().map(|t| &t[..t.len().min(60)]).collect::<Vec<_>>()
    );
    assert!(
        texts.iter().any(|t| t.contains("FOLFIRI")),
        "expected CYRAMZA.02 text mentioning FOLFIRI; got texts: {:?}",
        texts.iter().map(|t| &t[..t.len().min(60)]).collect::<Vec<_>>()
    );
}

#[test]
fn fhir_uses_explicit_indication_code_not_dossier_suffix_derivation() {
    use rust2xml::fhir_support::FhirExtractor;
    use serde_json::Value;
    use std::collections::HashSet;

    // The BAG changelog (>= v2.0.5) states the limitation code (CUD id) and
    // the indication code are independent.  Rewrite the explicit
    // `indicationCode` values so they no longer match FOPHDossierNumber +
    // CUD suffix, and confirm the extractor surfaces the explicit values.
    let ndjson = include_str!("fixtures/cyramza.ndjson");
    let mut bundle: Value = serde_json::from_str(ndjson).unwrap();
    for entry in bundle["entry"].as_array_mut().unwrap() {
        let res = &mut entry["resource"];
        if res["resourceType"] != "RegulatedAuthorization" {
            continue;
        }
        let Some(inds) = res["indication"].as_array_mut() else { continue };
        for ind in inds {
            let Some(exts) = ind["extension"].as_array_mut() else { continue };
            for ext in exts {
                if !ext["url"].as_str().unwrap_or("").contains("regulatedAuthorization-limitation") {
                    continue;
                }
                let Some(subs) = ext["extension"].as_array_mut() else { continue };
                for sub in subs {
                    if sub["url"] == "indicationCode" {
                        sub["valueString"] = Value::String("99999.77".into());
                    }
                }
            }
        }
    }
    let rewritten = serde_json::to_string(&bundle).unwrap();
    let data = FhirExtractor::new(rewritten).to_hash().unwrap();

    let mut all_codes: HashSet<String> = HashSet::new();
    for item in data.values() {
        for ic in &item.indication_codes {
            all_codes.insert(ic.code.clone());
        }
    }
    assert!(all_codes.contains("99999.77"), "expected explicit 99999.77 in {:?}", all_codes);
    assert!(!all_codes.contains("20403.01"), "must not re-derive 20403.01: {:?}", all_codes);
    assert!(!all_codes.contains("20403.02"), "must not re-derive 20403.02: {:?}", all_codes);
}

#[test]
fn cyramza_fhir_fills_limitation_descriptions_in_all_three_languages() {
    use rust2xml::fhir_support::{merge_translations, FhirExtractor};
    use serde_json::Value;
    use std::collections::HashMap;

    let ndjson = include_str!("fixtures/cyramza.ndjson");

    // The live BAG FHIR feed never stores limitation text inline, only a
    // reference to a ClinicalUseDefinition whose `concept.text` differs
    // per language.  We rewrite the CUD text of the bundle for each
    // language, feed it through the extractor as the per-language
    // NDJSON file, and merge.
    fn variant(src: &str, lang_texts: &HashMap<&str, &str>) -> String {
        let mut bundle: Value = serde_json::from_str(src).unwrap();
        for entry in bundle["entry"].as_array_mut().unwrap() {
            let res = &mut entry["resource"];
            if res["resourceType"] != "ClinicalUseDefinition" {
                continue;
            }
            let id = res["id"].as_str().unwrap_or("").to_string();
            if let Some(new_text) = lang_texts.get(id.as_str()) {
                res["indication"]["diseaseSymptomProcedure"]["concept"]["text"] =
                    Value::String((*new_text).into());
            }
        }
        serde_json::to_string(&bundle).unwrap()
    }

    let fr_text_01 = "FR limitation pour CYRAMZA.01";
    let fr_text_02 = "FR limitation pour CYRAMZA.02";
    let it_text_01 = "IT limitazione per CYRAMZA.01";
    let it_text_02 = "IT limitazione per CYRAMZA.02";

    let mut merged = FhirExtractor::new_with_lang(ndjson.to_string(), "de")
        .to_hash()
        .unwrap();

    // Sanity: DE limitation text was resolved from the CUD reference.
    let de_pkg = merged
        .values()
        .next()
        .unwrap()
        .packages
        .values()
        .next()
        .unwrap()
        .clone();
    assert!(
        !de_pkg.limitations.is_empty(),
        "DE pass must produce limitations"
    );
    let de_refs: Vec<String> = de_pkg.limitations.iter().map(|l| l.cud_ref.clone()).collect();
    assert!(
        de_refs.iter().any(|r| r == "CYRAMZA.01"),
        "cud_ref CYRAMZA.01: {:?}",
        de_refs
    );
    assert!(
        de_refs.iter().any(|r| r == "CYRAMZA.02"),
        "cud_ref CYRAMZA.02: {:?}",
        de_refs
    );
    let de_texts: Vec<String> = de_pkg.limitations.iter().map(|l| l.desc_de.clone()).collect();
    assert!(
        de_texts.iter().any(|t| t.contains("Paclitaxel")),
        "DescDe should resolve via CUD ref; got {:?}",
        de_texts
    );

    // Build FR + IT variants and merge.
    let fr_map: HashMap<&str, &str> = [
        ("CYRAMZA.01", fr_text_01),
        ("CYRAMZA.02", fr_text_02),
    ]
    .into_iter()
    .collect();
    let it_map: HashMap<&str, &str> = [
        ("CYRAMZA.01", it_text_01),
        ("CYRAMZA.02", it_text_02),
    ]
    .into_iter()
    .collect();

    let fr = FhirExtractor::new_with_lang(variant(ndjson, &fr_map), "fr")
        .to_hash()
        .unwrap();
    let it = FhirExtractor::new_with_lang(variant(ndjson, &it_map), "it")
        .to_hash()
        .unwrap();

    merge_translations(&mut merged, fr);
    merge_translations(&mut merged, it);

    let pkg = merged.values().next().unwrap().packages.values().next().unwrap();
    let by_ref: HashMap<&str, &rust2xml::extractor::BagLimitation> = pkg
        .limitations
        .iter()
        .map(|l| (l.cud_ref.as_str(), l))
        .collect();

    let l01 = by_ref.get("CYRAMZA.01").expect("CYRAMZA.01 limitation");
    let l02 = by_ref.get("CYRAMZA.02").expect("CYRAMZA.02 limitation");

    assert_eq!(l01.desc_fr, fr_text_01);
    assert_eq!(l02.desc_fr, fr_text_02);
    assert_eq!(l01.desc_it, it_text_01);
    assert_eq!(l02.desc_it, it_text_02);
    // DE text survives the merge.
    assert!(l01.desc_de.contains("Paclitaxel"));
}

#[test]
fn cyramza_bundle_emits_indikationscode_into_product_article_limitation() {
    use rust2xml::fhir_support::FhirExtractor;

    let ndjson = include_str!("fixtures/cyramza.ndjson");
    let bag = FhirExtractor::new(ndjson.to_string()).to_hash().unwrap();
    assert!(!bag.is_empty());

    let inputs = Inputs {
        bag,
        release_date: "2026-05-06".into(),
        ..Default::default()
    };
    let builder = Builder::new(Options::default(), inputs);

    let product = builder.build_product().unwrap();
    assert!(
        product.contains("<INDIKATIONSCODE>20403.01</INDIKATIONSCODE>")
            || product.contains("<INDIKATIONSCODE>20403.01,20403.02</INDIKATIONSCODE>")
            || product.contains("<INDIKATIONSCODE>20403.02,20403.01</INDIKATIONSCODE>"),
        "PRD missing INDIKATIONSCODE — got: {}",
        product.lines().filter(|l| l.contains("INDIKATIONSCODE")).collect::<Vec<_>>().join("\n")
    );

    let article = builder.build_article().unwrap();
    assert!(
        article.contains("<INDIKATIONSCODE>20403.01"),
        "ART missing INDIKATIONSCODE — got: {}",
        article.lines().filter(|l| l.contains("INDIKATIONSCODE")).collect::<Vec<_>>().join("\n")
    );

    let lim = builder.build_limitation().unwrap();
    assert!(
        lim.contains("<INDIKATIONSCODE>20403."),
        "LIM missing INDIKATIONSCODE — got: {}",
        lim.lines().filter(|l| l.contains("INDIKATIONSCODE")).collect::<Vec<_>>().join("\n")
    );

    // Limitation text per code lands in INDIKATIONSCODE_TEXT.
    assert!(
        product.contains("INDIKATIONSCODE_TEXT") && product.contains("Paclitaxel"),
        "PRD missing INDIKATIONSCODE_TEXT or text — got product slice: {}",
        product.lines().filter(|l| l.contains("INDIKATIONSCODE")).collect::<Vec<_>>().join("\n")
    );
    assert!(
        article.contains("INDIKATIONSCODE_TEXT") && article.contains("FOLFIRI"),
        "ART missing INDIKATIONSCODE_TEXT or text — got article slice: {}",
        article.lines().filter(|l| l.contains("INDIKATIONSCODE")).collect::<Vec<_>>().join("\n")
    );
}
