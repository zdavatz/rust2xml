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

#[test]
fn artikelstamm_v6_emits_products_limitations_items_and_artsl() {
    use rust2xml::extractor::swissmedic::SwissmedicPackage;
    use rust2xml::fhir_support::FhirExtractor;

    // CYRAMZA bundle → BAG items (packages carry SL prices, limitations
    // with an explicit BAG Indikationscode, and limitation text).
    let ndjson = include_str!("fixtures/cyramza.ndjson");
    let bag = FhirExtractor::new(ndjson.to_string()).to_hash().unwrap();
    assert!(!bag.is_empty(), "CYRAMZA fixture should yield items");

    // Feed a synthetic Swissmedic pack for the first BAG package so the
    // <PRODUCTS> / <LIMITATIONS> paths (keyed on PRODNO) get exercised.
    let (pkg_ean, no8) = {
        let item = bag.values().next().unwrap();
        let pkg = item.packages.values().next().unwrap();
        (pkg.ean13.clone(), pkg.swissmedic_number8.clone())
    };
    let mut swissmedic_packages = std::collections::HashMap::new();
    swissmedic_packages.insert(
        no8.clone(),
        SwissmedicPackage {
            no8: no8.clone(),
            ean13: pkg_ean.clone(),
            prodno: "9999901".into(),
            swissmedic_category: "A".into(),
            atc_code: "L01XC21".into(),
            package_size: "1".into(),
            einheit_swissmedic: "Stück".into(),
            sequence_name: "CYRAMZA Test".into(),
            ..Default::default()
        },
    );
    // A Swissmedic-registered pack absent from BAG/Refdata/ZurRose — oddb2xml
    // emits every such pack (obj = @packs[no8]); rust2xml must too.
    let sm_only_gtin = "7680999998887";
    swissmedic_packages.insert(
        "99999888".into(),
        SwissmedicPackage {
            no8: "99999888".into(),
            ean13: sm_only_gtin.into(),
            prodno: "1234567".into(),
            swissmedic_category: "B".into(),
            atc_code: "N02BE01".into(),
            package_size: "20".into(),
            einheit_swissmedic: "Tablette(n)".into(),
            substance_swissmedic: "paracetamolum".into(),
            ith_swissmedic: "01.01.1.".into(),
            sequence_name: "SWISSMEDIC-ONLY Tabl 20 Stk".into(),
            ..Default::default()
        },
    );

    // A non-pharma article (no BAG pack) so the ITEMS/CSV also cover the
    // item-level, non-SL rows — mirroring oddb2xml, whose CSV has one row per
    // article, not just per priced pack.
    let nonpharma_gtin = "7612345678901";
    let mut refdata_nonpharma = std::collections::HashMap::new();
    refdata_nonpharma.insert(
        nonpharma_gtin.to_string(),
        rust2xml::extractor::RefdataItem {
            ean13: nonpharma_gtin.into(),
            desc_de: "TEST Non-Pharma Artikel".into(),
            ..Default::default()
        },
    );
    // A synthetic 999999+pharmacode placeholder (ZurRose article with no real
    // EAN13) — must be dropped from the Artikelstamm, like oddb2xml does.
    // FAKE_GTIN_START is "999999" (six nines) + pharmacode; a GTIN with only
    // five leading nines (e.g. a real 9999978… article) must NOT be filtered.
    let fake_gtin = "9999991234567";
    refdata_nonpharma.insert(
        fake_gtin.to_string(),
        rust2xml::extractor::RefdataItem {
            ean13: fake_gtin.into(),
            desc_de: "FAKE placeholder article".into(),
            ..Default::default()
        },
    );
    // Boundary: a real GTIN with only five leading nines must survive.
    let real_five_nine_gtin = "9999978462921";
    refdata_nonpharma.insert(
        real_five_nine_gtin.to_string(),
        rust2xml::extractor::RefdataItem {
            ean13: real_five_nine_gtin.into(),
            desc_de: "REAL five-nine article".into(),
            ..Default::default()
        },
    );
    // Veterinary article ("ad us vet") — must be dropped like oddb2xml.
    let vet_gtin = "7680427360319";
    refdata_nonpharma.insert(
        vet_gtin.to_string(),
        rust2xml::extractor::RefdataItem {
            ean13: vet_gtin.into(),
            desc_de: "MINALGIN Inj Lös ad us vet. Fl 100 ml".into(),
            ..Default::default()
        },
    );
    // A Weleda Kapitel-70 article missing from the FHIR feed (arrives via
    // Refdata/ZurRose without a price) — the Weleda/WALA SL recovery must add
    // the SL flag and the BAG group price (issue #121).
    let weleda_gtin = "7611916162404";
    refdata_nonpharma.insert(
        weleda_gtin.to_string(),
        rust2xml::extractor::RefdataItem {
            ean13: weleda_gtin.into(),
            desc_de: "Absinthium Tropfen 50 ml".into(),
            ..Default::default()
        },
    );
    let mut weleda_sl = std::collections::HashMap::new();
    weleda_sl.insert(
        weleda_gtin.to_string(),
        rust2xml::weleda_sl::WeledaEntry {
            sl: true,
            price: Some("26.95".into()),
            csl: "2069591".into(),
            abgabe: "FM / SL".into(),
        },
    );

    // TWINRIX case (July 2026 regression): a pack registered in the Swissmedic
    // register but absent from the BAG SL feed, known to Refdata and ZurRose.
    // Ruby routes it through the pharma branch (`obj = @packs[no8].merge(obj)`),
    // so the ITEM must carry the merged COMP NAME+GLN, PHAR, prices (PPUB 0.00
    // included), PKG_SIZE, MEASURE, DOSAGE_FORM(F), IKSCAT and PRODNO.
    let twinrix_gtin = "7680005920034";
    swissmedic_packages.insert(
        "00592003".into(),
        SwissmedicPackage {
            no8: "00592003".into(),
            ean13: twinrix_gtin.into(),
            prodno: "0059201".into(),
            swissmedic_category: "B".into(),
            atc_code: "J07BC20".into(),
            package_size: "1".into(),
            einheit_swissmedic: "Fertigspritze(n)".into(),
            sequence_name: "Twinrix 720/20, Injektionssuspension".into(),
            company_name: "GlaxoSmithKline AG".into(),
            ..Default::default()
        },
    );
    let mut refdata_pharma = std::collections::HashMap::new();
    refdata_pharma.insert(
        twinrix_gtin.to_string(),
        rust2xml::extractor::RefdataItem {
            ean13: twinrix_gtin.into(),
            desc_de: "TWINRIX 720/20 Inj Susp (n Fertspr) 1 ml".into(),
            desc_fr: "TWINRIX 720/20 susp inj (n ser pré) 1 ml".into(),
            company_ean: "7601001000674".into(),
            ..Default::default()
        },
    );
    let mut zurrose = std::collections::HashMap::new();
    zurrose.insert(
        twinrix_gtin.to_string(),
        rust2xml::extractor::ZurroseItem {
            data_origin: "zur_rose".into(),
            ean13: twinrix_gtin.into(),
            pharmacode: "1108362".into(),
            price: "57.41".into(),
            pub_price: "0.00".into(),
            cmut: "1".into(),
            ..Default::default()
        },
    );
    // Vaccination PRODNO patch: a register pack WITHOUT a PRODNO whose ATC is
    // a vaccine code (^J07, not J07AX) borrows the PRODNO of another pack with
    // the same ATC (here TWINRIX's 0059201), so the Elexis Impfliste keeps
    // resolving it to a product.
    let vaccine_gtin = "7680001330017";
    swissmedic_packages.insert(
        "00133001".into(),
        SwissmedicPackage {
            no8: "00133001".into(),
            ean13: vaccine_gtin.into(),
            prodno: String::new(),
            swissmedic_category: "B".into(),
            atc_code: "J07BC20".into(),
            sequence_name: "VACCINE ohne Prodno, Injektionssuspension".into(),
            ..Default::default()
        },
    );

    let inputs = Inputs {
        bag,
        swissmedic_packages,
        refdata_pharma,
        refdata_nonpharma,
        zurrose,
        weleda_sl,
        release_date: "2026-07-01".into(),
        ..Default::default()
    };
    let inputs_it = inputs.clone();
    let builder = Builder::new(Options::default(), inputs);

    let xml = builder.build_artikelstamm(6).unwrap();
    assert!(
        xml.contains("http://elexis.ch/Elexis_Artikelstamm_v6"),
        "v6 namespace missing"
    );
    assert!(xml.contains("DATA_SOURCE=\"oddb2xml\""), "DATA_SOURCE attr missing");
    assert!(xml.contains("<PRODUCTS>") && xml.contains("<ITEMS>") && xml.contains("<LIMITATIONS>"));
    assert!(xml.contains("<PRODNO>9999901</PRODNO>"), "PRODUCT PRODNO missing");
    // Limitations must be present: FHIR carries no native LIMCD, so the CUD id
    // (cud_ref) is used as <LIMNAMEBAG>.  A populated <LIMITATION> with its
    // German text proves the fallback works end-to-end.
    assert!(xml.contains("<LIMITATION>"), "no <LIMITATION> emitted");
    assert!(
        !xml.contains("<LIMNAMEBAG></LIMNAMEBAG>") && !xml.contains("<LIMNAMEBAG/>"),
        "empty <LIMNAMEBAG> — cud_ref fallback not applied"
    );
    assert!(
        xml.contains(&format!("<GTIN>{}</GTIN>", pkg_ean)),
        "pack GTIN missing from ITEMS"
    );
    // v6 ARTSL block with the explicit BAG Indikationscode.
    assert!(xml.contains("<ARTSL>") && xml.contains("<PM>true</PM>"), "ARTSL missing");
    assert!(
        xml.contains("<INDCD>20403.01</INDCD>") || xml.contains("<INDCD>20403.02</INDCD>"),
        "ARTSL INDCD missing — got: {}",
        xml.lines().filter(|l| l.contains("INDCD")).collect::<Vec<_>>().join("\n")
    );

    // By default (no --italian) the strict Elexis XSD applies: only DE + FR
    // descriptions, never <DSCRI> — emitting it makes the Elexis importer
    // reject the file with cvc-complex-type.2.4.a.
    assert!(
        !xml.contains("<DSCRI>"),
        "default output must not carry <DSCRI> (strict Elexis XSD)"
    );

    // Legacy v5 switches the namespace and drops <ARTSL> entirely.
    let xml5 = builder.build_artikelstamm(5).unwrap();
    assert!(xml5.contains("http://elexis.ch/Elexis_Artikelstamm_v5"), "v5 namespace missing");
    assert!(!xml5.contains("<ARTSL>"), "v5 must not contain <ARTSL>");
    assert!(!xml5.contains("<DSCRI>"), "v5 default must not carry <DSCRI>");

    // Opting in with --italian re-adds the Italian description leaves.
    let mut it_opts = Options::default();
    it_opts.italian = true;
    let builder_it = Builder::new(it_opts, inputs_it);
    let xml_it = builder_it.build_artikelstamm(6).unwrap();
    assert!(
        xml_it.contains("<DSCRI>"),
        "--italian output must carry <DSCRI>"
    );

    // Companion CSV has the header and at least the pack row.
    let csv = builder.artikelstamm_csv();
    assert!(csv.starts_with("gtin,name,pkg_size,galenic_form"), "CSV header missing");
    assert!(csv.contains(&pkg_ean), "CSV missing pack row");
    // Item-level alignment: the non-pharma article is present as its own CSV
    // row with only gtin + name filled and the pharma columns empty.
    assert!(
        csv.lines()
            .any(|l| l.starts_with(&format!("{nonpharma_gtin},TEST Non-Pharma Artikel,,,,,,,,,,"))),
        "CSV missing item-level non-pharma row"
    );
    // The non-pharma GTIN must also appear as an <ITEM> in the XML, so the CSV
    // and XML item sets stay identical.
    assert!(
        xml.contains(&format!("<GTIN>{nonpharma_gtin}</GTIN>")),
        "non-pharma GTIN missing from ITEMS"
    );
    // The synthetic 999999… placeholder must be filtered out of both outputs.
    assert!(
        !csv.lines().any(|l| l.starts_with(fake_gtin)),
        "fake 999999 GTIN leaked into the CSV"
    );
    assert!(
        !xml.contains(&format!("<GTIN>{fake_gtin}</GTIN>")),
        "fake 999999 GTIN leaked into the XML"
    );
    // …but a real five-nine GTIN must not be caught by the filter.
    assert!(
        csv.lines().any(|l| l.starts_with(real_five_nine_gtin)),
        "real five-nine GTIN wrongly filtered from the CSV"
    );
    // A Swissmedic-only pack (no BAG/Refdata/ZurRose) is emitted as a pharma
    // ITEM carrying its register data, matching oddb2xml.
    assert!(
        xml.contains(&format!("<GTIN>{sm_only_gtin}</GTIN>")),
        "Swissmedic-only GTIN missing from ITEMS"
    );
    assert!(
        xml.contains("<PRODNO>1234567</PRODNO>"),
        "Swissmedic-only PRODNO missing from ITEM"
    );
    assert!(
        csv.lines().any(|l| l.starts_with(&format!(
            "{sm_only_gtin},SWISSMEDIC-ONLY Tabl 20 Stk,20,"
        )) && l.ends_with(",1234567,N02BE01,paracetamolum,,01.01.1.,")),
        "Swissmedic-only CSV row missing or malformed"
    );
    // Veterinary article must be filtered out of both outputs.
    assert!(
        !csv.lines().any(|l| l.starts_with(vet_gtin)),
        "veterinary (ad us vet) article leaked into the CSV"
    );
    assert!(
        !xml.contains(&format!("<GTIN>{vet_gtin}</GTIN>")),
        "veterinary (ad us vet) article leaked into the XML"
    );
    // Weleda Kapitel-70 recovery: SL flag + BAG group price added.
    assert!(
        xml.contains(&format!("<GTIN>{weleda_gtin}</GTIN>")),
        "Weleda GTIN missing from ITEMS"
    );
    assert!(
        xml.contains("<PPUB>26.95</PPUB>") && xml.contains("<SL_ENTRY>true</SL_ENTRY>"),
        "Weleda recovery did not add PPUB + SL_ENTRY"
    );
    assert!(
        csv.lines().any(|l| l
            .starts_with(&format!("{weleda_gtin},Absinthium Tropfen 50 ml,,,,26.95,"))
            && l.ends_with(",SL")),
        "Weleda CSV row missing recovered price / SL flag"
    );

    // TWINRIX merged item: Swissmedic register + Refdata + ZurRose combined.
    let twinrix_item = xml
        .split("<ITEM ")
        .find(|s| s.contains(&format!("<GTIN>{twinrix_gtin}</GTIN>")))
        .expect("TWINRIX ITEM missing");
    for expected in [
        "<PHAR>1108362</PHAR>",
        "<DSCR>TWINRIX 720/20 Inj Susp (n Fertspr) 1 ml</DSCR>",
        "<DSCRF>TWINRIX 720/20 susp inj (n ser pré) 1 ml</DSCRF>",
        "<NAME>GlaxoSmithKline AG</NAME>",
        "<GLN>7601001000674</GLN>",
        "<PEXF>57.41</PEXF>",
        "<PPUB>0.00</PPUB>",
        "<PKG_SIZE>1</PKG_SIZE>",
        "<MEASURE>Fertigspritze(n)</MEASURE>",
        "<DOSAGE_FORM>Injektionssuspension</DOSAGE_FORM>",
        "<DOSAGE_FORMF>Suspension injectable</DOSAGE_FORMF>",
        "<IKSCAT>B</IKSCAT>",
        "<PRODNO>0059201</PRODNO>",
    ] {
        assert!(
            twinrix_item.contains(expected),
            "TWINRIX ITEM missing {expected}:\n{twinrix_item}"
        );
    }
    // Vaccination patch: the prodno-less J07 pack borrows TWINRIX's PRODNO.
    let vaccine_item = xml
        .split("<ITEM ")
        .find(|s| s.contains(&format!("<GTIN>{vaccine_gtin}</GTIN>")))
        .expect("vaccine ITEM missing");
    assert!(
        vaccine_item.contains("<PRODNO>0059201</PRODNO>"),
        "vaccination PRODNO patch not applied:\n{vaccine_item}"
    );

    // Validate against the committed v6 XSD when xmllint is available
    // (skipped silently on hosts without libxml2-utils, e.g. bare CI).
    let path = std::env::temp_dir().join("rust2xml_artikelstamm_v6_test.xml");
    std::fs::write(&path, &xml).unwrap();
    if let Ok(out) = std::process::Command::new("xmllint")
        .args(["--noout", "--schema", "Elexis_Artikelstamm_v6.xsd"])
        .arg(&path)
        .output()
    {
        assert!(
            out.status.success(),
            "generated XML failed v6 XSD validation:\n{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let _ = std::fs::remove_file(&path);
}

/// `-r`/`--rogger`: the curated "Rogger Mediliste" name replaces the German
/// Refdata description end-to-end, wins over the issue-#112 Refdata cleanups
/// (it is applied last, like Ruby's `apply_rogger_name_overrides!`), and
/// leaves the French description untouched.
#[test]
fn rogger_names_override_german_description_and_win_over_refdata_cleanup() {
    use rust2xml::extractor::swissmedic::SwissmedicPackage;
    use rust2xml::extractor::RefdataItem;

    // This GTIN is on the real bundled Rogger list.
    let gtin = "7680680690017";
    let no8 = "68069001";
    let preferred = "EZETIMIB ROSUVA Spirig HC Filmtab 10/10mg 30 Stk";

    // A description that the issue-#112 double-dose cleanup *would* rewrite
    // ("30 mg / 30 mg /" → "30 mg /"), so a passing assertion proves the
    // Rogger name is applied after — and wins over — that cleanup.
    let refdata_desc = "MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette";

    let mut refdata_pharma = std::collections::HashMap::new();
    refdata_pharma.insert(
        gtin.to_string(),
        RefdataItem {
            ean13: gtin.into(),
            no8: no8.into(),
            refdata: true,
            r#type: "PHARMA".into(),
            desc_de: refdata_desc.into(),
            desc_fr: "MIRTAZAPINE Sandoz eco 30 mg / 30 mg / 100 comprimé".into(),
            ..Default::default()
        },
    );

    // Single active substance → the double-dose cleanup is allowed to fire.
    let mut swissmedic_packages = std::collections::HashMap::new();
    swissmedic_packages.insert(
        no8.to_string(),
        SwissmedicPackage {
            no8: no8.into(),
            ean13: gtin.into(),
            substance_swissmedic: "mirtazapinum".into(),
            sequence_name: "MIRTAZAPIN Sandoz eco".into(),
            ..Default::default()
        },
    );

    let rogger_names = rust2xml::rogger_names::load(None);
    assert!(
        rogger_names.contains_key(gtin),
        "bundled Rogger list should carry the fixture GTIN"
    );

    let make = |names: rust2xml::rogger_names::RoggerMap| {
        Builder::new(
            Options::default(),
            Inputs {
                refdata_pharma: refdata_pharma.clone(),
                swissmedic_packages: swissmedic_packages.clone(),
                rogger_names: names,
                release_date: "2026-07-27".into(),
                ..Default::default()
            },
        )
    };

    // With -r: the curated name reaches <DSCRD>, the Refdata one is gone.
    let with_rogger = make(rogger_names).build_article().unwrap();
    assert!(
        with_rogger.contains(&format!("<DSCRD>{preferred}</DSCRD>")),
        "Rogger name not emitted:\n{with_rogger}"
    );
    assert!(
        !with_rogger.contains("MIRTAZAPIN Sandoz eco 30 mg"),
        "Refdata description survived the Rogger override:\n{with_rogger}"
    );
    // German-only: French keeps its Refdata-derived value.  (It still shows
    // the issue-#112 double-dose cleanup, which is language-agnostic — what
    // matters is that the Rogger name never reaches DSCRF.)
    assert!(
        with_rogger.contains("<DSCRF>MIRTAZAPINE Sandoz eco 30 mg / 100 comprimé</DSCRF>"),
        "French description must stay the Refdata one:\n{with_rogger}"
    );
    assert_eq!(
        with_rogger.matches(preferred).count(),
        1,
        "Rogger name must appear only in DSCRD (SORTD is uppercased), never in DSCRF:\n{with_rogger}"
    );

    // Without -r (empty map): the issue-#112 cleanup result stands.
    let without = make(rust2xml::rogger_names::RoggerMap::new())
        .build_article()
        .unwrap();
    assert!(
        !without.contains(preferred),
        "Rogger name leaked in without the flag:\n{without}"
    );
    assert!(
        without.contains("<DSCRD>MIRTAZAPIN Sandoz eco 30 mg / 100 Tablette</DSCRD>"),
        "expected the double-dose cleanup result without -r:\n{without}"
    );
}
