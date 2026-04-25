//! FHIR NDJSON support — port of `lib/oddb2xml/fhir_support.rb`.
//!
//! Consumes FOPH/BAG's FHIR bundle at
//! <https://epl.bag.admin.ch/fhir-ch-em-epl/PackageBundle.ndjson> (or a
//! language-specific variant) and normalizes MedicinalProductDefinition,
//! PackagedProductDefinition, RegulatedAuthorization and Ingredient
//! resources into the same item shape the builder already expects from
//! `BagXmlExtractor`.

use crate::extractor::{BagItem, BagLimitation, BagPackage, BagPrice, BagPrices, BagSubstance};
use crate::util;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashMap;

pub const DEFAULT_FHIR_URL: &str =
    "https://epl.bag.admin.ch/static/fhir/foph-sl-export-latest-de.ndjson";

/// Minimal FHIR resource shape — we only deserialize the fields we need.
/// Everything else is ignored so the schema can evolve.  This struct
/// covers both the outer `Bundle` (which carries `entry[]`) and the
/// per-entry resources (`MedicinalProductDefinition`,
/// `PackagedProductDefinition`, `RegulatedAuthorization`, …).
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirResource {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub id: Option<String>,
    pub identifier: Vec<FhirIdentifier>,
    pub name: Vec<FhirName>,
    pub classification: Vec<FhirCodeableConcept>,
    #[serde(rename = "productClassification")]
    pub product_classification: Vec<FhirCodeableConcept>,
    pub ingredient: Vec<FhirIngredient>,
    #[serde(rename = "packagedMedicinalProduct")]
    pub packaged_medicinal_product: Vec<FhirPackagedMedicinalProduct>,
    #[serde(rename = "packageFor")]
    pub package_for: Vec<FhirReference>,
    pub language: Option<String>,
    pub package: Vec<FhirPackage>,
    pub packaging: Option<FhirPackaging>,
    pub quantity: Option<FhirQuantity>,
    #[serde(rename = "statusReason")]
    pub status_reason: Option<FhirCodeableConcept>,
    pub substance: Option<FhirCodeableReference>,
    pub strength: Option<FhirRatio>,
    pub subject: Vec<FhirReference>,
    #[serde(rename = "for")]
    pub for_ref: Vec<FhirReference>,
    pub extension: Vec<FhirExtension>,
    /// Used by RegulatedAuthorization to carry limitations under
    /// `indication[].extension[]` in the new FOPH FHIR feed.
    pub indication: Vec<FhirIndication>,
    pub entry: Vec<FhirBundleEntry>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirIndication {
    pub extension: Vec<FhirExtension>,
}

/// One slot in a Bundle's `entry[]` list — wraps an inner resource.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirBundleEntry {
    #[serde(rename = "fullUrl")]
    pub full_url: Option<String>,
    pub resource: FhirResource,
}

