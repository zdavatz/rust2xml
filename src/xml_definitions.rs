//! serde XML bindings mirroring `lib/oddb2xml/xml_definitions.rb`
//! (originally SAX-machine classes).
//!
//! We use `quick-xml`'s `serde` feature to deserialize directly into
//! these structs.  Field names match the PascalCase XML element names
//! exactly — we do not rename via serde attributes.
//!
//! These are read-only input structs.  The builder module generates
//! output XML separately via `quick_xml::Writer`.

#![allow(non_snake_case)]

use serde::Deserialize;

/// String consumed from `Preparations.xml` etc. before it reaches our
/// deserializer — matches `STRIP_FOR_SAX_MACHINE` in the Ruby version.
pub const STRIP_FOR_SAX_MACHINE: &str = "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n";

// ------------------------------------------------------------------
// BAG Preparations.xml hierarchy
// ------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PriceElement {
    #[serde(rename = "Price", default)]
    pub Price: Option<String>,
    #[serde(rename = "ValidFromDate", default)]
    pub ValidFromDate: Option<String>,
    #[serde(rename = "DivisionDescription", default)]
    pub DivisionDescription: Option<String>,
    #[serde(rename = "PriceTypeCode", default)]
    pub PriceTypeCode: Option<String>,
    #[serde(rename = "PriceTypeDescriptionDe", default)]
    pub PriceTypeDescriptionDe: Option<String>,
    #[serde(rename = "PriceTypeDescriptionFr", default)]
    pub PriceTypeDescriptionFr: Option<String>,
    #[serde(rename = "PriceTypeDescriptionIt", default)]
    pub PriceTypeDescriptionIt: Option<String>,
    #[serde(rename = "PriceChangeTypeDescriptionDe", default)]
    pub PriceChangeTypeDescriptionDe: Option<String>,
    #[serde(rename = "PriceChangeTypeDescriptionFr", default)]
    pub PriceChangeTypeDescriptionFr: Option<String>,
    #[serde(rename = "PriceChangeTypeDescriptionIt", default)]
    pub PriceChangeTypeDescriptionIt: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct StatusElement {
    #[serde(rename = "IntegrationDate", default)]
    pub IntegrationDate: Option<String>,
    #[serde(rename = "ValidFromDate", default)]
    pub ValidFromDate: Option<String>,
    #[serde(rename = "ValidThruDate", default)]
    pub ValidThruDate: Option<String>,
    #[serde(rename = "StatusTypeCodeSl", default)]
    pub StatusTypeCodeSl: Option<String>,
    #[serde(rename = "StatusTypeDescriptionSl", default)]
    pub StatusTypeDescriptionSl: Option<String>,
    #[serde(rename = "FlagApd", default)]
    pub FlagApd: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PricesElement {
    #[serde(rename = "ExFactoryPrice", default)]
    pub ExFactoryPrice: Option<PriceElement>,
    #[serde(rename = "PublicPrice", default)]
    pub PublicPrice: Option<PriceElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct LimitationElement {
    #[serde(rename = "LimitationCode", default)]
    pub LimitationCode: Option<String>,
    #[serde(rename = "LimitationType", default)]
    pub LimitationType: Option<String>,
    #[serde(rename = "LimitationValue", default)]
    pub LimitationValue: Option<String>,
    #[serde(rename = "LimitationNiveau", default)]
    pub LimitationNiveau: Option<String>,
    #[serde(rename = "DescriptionDe", default)]
    pub DescriptionDe: Option<String>,
    #[serde(rename = "DescriptionFr", default)]
    pub DescriptionFr: Option<String>,
    #[serde(rename = "DescriptionIt", default)]
    pub DescriptionIt: Option<String>,
    #[serde(rename = "ValidFromDate", default)]
    pub ValidFromDate: Option<String>,
    #[serde(rename = "ValidThruDate", default)]
    pub ValidThruDate: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct LimitationsElement {
    #[serde(rename = "Limitation", default)]
    pub Limitation: Vec<LimitationElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PointLimitationElement {
    #[serde(rename = "Points", default)]
    pub Points: Option<String>,
    #[serde(rename = "Packs", default)]
    pub Packs: Option<String>,
    #[serde(rename = "ValidFromDate", default)]
    pub ValidFromDate: Option<String>,
    #[serde(rename = "ValidThruDate", default)]
    pub ValidThruDate: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PointLimitationsElement {
    #[serde(rename = "PointLimitation", default)]
    pub PointLimitation: Vec<PointLimitationElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PackContent {
    #[serde(rename = "@ProductKey", default)]
    pub ProductKey: Option<String>,
    #[serde(rename = "@PackId", default)]
    pub PackId: Option<String>,
    #[serde(rename = "DescriptionDe", default)]
    pub DescriptionDe: Option<String>,
    #[serde(rename = "DescriptionFr", default)]
    pub DescriptionFr: Option<String>,
    #[serde(rename = "DescriptionIt", default)]
    pub DescriptionIt: Option<String>,
    #[serde(rename = "SwissmedicCategory", default)]
    pub SwissmedicCategory: Option<String>,
    #[serde(rename = "SwissmedicNo8", default)]
    pub SwissmedicNo8: Option<String>,
    #[serde(rename = "FlagNarcosis", default)]
    pub FlagNarcosis: Option<String>,
    #[serde(rename = "FlagModal", default)]
    pub FlagModal: Option<String>,
    #[serde(rename = "BagDossierNo", default)]
    pub BagDossierNo: Option<String>,
    #[serde(rename = "GTIN", default)]
    pub GTIN: Option<String>,
    #[serde(rename = "Limitations", default)]
    pub Limitations: Option<LimitationsElement>,
    #[serde(rename = "PointLimitations", default)]
    pub PointLimitations: Option<PointLimitationsElement>,
    #[serde(rename = "Prices", default)]
    pub Prices: Option<PricesElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PacksElement {
    #[serde(rename = "Pack", default)]
    pub Pack: Vec<PackContent>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ItCodeContent {
    #[serde(rename = "@Code", default)]
    pub Code: Option<String>,
    #[serde(rename = "DescriptionDe", default)]
    pub DescriptionDe: Option<String>,
    #[serde(rename = "DescriptionFr", default)]
    pub DescriptionFr: Option<String>,
    #[serde(rename = "DescriptionIt", default)]
    pub DescriptionIt: Option<String>,
    #[serde(rename = "Limitations", default)]
    pub Limitations: Option<LimitationsElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ItCodesElement {
    #[serde(rename = "ItCode", default)]
    pub ItCode: Vec<ItCodeContent>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SubstanceElement {
    #[serde(rename = "DescriptionLa", default)]
    pub DescriptionLa: Option<String>,
    #[serde(rename = "Quantity", default)]
    pub Quantity: Option<String>,
    #[serde(rename = "QuantityUnit", default)]
    pub QuantityUnit: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SubstancesElement {
    #[serde(rename = "Substance", default)]
    pub Substance: Vec<SubstanceElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PreparationContent {
    #[serde(rename = "@ProductCommercial", default)]
    pub ProductCommercial: Option<String>,
    #[serde(rename = "NameFr", default)]
    pub NameFr: Option<String>,
    #[serde(rename = "NameDe", default)]
    pub NameDe: Option<String>,
    #[serde(rename = "NameIt", default)]
    pub NameIt: Option<String>,
    #[serde(rename = "Status", default)]
    pub Status: Option<StatusElement>,
    #[serde(rename = "DescriptionDe", default)]
    pub DescriptionDe: Option<String>,
    #[serde(rename = "DescriptionFr", default)]
    pub DescriptionFr: Option<String>,
    #[serde(rename = "DescriptionIt", default)]
    pub DescriptionIt: Option<String>,
    #[serde(rename = "AtcCode", default)]
    pub AtcCode: Option<String>,
    #[serde(rename = "SwissmedicNo5", default)]
    pub SwissmedicNo5: Option<String>,
    #[serde(rename = "FlagItLimitation", default)]
    pub FlagItLimitation: Option<String>,
    #[serde(rename = "OrgGenCode", default)]
    pub OrgGenCode: Option<String>,
    #[serde(rename = "FlagSB", default)]
    pub FlagSB: Option<String>,
    #[serde(rename = "FlagSB20", default)]
    pub FlagSB20: Option<String>,
    #[serde(rename = "CommentDe", default)]
    pub CommentDe: Option<String>,
    #[serde(rename = "CommentFr", default)]
    pub CommentFr: Option<String>,
    #[serde(rename = "CommentIt", default)]
    pub CommentIt: Option<String>,
    #[serde(rename = "VatInEXF", default)]
    pub VatInEXF: Option<String>,
    #[serde(rename = "Limitations", default)]
    pub Limitations: Option<LimitationsElement>,
    #[serde(rename = "Substances", default)]
    pub Substances: Option<SubstancesElement>,
    #[serde(rename = "Packs", default)]
    pub Packs: Option<PacksElement>,
    #[serde(rename = "ItCodes", default)]
    pub ItCodes: Option<ItCodesElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PreparationsContent {
    #[serde(rename = "@ReleaseDate", default)]
    pub ReleaseDate: Option<String>,
    #[serde(rename = "Preparation", default)]
    pub Preparation: Vec<PreparationContent>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PreparationsEntry {
    #[serde(rename = "Preparations", default)]
    pub Preparations: PreparationsContent,
}

// ------------------------------------------------------------------
// swissINDEX Pharma / NonPharma feed (legacy)
// ------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct CompElement {
    #[serde(rename = "NAME", default)]
    pub NAME: Option<String>,
    #[serde(rename = "GLN", default)]
    pub GLN: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct ItemContent {
    #[serde(rename = "@DT", default)]
    pub DT: Option<String>,
    #[serde(rename = "GTIN", default)]
    pub GTIN: Option<String>,
    #[serde(rename = "PHAR", default)]
    pub PHAR: Option<String>,
    #[serde(rename = "STATUS", default)]
    pub STATUS: Option<String>,
    #[serde(rename = "SDATE", default)]
    pub SDATE: Option<String>,
    #[serde(rename = "STDATE", default)]
    pub STDATE: Option<String>,
    #[serde(rename = "LANG", default)]
    pub LANG: Option<String>,
    #[serde(rename = "DSCR", default)]
    pub DSCR: Option<String>,
    #[serde(rename = "ADDSCR", default)]
    pub ADDSCR: Option<String>,
    #[serde(rename = "ATC", default)]
    pub ATC: Option<String>,
    #[serde(rename = "COMP", default)]
    pub COMP: Option<CompElement>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct PharmaContent {
    #[serde(rename = "@CREATION_DATETIME", default)]
    pub CREATION_DATETIME: Option<String>,
    #[serde(rename = "ITEM", default)]
    pub ITEM: Vec<ItemContent>,
}

// ------------------------------------------------------------------
// Swissmedic-Info AipsDownload / medicalInformation feed
// ------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct MedicalInformationContent {
    #[serde(rename = "@type", default)]
    pub r#type: Option<String>,
    #[serde(rename = "@version", default)]
    pub version: Option<String>,
    #[serde(rename = "@lang", default)]
    pub lang: Option<String>,
    #[serde(rename = "title", default)]
    pub title: Option<String>,
    #[serde(rename = "authHolder", default)]
    pub authHolder: Option<String>,
    #[serde(rename = "authNrs", default)]
    pub authNrs: Option<String>,
    #[serde(rename = "style", default)]
    pub style: Option<String>,
    #[serde(rename = "content", default)]
    pub content: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct MedicalInformationsContent {
    #[serde(rename = "medicalInformation", default)]
    pub medicalInformation: Vec<MedicalInformationContent>,
}

// ------------------------------------------------------------------
// Refdata / SwissReg Articles feed
// ------------------------------------------------------------------

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegProductClassification {
    #[serde(rename = "ProductClass", default)]
    pub ProductClass: Option<String>,
    #[serde(rename = "Atc", default)]
    pub Atc: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegMedicinalProduct {
    #[serde(rename = "Identifier", default)]
    pub Identifier: Option<String>,
    #[serde(rename = "Domain", default)]
    pub Domain: Option<String>,
    #[serde(rename = "LegalStatusOfSupply", default)]
    pub LegalStatusOfSupply: Option<String>,
    #[serde(rename = "RegulatedAuthorisationIdentifier", default)]
    pub RegulatedAuthorisationIdentifier: Option<String>,
    #[serde(rename = "ProductClassification", default)]
    pub ProductClassification: Option<SwissRegProductClassification>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegHolder {
    #[serde(rename = "Identifier", default)]
    pub Identifier: Option<String>,
    #[serde(rename = "Name", default)]
    pub Name: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegName {
    #[serde(rename = "Language", default)]
    pub Language: Option<String>,
    #[serde(rename = "FullName", default)]
    pub FullName: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegMarketingStatus {
    #[serde(rename = "DateStart", default)]
    pub DateStart: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegPackagedProduct {
    #[serde(rename = "Identifier", default)]
    pub Identifier: Option<String>,
    #[serde(rename = "RegulatedAuthorisationIdentifier", default)]
    pub RegulatedAuthorisationIdentifier: Option<String>,
    #[serde(rename = "DataCarrierIdentifier", default)]
    pub DataCarrierIdentifier: Option<String>,
    #[serde(rename = "Holder", default)]
    pub Holder: Option<SwissRegHolder>,
    #[serde(rename = "Name", default)]
    pub Name: Vec<SwissRegName>,
    #[serde(rename = "MarketingStatus", default)]
    pub MarketingStatus: Option<SwissRegMarketingStatus>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegArticle {
    #[serde(rename = "MedicinalProduct", default)]
    pub MedicinalProduct: Option<SwissRegMedicinalProduct>,
    #[serde(rename = "PackagedProduct", default)]
    pub PackagedProduct: Option<SwissRegPackagedProduct>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegDocumentReference {
    #[serde(rename = "Language", default)]
    pub Language: Option<String>,
    #[serde(rename = "Url", default)]
    pub Url: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegProductPrice {
    #[serde(rename = "RetailPrice", default)]
    pub RetailPrice: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub struct SwissRegArticles {
    #[serde(rename = "Article", default)]
    pub Article: Vec<SwissRegArticle>,
    #[serde(rename = "DocumentReference", default)]
    pub DocumentReference: Vec<SwissRegDocumentReference>,
    #[serde(rename = "Hpc", default)]
    pub Hpc: Option<String>,
    #[serde(rename = "ProductPrice", default)]
    pub ProductPrice: Option<SwissRegProductPrice>,
}
