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
    "https://epl.bag.admin.ch/fhir-ch-em-epl/PackageBundle.ndjson";

/// Minimal FHIR resource shape — we only deserialize the fields we need.
/// Everything else is ignored so the schema can evolve.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(default)]
pub struct FhirResource {
    #[serde(rename = "resourceType")]
    pub resource_type: String,
    pub id: Option<String>,
    pub identifier: Vec<FhirIdentifier>,
    pub name: Option<FhirName>,
    #[serde(rename = "productClassification")]
    pub product_classification: Vec<FhirCodeableConcept>,
    pub ingredient: Vec<FhirIngredient>,
    #[serde(rename = "packagedMedicinalProduct")]
    pub packaged_medicinal_product: Vec<FhirPackagedMedicinalProduct>,
    pub language: Option<String>,
    pub package: Vec<FhirPackage>,
    pub quantity: Option<FhirQuantity>,
    #[serde(rename = "statusReason")]
    pub status_reason: Option<FhirCodeableConcept>,
    pub substance: Option<FhirCodeableReference>,
    pub strength: Option<FhirRatio>,
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
    #[serde(rename = "countryLanguage")]
    pub country_language: Vec<FhirCountryLanguage>,
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
    pub strength: Vec<FhirStrength>,
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
            .user_agent("rust2xml/oddb2xml-fhir")
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

        for (lineno, line) in self.ndjson.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            let res: FhirResource = serde_json::from_str(line)
                .with_context(|| format!("FHIR NDJSON parse line {}", lineno + 1))?;
            match res.resource_type.as_str() {
                "MedicinalProductDefinition" => {
                    if let Some(id) = res.id.clone() {
                        medicinal.insert(id, res);
                    }
                }
                "PackagedProductDefinition" => packaged.push(res),
                // Ingredients / RegulatedAuthorization could be indexed here
                // when we need them for the builder.
                _ => {}
            }
        }

        for pack in packaged {
            let ean13 = pack
                .package
                .iter()
                .flat_map(|p| p.identifier.iter())
                .filter_map(|id| id.value.clone())
                .find(|v| v.chars().all(|c| c.is_ascii_digit()) && v.len() == 13);

            let ean13 = match ean13 {
                Some(v) => v,
                None => continue,
            };

            let mp_ref = pack
                .packaged_medicinal_product
                .first()
                .and_then(|r| r.reference.clone())
                .unwrap_or_default();
            let mp_id = mp_ref
                .strip_prefix("MedicinalProductDefinition/")
                .unwrap_or(mp_ref.as_str())
                .to_string();
            let mp = medicinal.get(&mp_id);

            let mut item = BagItem::default();
            item.data_origin = "fhir".into();
            item.refdata = true;

            if let Some(mp) = mp {
                if let Some(name) = &mp.name {
                    let mut de = String::new();
                    let mut fr = String::new();
                    let mut it = String::new();
                    for p in &name.part {
                        if let Some(t) = &p.r#type {
                            let code = t
                                .coding
                                .first()
                                .and_then(|c| c.code.clone())
                                .unwrap_or_default();
                            match code.as_str() {
                                "de" => de = p.part.clone(),
                                "fr" => fr = p.part.clone(),
                                "it" => it = p.part.clone(),
                                _ => {}
                            }
                        }
                    }
                    if de.is_empty() {
                        de = name.product_name.clone().unwrap_or_default();
                    }
                    item.name_de = de;
                    item.name_fr = fr;
                    item.name_it = it;
                }
                for classification in &mp.product_classification {
                    for c in &classification.coding {
                        if c.system.as_deref() == Some("http://www.whocc.no/atc") {
                            item.atc_code = c.code.clone().unwrap_or_default();
                        }
                    }
                }
                // Ingredients → substances.
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
            package.prices = BagPrices {
                exf_price: BagPrice::default(),
                pub_price: BagPrice::default(),
            };
            package.limitations = Vec::<BagLimitation>::new();

            item.packages.insert(ean13.clone(), package);
            out.insert(ean13, item);
        }

        Ok(out)
    }
}