/// FHIR Extension — recursive (extensions can carry sub-extensions).
/// Only the value-types we actually consume are deserialized; the rest
/// is ignored via `#[serde(default)]`.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirExtension {
    pub url: String,
    #[serde(rename = "valueString")]
    pub value_string: Option<String>,
    #[serde(rename = "valueDate")]
    pub value_date: Option<String>,
    #[serde(rename = "valueInteger")]
    pub value_integer: Option<i64>,
    #[serde(rename = "valueDecimal")]
    pub value_decimal: Option<f64>,
    #[serde(rename = "valueBoolean")]
    pub value_boolean: Option<bool>,
    #[serde(rename = "valueCodeableConcept")]
    pub value_codeable_concept: Option<FhirCodeableConcept>,
    #[serde(rename = "valueIdentifier")]
    pub value_identifier: Option<FhirIdentifier>,
    #[serde(rename = "valuePeriod")]
    pub value_period: Option<FhirPeriod>,
    #[serde(rename = "valueMoney")]
    pub value_money: Option<FhirMoney>,
    pub extension: Vec<FhirExtension>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirPeriod {
    pub start: Option<String>,
    pub end: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirMoney {
    pub value: Option<f64>,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirPackaging {
    pub identifier: Vec<FhirIdentifier>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirIdentifier {
    pub system: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirName {
    #[serde(rename = "productName")]
    pub product_name: Option<String>,
    pub part: Vec<FhirNamePart>,
    pub usage: Vec<FhirUsage>,
    #[serde(rename = "countryLanguage")]
    pub country_language: Vec<FhirCountryLanguage>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirUsage {
    pub country: Option<FhirCodeableConcept>,
    pub language: Option<FhirCodeableConcept>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirNamePart {
    pub part: String,
    pub r#type: Option<FhirCodeableConcept>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirCountryLanguage {
    pub language: Option<FhirCodeableConcept>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirCodeableConcept {
    pub coding: Vec<FhirCoding>,
    pub text: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirCoding {
    pub system: Option<String>,
    pub code: Option<String>,
    pub display: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirIngredient {
    pub substance: Option<FhirSubstanceRef>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirSubstanceRef {
    #[serde(rename = "codeableConcept")]
    pub codeable_concept: Option<FhirCodeableConcept>,
    /// New feed wraps the codeable concept under `code.concept`.
    pub code: Option<FhirSubstanceCode>,
    pub concept: Option<FhirCodeableConcept>,
    pub strength: Vec<FhirStrength>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirSubstanceCode {
    pub concept: Option<FhirCodeableConcept>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirStrength {
    #[serde(rename = "presentationQuantity")]
    pub presentation_quantity: Option<FhirQuantity>,
    #[serde(rename = "presentationRatio")]
    pub presentation_ratio: Option<FhirRatio>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirQuantity {
    pub value: Option<f64>,
    pub unit: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirRatio {
    pub numerator: Option<FhirQuantity>,
    pub denominator: Option<FhirQuantity>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirPackagedMedicinalProduct {
    pub reference: Option<String>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirPackage {
    pub identifier: Vec<FhirIdentifier>,
    pub quantity: Option<FhirQuantity>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirCodeableReference {
    pub concept: Option<FhirCodeableConcept>,
    pub reference: Option<FhirReference>,
    /// New-feed Ingredient resources put the substance text under
    /// `substance.code.concept`.
    pub code: Option<FhirSubstanceCode>,
    /// Same Ingredient also carries a strength array.
    pub strength: Vec<FhirStrength>,
    /// Old-feed compatibility — explicit codeable concept under
    /// `substance.codeableConcept`.
    #[serde(rename = "codeableConcept")]
    pub codeable_concept: Option<FhirCodeableConcept>,
}

#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirReference {
    pub reference: Option<String>,
    pub display: Option<String>,
}

// -----------------------------------------------------------------
// Downloader
// -----------------------------------------------------------------

pub struct FhirDownloader {
    pub url: String,
    pub client: reqwest::blocking::Client,
}

impl FhirDownloader {
    pub fn new(url: impl Into<String>) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("rust2xml-fhir")
            .timeout(std::time::Duration::from_secs(600))
            .build()?;
        Ok(Self { url: url.into(), client })
    }

    pub fn download(&self) -> Result<String> {
        util::log(format!("FhirDownloader GET {}", self.url));
        let path = util::work_dir().join("fhir_package_bundle.ndjson");
        let bytes = crate::downloader::download_as(&self.client, &self.url, &path)?;
        Ok(String::from_utf8_lossy(&bytes).into_owned())
    }
}

// -----------------------------------------------------------------
// Extractor — NDJSON → BagItem map keyed by EAN-13.
// -----------------------------------------------------------------

/// FHIR extractor.  The implementation deliberately mirrors the *shape*
/// of `BagXmlExtractor::to_hash` so that the rest of the pipeline
/// (builder, compare, semantic_check) can accept either feed without
/// branching.
pub struct FhirExtractor {
    pub ndjson: String,
}

impl FhirExtractor {
    pub fn new(ndjson: impl Into<String>) -> Self {
        Self { ndjson: ndjson.into() }
    }

    pub fn to_hash(&self) -> Result<HashMap<String, BagItem>> {
        let mut out: HashMap<String, BagItem> = HashMap::new();

        let mut medicinal: HashMap<String, FhirResource> = HashMap::new();
        let mut packaged: Vec<FhirResource> = Vec::new();
        // Ingredients keyed by their MPD reference (substance per product).
        let mut ingredients_by_mpd: HashMap<String, Vec<FhirResource>> = HashMap::new();
        // Per-package prices + limitations parsed out of
        // RegulatedAuthorization.  Keyed by PackagedProductDefinition id.
        let mut sl_data: HashMap<String, (BagPrices, Vec<BagLimitation>)> = HashMap::new();

        for (lineno, line) in self.ndjson.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let bundle: FhirResource = serde_json::from_str(line)
                .with_context(|| format!("FHIR NDJSON parse line {}", lineno + 1))?;

            // Each NDJSON line is a Bundle whose .entry[] holds the
            // actual resources we care about.  Older format had one
            // resource per line — keep that path as a fallback.
            let resources: Vec<&FhirResource> = if bundle.resource_type == "Bundle" {
                bundle.entry.iter().map(|e| &e.resource).collect()
            } else {
                vec![&bundle]
            };

            for res in &resources {
                match res.resource_type.as_str() {
                    "MedicinalProductDefinition" => {
                        if let Some(id) = res.id.clone() {
                            medicinal.insert(id, (*res).clone());
                        }
                    }
                    "PackagedProductDefinition" => packaged.push((*res).clone()),
                    "Ingredient" => {
                        // `for[].reference` (CHIDMPMedicinalProductDefinition/<id>)
                        // tells us which product this ingredient belongs to.
                        if let Some(mpd_ref) = res
                            .for_ref
                            .first()
                            .and_then(|r| r.reference.clone())
                        {
                            let mp_id = mpd_ref
                                .rsplit('/')
                                .next()
                                .unwrap_or(mpd_ref.as_str())
                                .to_string();
                            ingredients_by_mpd.entry(mp_id).or_default().push((*res).clone());
                        }
                    }
                    "RegulatedAuthorization" => {
                        // SL pricing + limitation extensions live on the
                        // package-targeting RegulatedAuthorization
                        // (subject = CHIDMPPackagedProductDefinition/<id>):
                        // prices in `extension[reimbursementSL].extension[productPrice]`,
                        // limitations under `indication[].extension[]`.
                        let target_pack = res
                            .subject
                            .iter()
                            .filter_map(|s| s.reference.as_deref())
                            .find_map(|r| {
                                if r.contains("PackagedProductDefinition") {
                                    r.rsplit('/').next().map(|s| s.to_string())
                                } else {
                                    None
                                }
                            });
                        if let Some(pack_id) = target_pack {
                            let mut all_extensions = res.extension.clone();
                            for ind in &res.indication {
                                all_extensions.extend(ind.extension.iter().cloned());
                            }
                            let (prices, limitations) = extract_sl_data(&all_extensions);
                            if has_any_price(&prices) || !limitations.is_empty() {
                                let entry = sl_data
                                    .entry(pack_id)
                                    .or_insert_with(|| (BagPrices::default(), Vec::new()));
                                if !prices.exf_price.price.is_empty() {
                                    entry.0.exf_price = prices.exf_price;
                                }
                                if !prices.pub_price.price.is_empty() {
                                    entry.0.pub_price = prices.pub_price;
                                }
                                entry.1.extend(limitations);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }

        for pack in packaged {
            // New FHIR feed: GTIN lives in `packaging.identifier`.
            // Older feed used `package[].identifier`.  Try both.
            let ean13 = pack
                .packaging
                .as_ref()
                .map(|p| p.identifier.as_slice())
                .unwrap_or(&[])
                .iter()
                .chain(pack.package.iter().flat_map(|p| p.identifier.iter()))
                .filter_map(|id| id.value.clone())
                .find(|v| v.chars().all(|c| c.is_ascii_digit()) && v.len() == 13);

            let ean13 = match ean13 {
                Some(v) => v,
                None => continue,
            };

            // New format references the MPD via `packageFor`; old via
            // `packagedMedicinalProduct`.  Strip whichever prefix the
            // server used (`CHIDMP…/`, `MedicinalProductDefinition/`).
            let mp_ref = pack
                .package_for
                .first()
                .and_then(|r| r.reference.clone())
                .or_else(|| {
                    pack.packaged_medicinal_product
                        .first()
                        .and_then(|r| r.reference.clone())
                })
                .unwrap_or_default();
            let mp_id = mp_ref
                .rsplit('/')
                .next()
                .unwrap_or(mp_ref.as_str())
                .to_string();
            let mp = medicinal.get(&mp_id);

            let mut item = BagItem::default();
            item.data_origin = "fhir".into();
            item.refdata = true;

            if let Some(mp) = mp {
                // New feed: `name` is an array of {productName, usage[].language},
                // one item per language.  Old feed: single object with parts.
                let mut de = String::new();
                let mut fr = String::new();
                let mut it = String::new();
                for n in &mp.name {
                    let pn = n.product_name.clone().unwrap_or_default();
                    let lang = n
                        .usage
                        .first()
                        .and_then(|u| u.language.as_ref())
                        .and_then(|cc| cc.coding.first())
                        .and_then(|c| c.code.clone())
                        .unwrap_or_default();
                    match lang.split('-').next().unwrap_or("") {
                        "de" => de = pn.clone(),
                        "fr" => fr = pn.clone(),
                        "it" => it = pn.clone(),
                        _ => {
                            if de.is_empty() {
                                de = pn.clone();
                            }
                        }
                    }
                    // Old-format fallback via `name.part`.
                    for p in &n.part {
                        let code = p
                            .r#type
                            .as_ref()
                            .and_then(|t| t.coding.first())
                            .and_then(|c| c.code.clone())
                            .unwrap_or_default();
                        match code.as_str() {
                            "de" if de.is_empty() => de = p.part.clone(),
                            "fr" if fr.is_empty() => fr = p.part.clone(),
                            "it" if it.is_empty() => it = p.part.clone(),
                            _ => {}
                        }
                    }
                }
                item.name_de = de;
                item.name_fr = fr;
                item.name_it = it;

                // ATC + IT classification.  New feed puts both in
                // `classification[]`; old feed used `productClassification`.
                for classification in mp.classification.iter().chain(mp.product_classification.iter()) {
                    for c in &classification.coding {
                        if c.system.as_deref() == Some("http://www.whocc.no/atc") {
                            item.atc_code = c.code.clone().unwrap_or_default();
                        }
                        if c.system.as_deref()
                            == Some(
                                "http://fhir.ch/ig/ch-epl/CodeSystem/ch-epl-foph-index-therapeuticus",
                            )
                        {
                            // Pick the most-specific (longest) IT code we see.
                            if let Some(code) = &c.code {
                                if code.len() > item.it_code.len() {
                                    item.it_code = code.clone();
                                }
                            }
                        }
                    }
                }
                // Old feed: substances embedded in MPD.ingredient.
                for (idx, ing) in mp.ingredient.iter().enumerate() {
                    if let Some(sub) = &ing.substance {
                        if let Some(cc) = &sub.codeable_concept {
                            let name = cc
                                .text
                                .clone()
                                .or_else(|| cc.coding.first().and_then(|c| c.display.clone()))
                                .unwrap_or_default();
                            let (qty, unit) = sub
                                .strength
                                .first()
                                .and_then(|s| s.presentation_quantity.clone())
                                .map(|q| (q.value.map(|v| v.to_string()).unwrap_or_default(), q.unit.unwrap_or_default()))
                                .unwrap_or_default();
                            item.substances.push(BagSubstance {
                                index: idx.to_string(),
                                name,
                                quantity: qty,
                                unit,
                            });
                        }
                    }
                }
                // New feed: substances are stand-alone Ingredient resources
                // in the same Bundle; we indexed them by MPD reference above.
                if let Some(ings) = ingredients_by_mpd.get(&mp_id) {
                    for (idx, ing_res) in ings.iter().enumerate() {
                        let sub = match &ing_res.substance {
                            Some(s) => s,
                            None => continue,
                        };
                        let cc = sub
                            .codeable_concept
                            .as_ref()
                            .or(sub.concept.as_ref())
                            .or_else(|| sub.code.as_ref().and_then(|c| c.concept.as_ref()));
                        if let Some(cc) = cc {
                            let name = cc
                                .text
                                .clone()
                                .or_else(|| cc.coding.first().and_then(|c| c.display.clone()))
                                .unwrap_or_default();
                            let (qty, unit) = sub
                                .strength
                                .first()
                                .and_then(|s| s.presentation_quantity.clone())
                                .map(|q: FhirQuantity| {
                                    (
                                        q.value.map(|v| v.to_string()).unwrap_or_default(),
                                        q.unit.unwrap_or_default(),
                                    )
                                })
                                .unwrap_or_default();
                            item.substances.push(BagSubstance {
                                index: (mp.ingredient.len() + idx).to_string(),
                                name,
                                quantity: qty,
                                unit,
                            });
                        }
                    }
                }
                item.swissmedic_number5 = mp
                    .identifier
                    .iter()
                    .find(|id| id.system.as_deref() == Some("urn:oid:2.51.1.1"))
                    .and_then(|id| id.value.clone())
                    .unwrap_or_default();
            }

            let no8 = pack
                .identifier
                .iter()
                .find(|id| id.system.as_deref() == Some("urn:oid:2.51.1.1"))
                .and_then(|id| id.value.clone())
                .unwrap_or_default();

            util::set_ean13_for_no8(no8.clone(), ean13.clone());

            let mut package = BagPackage::default();
            package.ean13 = ean13.clone();
            package.name_de = item.name_de.clone();
            package.name_fr = item.name_fr.clone();
            package.name_it = item.name_it.clone();
            package.swissmedic_number8 = no8;
            package.sl_entry = true;
            // Pull prices + limitations from the matching package-level
            // RegulatedAuthorization (extracted in the bundle scan,
            // keyed by PackagedProductDefinition id).
            let pack_id = pack.id.clone().unwrap_or_default();
            if let Some((prices, limitations)) = sl_data.get(&pack_id) {
                package.prices = prices.clone();
                package.limitations = limitations.clone();
            } else {
                package.prices = BagPrices {
                    exf_price: BagPrice::default(),
                    pub_price: BagPrice::default(),
                };
                package.limitations = Vec::<BagLimitation>::new();
            }

            item.packages.insert(ean13.clone(), package);
            out.insert(ean13, item);
        }

        Ok(out)
    }
}

// -----------------------------------------------------------------
// SL-pricing & limitation helpers
// -----------------------------------------------------------------

const PRICE_TYPE_FACTORY: &str = "756002005002"; // Ex-factory
const PRICE_TYPE_RETAIL: &str = "756002005001"; // Retail / Public

fn extract_sl_data(extensions: &[FhirExtension]) -> (BagPrices, Vec<BagLimitation>) {
    let mut prices = BagPrices::default();
    let mut limitations: Vec<BagLimitation> = Vec::new();

    for ext in extensions {
        // Lift any limitation extensions placed directly under the
        // parent (the new feed keeps them under indication[].extension[]).
        match ext.url.as_str() {
            "http://fhir.ch/ig/ch-epl/StructureDefinition/regulatedAuthorization-limitation" => {
                let lim = parse_limitation(&ext.extension);
                if !lim.desc_de.is_empty() || !lim.r#type.is_empty() {
                    limitations.push(lim);
                }
            }
            url if url.ends_with("/reimbursementSL") => {
                for sub in &ext.extension {
                    match sub.url.as_str() {
                        "http://fhir.ch/ig/ch-epl/StructureDefinition/productPrice" => {
                            let (code, display, value, date) = parse_price(&sub.extension);
                            let bag_price = BagPrice {
                                price: value,
                                valid_date: date,
                                price_code: if !display.is_empty() {
                                    display
                                } else {
                                    code.clone()
                                },
                            };
                            if code == PRICE_TYPE_FACTORY {
                                prices.exf_price = bag_price;
                            } else if code == PRICE_TYPE_RETAIL {
                                prices.pub_price = bag_price;
                            }
                        }
                        // Some servers put the limitation inside reimbursementSL
                        // — handle that path too.
                        "http://fhir.ch/ig/ch-epl/StructureDefinition/regulatedAuthorization-limitation" => {
                            let lim = parse_limitation(&sub.extension);
                            if !lim.desc_de.is_empty() || !lim.r#type.is_empty() {
                                limitations.push(lim);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
    (prices, limitations)
}

fn has_any_price(p: &BagPrices) -> bool {
    !p.exf_price.price.is_empty() || !p.pub_price.price.is_empty()
}

fn parse_price(extensions: &[FhirExtension]) -> (String, String, String, String) {
    let mut code = String::new();
    let mut display = String::new();
    let mut value = String::new();
    let mut date = String::new();
    for e in extensions {
        match e.url.as_str() {
            "type" => {
                if let Some(cc) = &e.value_codeable_concept {
                    if let Some(c) = cc.coding.first() {
                        code = c.code.clone().unwrap_or_default();
                        display = c.display.clone().unwrap_or_default();
                    }
                }
            }
            "value" => {
                if let Some(m) = &e.value_money {
                    if let Some(v) = m.value {
                        value = format!("{v:.2}");
                    }
                }
            }
            "changeDate" => {
                date = e.value_date.clone().unwrap_or_default();
            }
            _ => {}
        }
    }
    (code, display, value, date)
}

fn parse_limitation(extensions: &[FhirExtension]) -> BagLimitation {
    let mut lim = BagLimitation::default();
    for e in extensions {
        match e.url.as_str() {
            "limitationText" => {
                lim.desc_de = e.value_string.clone().unwrap_or_default();
            }
            "statusDate" => {
                lim.vdate = e.value_date.clone().unwrap_or_default();
            }
            "status" => {
                if let Some(cc) = &e.value_codeable_concept {
                    if let Some(c) = cc.coding.first() {
                        lim.r#type = c
                            .display
                            .clone()
                            .or_else(|| c.code.clone())
                            .unwrap_or_default();
                    }
                }
            }
            _ => {}
        }
    }
    lim
}
