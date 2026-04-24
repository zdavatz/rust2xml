//! Source-format → normalized-record adapters.  Port of
//! `lib/oddb2xml/extractor.rb`.
//!
//! Each extractor ingests a raw byte/string/Path and emits either a
//! `HashMap<Key, Item>` or a `Vec<Item>` where `Key` is typically the
//! EAN-13 GTIN or swissmedic_no8.

use crate::util;
use crate::xml_definitions::{
    MedicalInformationsContent, PreparationsContent, SwissRegArticles, STRIP_FOR_SAX_MACHINE,
};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Normalized pack/price record produced by `BagXmlExtractor::to_hash`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BagPackage {
    pub ean13: String,
    pub name_de: String,
    pub name_fr: String,
    pub name_it: String,
    pub desc_de: String,
    pub desc_fr: String,
    pub desc_it: String,
    pub sl_entry: bool,
    pub swissmedic_category: String,
    pub swissmedic_number8: String,
    pub prices: BagPrices,
    pub limitations: Vec<BagLimitation>,
    pub limitation_points: String,
    /// When refdata disagrees with `calc_checksum`, the corrected EAN-13.
    pub correct_ean13: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BagPrices {
    pub exf_price: BagPrice,
    pub pub_price: BagPrice,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BagPrice {
    pub price: String,
    pub valid_date: String,
    pub price_code: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BagLimitation {
    pub it: String,
    pub key: String,
    pub id: String,
    pub code: String,
    pub r#type: String,
    pub value: String,
    pub niv: String,
    pub desc_de: String,
    pub desc_fr: String,
    pub desc_it: String,
    pub vdate: String,
    pub del: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BagSubstance {
    pub index: String,
    pub name: String,
    pub quantity: String,
    pub unit: String,
}

/// Normalized preparation produced by `BagXmlExtractor::to_hash`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BagItem {
    pub data_origin: String,
    pub refdata: bool,
    pub product_key: String,
    pub desc_de: String,
    pub desc_fr: String,
    pub desc_it: String,
    pub name_de: String,
    pub name_fr: String,
    pub name_it: String,
    pub swissmedic_number5: String,
    pub org_gen_code: String,
    pub deductible: String,
    pub deductible20: String,
    pub atc_code: String,
    pub comment_de: String,
    pub comment_fr: String,
    pub comment_it: String,
    pub it_code: String,
    pub substances: Vec<BagSubstance>,
    pub pharmacodes: Vec<String>,
    pub packages: HashMap<String, BagPackage>,
}

// ------------------------------------------------------------------

pub struct BagXmlExtractor {
    pub xml: String,
}

impl BagXmlExtractor {
    pub fn new(xml: impl Into<String>) -> Self {
        Self { xml: xml.into() }
    }

    /// Parse a Preparations.xml payload into a map keyed by EAN-13.
    pub fn to_hash(&self) -> Result<HashMap<String, BagItem>> {
        let stripped = strip_xml_header(&self.xml);
        // Strip namespaces defensively — quick-xml's serde deserializer
        // is namespace-ignoring but some callers pass the raw BAG file
        // with prefixed elements.
        let stripped = strip_default_namespace(&stripped);
        let doc: PreparationsContent = quick_xml::de::from_str(&stripped)
            .map_err(|e| anyhow::anyhow!("BAG XML deserialize failed: {e}"))?;

        let mut out: HashMap<String, BagItem> = HashMap::new();

        for seq in &doc.Preparation {
            if seq.SwissmedicNo5.as_deref() == Some("0") {
                continue;
            }
            let mut item = BagItem::default();
            item.data_origin = "bag_xml".into();
            item.refdata = true;
            item.product_key = seq.ProductCommercial.clone().unwrap_or_default();
            item.desc_de = seq.DescriptionDe.clone().unwrap_or_default();
            item.desc_fr = seq.DescriptionFr.clone().unwrap_or_default();
            item.desc_it = seq.DescriptionIt.clone().unwrap_or_default();
            item.name_de = seq.NameDe.clone().unwrap_or_default();
            item.name_fr = seq.NameFr.clone().unwrap_or_default();
            item.name_it = seq.NameIt.clone().unwrap_or_default();
            item.swissmedic_number5 = seq
                .SwissmedicNo5
                .clone()
                .map(|s| pad_left(&s, 5, '0'))
                .unwrap_or_default();
            item.org_gen_code = seq.OrgGenCode.clone().unwrap_or_default();
            item.deductible = seq.FlagSB.clone().unwrap_or_default();
            item.deductible20 = seq.FlagSB20.clone().unwrap_or_default();
            item.atc_code = seq.AtcCode.clone().unwrap_or_default();
            item.comment_de = seq.CommentDe.clone().unwrap_or_default();
            item.comment_fr = seq.CommentFr.clone().unwrap_or_default();
            item.comment_it = seq.CommentIt.clone().unwrap_or_default();

            if let Some(itc) = &seq.ItCodes {
                let itc_regex = regex::Regex::new(r"(\d+)\.(\d+)\.(\d+).").unwrap();
                for it_code in &itc.ItCode {
                    if item.it_code.is_empty() {
                        if let Some(code) = &it_code.Code {
                            if itc_regex.is_match(code) {
                                item.it_code = code.clone();
                            }
                        }
                    }
                }
            }

            if let Some(subs) = &seq.Substances {
                for (i, sub) in subs.Substance.iter().enumerate() {
                    item.substances.push(BagSubstance {
                        index: i.to_string(),
                        name: sub.DescriptionLa.clone().unwrap_or_default(),
                        quantity: sub.Quantity.clone().unwrap_or_default(),
                        unit: sub.QuantityUnit.clone().unwrap_or_default(),
                    });
                }
            }

            let mut pack_ean13s: Vec<String> = Vec::new();
            if let Some(packs) = &seq.Packs {
                for pac in &packs.Pack {
                    let mut gtin = pac.GTIN.clone();
                    let mut no8 = pac.SwissmedicNo8.clone();
                    if let Some(n) = &no8 {
                        if n.len() < 8 {
                            let padded = pad_left(n, 8, '0');
                            no8 = Some(padded);
                        }
                    }
                    if gtin.is_none() {
                        if let Some(n) = &no8 {
                            let ean12 = format!("7680{n}");
                            let cd = util::calc_checksum(&ean12);
                            gtin = Some(format!("{ean12}{cd}"));
                        } else {
                            continue;
                        }
                    }
                    let ean13 = gtin.unwrap_or_default();
                    if let Some(n) = &no8 {
                        util::set_ean13_for_no8(n.clone(), ean13.clone());
                    }

                    let (exf, pubp) = extract_prices(pac.Prices.as_ref());

                    let mut package = BagPackage {
                        ean13: ean13.clone(),
                        name_de: seq.NameDe.clone().unwrap_or_default(),
                        name_fr: seq.NameFr.clone().unwrap_or_default(),
                        name_it: seq.NameIt.clone().unwrap_or_default(),
                        desc_de: pac.DescriptionDe.clone().unwrap_or_default(),
                        desc_fr: pac.DescriptionFr.clone().unwrap_or_default(),
                        desc_it: pac.DescriptionIt.clone().unwrap_or_default(),
                        sl_entry: true,
                        swissmedic_category: pac.SwissmedicCategory.clone().unwrap_or_default(),
                        swissmedic_number8: no8.clone().unwrap_or_default(),
                        prices: BagPrices { exf_price: exf, pub_price: pubp },
                        limitations: Vec::new(),
                        limitation_points: String::new(),
                        correct_ean13: None,
                    };

                    append_limitations(
                        &mut package,
                        seq.Limitations.as_ref(),
                        seq.ItCodes.as_ref(),
                        pac.Limitations.as_ref(),
                        &item.it_code,
                        &item.swissmedic_number5,
                    );

                    if let Some(pl) = pac.PointLimitations.as_ref() {
                        if let Some(first) = pl.PointLimitation.first() {
                            package.limitation_points = first.Points.clone().unwrap_or_default();
                        }
                    }

                    if let Some(n) = &no8 {
                        let ean12 = format!("7680{n}");
                        let correct = format!("{ean12}{}", util::calc_checksum(&ean12));
                        if correct != ean13 {
                            package.correct_ean13 = Some(correct);
                        }
                    }

                    pack_ean13s.push(ean13.clone());
                    item.packages.insert(ean13, package);
                }
            }

            // Ruby semantics: `data[ean13] = item` runs inside the pack
            // loop, so every pack's EAN-13 ends up as its own key pointing
            // at the same (fully populated) item.
            for ean13 in pack_ean13s {
                out.insert(ean13, item.clone());
            }
        }

        Ok(out)
    }
}

fn extract_prices(prices: Option<&crate::xml_definitions::PricesElement>) -> (BagPrice, BagPrice) {
    let mut exf = BagPrice::default();
    let mut pubp = BagPrice::default();
    if let Some(p) = prices {
        if let Some(ex) = &p.ExFactoryPrice {
            exf.price = ex.Price.clone().unwrap_or_default();
            exf.valid_date = ex.ValidFromDate.clone().unwrap_or_default();
            exf.price_code = ex.PriceTypeCode.clone().unwrap_or_default();
        }
        if let Some(pu) = &p.PublicPrice {
            pubp.price = pu.Price.clone().unwrap_or_default();
            pubp.valid_date = pu.ValidFromDate.clone().unwrap_or_default();
            pubp.price_code = pu.PriceTypeCode.clone().unwrap_or_default();
        }
    }
    (exf, pubp)
}

fn append_limitations(
    pkg: &mut BagPackage,
    seq_lims: Option<&crate::xml_definitions::LimitationsElement>,
    seq_itcodes: Option<&crate::xml_definitions::ItCodesElement>,
    pac_lims: Option<&crate::xml_definitions::LimitationsElement>,
    it_code: &str,
    number5: &str,
) {
    use crate::xml_definitions::LimitationElement;
    use chrono::NaiveDate;

    let today = chrono::Local::now().naive_local().date();

    let push = |lims: &[LimitationElement], key: &str, id: &str, out: &mut Vec<BagLimitation>| {
        for lim in lims {
            let mut deleted = false;
            if let Some(thru) = &lim.ValidThruDate {
                if regex::Regex::new(r"\d{2}\.\d{2}\.\d{2}").unwrap().is_match(thru) {
                    if let Ok(d) = NaiveDate::parse_from_str(thru, "%d.%m.%y") {
                        if d >= today {
                            deleted = true;
                        }
                    }
                }
            }
            out.push(BagLimitation {
                it: it_code.to_string(),
                key: key.to_string(),
                id: id.to_string(),
                code: lim.LimitationCode.clone().unwrap_or_default(),
                r#type: lim.LimitationType.clone().unwrap_or_default(),
                value: lim.LimitationValue.clone().unwrap_or_default(),
                niv: lim.LimitationNiveau.clone().unwrap_or_default(),
                desc_de: lim.DescriptionDe.clone().unwrap_or_default(),
                desc_fr: lim.DescriptionFr.clone().unwrap_or_default(),
                desc_it: lim.DescriptionIt.clone().unwrap_or_default(),
                vdate: lim.ValidFromDate.clone().unwrap_or_default(),
                del: deleted,
            });
        }
    };

    if let Some(ls) = seq_lims {
        push(&ls.Limitation, "swissmedic_number5", number5, &mut pkg.limitations);
    }
    if let Some(itc) = seq_itcodes {
        for it in &itc.ItCode {
            if let Some(lims) = &it.Limitations {
                push(
                    &lims.Limitation,
                    "swissmedic_number5",
                    number5,
                    &mut pkg.limitations,
                );
            }
        }
    }
    if let Some(ls) = pac_lims {
        push(
            &ls.Limitation,
            "swissmedic_number8",
            &pkg.swissmedic_number8,
            &mut pkg.limitations,
        );
    }
}

// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RefdataItem {
    pub ean13: String,
    pub no8: String,
    pub data_origin: String,
    pub refdata: bool,
    pub r#type: String,
    pub last_change: String,
    pub desc_de: String,
    pub desc_fr: String,
    pub desc_it: String,
    pub atc_code: String,
    pub company_name: String,
    pub company_ean: String,
}

pub struct RefdataExtractor {
    pub xml: String,
    pub r#type: String,
}

impl RefdataExtractor {
    pub fn new(xml: impl Into<String>, type_: impl AsRef<str>) -> Self {
        let t = type_.as_ref().to_uppercase();
        let t = if t == "PHARMA" { "PHARMA" } else { "NONPHARMA" };
        Self {
            xml: xml.into(),
            r#type: t.to_string(),
        }
    }

    pub fn to_hash(&self) -> Result<HashMap<String, RefdataItem>> {
        let stripped = strip_xml_header(&self.xml);
        let stripped = strip_default_namespace(&stripped);
        let doc: SwissRegArticles = quick_xml::de::from_str(&stripped)
            .map_err(|e| anyhow::anyhow!("Refdata XML deserialize failed: {e}"))?;

        let mut out: HashMap<String, RefdataItem> = HashMap::new();

        for article in &doc.Article {
            let mp = match &article.MedicinalProduct { Some(x) => x, None => continue };
            let pkg = match &article.PackagedProduct { Some(x) => x, None => continue };
            let classification = match &mp.ProductClassification {
                Some(c) => c.ProductClass.clone().unwrap_or_default(),
                None => String::new(),
            };
            if classification != self.r#type {
                continue;
            }

            let raw_ean13 = if self.r#type == "PHARMA" {
                pkg.DataCarrierIdentifier.clone().unwrap_or_default()
            } else {
                mp.Identifier.clone().unwrap_or_default()
            };
            let mut ean13 = raw_ean13.clone();
            if ean13.len() < 13 {
                ean13 = pad_left(&ean13, 13, '0');
            }
            if ean13.len() == 14 && ean13.starts_with('0') {
                ean13 = ean13[1..].to_string();
            }

            let mut names = (String::new(), String::new(), String::new());
            for n in &pkg.Name {
                match n.Language.as_deref() {
                    Some("DE") => names.0 = n.FullName.clone().unwrap_or_default(),
                    Some("FR") => names.1 = n.FullName.clone().unwrap_or_default(),
                    Some("IT") => names.2 = n.FullName.clone().unwrap_or_default(),
                    _ => {}
                }
            }
            if names.2.is_empty() {
                names.2 = names.0.clone();
            }

            let holder = pkg.Holder.clone().unwrap_or_default();

            let item = RefdataItem {
                ean13: ean13.clone(),
                no8: pkg.RegulatedAuthorisationIdentifier.clone().unwrap_or_default(),
                data_origin: "refdata".into(),
                refdata: true,
                r#type: self.r#type.to_lowercase(),
                last_change: String::new(),
                desc_de: names.0,
                desc_fr: names.1,
                desc_it: names.2,
                atc_code: mp
                    .ProductClassification
                    .as_ref()
                    .and_then(|c| c.Atc.clone())
                    .unwrap_or_default(),
                company_name: holder.Name.unwrap_or_default(),
                company_ean: holder.Identifier.unwrap_or_default(),
            };
            out.insert(ean13, item);
        }

        Ok(out)
    }
}

// ------------------------------------------------------------------
// LPPV extractor — just collects EAN-13s from a text file.
// ------------------------------------------------------------------

pub struct LppvExtractor {
    pub text: String,
}

impl LppvExtractor {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn to_hash(&self) -> HashMap<String, bool> {
        let re = regex::Regex::new(r"\d{13}").unwrap();
        let mut out = HashMap::new();
        for line in self.text.lines() {
            if !re.is_match(line) {
                continue;
            }
            let ean13 = line.trim().replace('"', "");
            out.insert(ean13, true);
        }
        out
    }
}

// ------------------------------------------------------------------
// EPha interactions CSV extractor.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EphaInteraction {
    pub data_origin: String,
    pub ixno: usize,
    pub title: String,
    pub atc1: String,
    pub atc2: String,
    pub mechanism: String,
    pub effect: String,
    pub measures: String,
    pub grad: String,
}

pub struct EphaExtractor {
    pub text: String,
}

impl EphaExtractor {
    pub fn new(text: impl Into<String>) -> Self {
        Self { text: text.into() }
    }

    pub fn to_vec(&self) -> Vec<EphaInteraction> {
        let header = regex::Regex::new(r"ATC1.*Name1.*ATC2.*Name2").unwrap();
        let mut out = Vec::new();
        for (ixno, line) in self.text.split('\n').enumerate() {
            let ixno = ixno + 1;
            if header.is_match(line) {
                continue;
            }
            let cleaned = line.replace("\"\"", "\"");
            let mut rdr = csv::ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .from_reader(cleaned.as_bytes());
            if let Some(rec) = rdr.records().next() {
                if let Ok(rec) = rec {
                    if rec.len() <= 8 {
                        continue;
                    }
                    out.push(EphaInteraction {
                        data_origin: "epha".into(),
                        ixno,
                        title: rec[4].to_string(),
                        atc1: rec[0].to_string(),
                        atc2: rec[2].to_string(),
                        mechanism: rec[5].to_string(),
                        effect: rec[6].to_string(),
                        measures: rec[7].to_string(),
                        grad: rec[8].to_string(),
                    });
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------
// Firstbase CSV extractor.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FirstbaseItem {
    pub gtin: String,
    pub gln: String,
    pub target_market: String,
    pub gpc: String,
    pub trade_item_description_de: String,
    pub trade_item_description_en: String,
    pub trade_item_description_fr: String,
    pub trade_item_description_it: String,
    pub manufacturer_name: String,
    pub start_availability_date: String,
    pub gross_weight: String,
    pub net_weight: String,
}

pub struct FirstbaseExtractor<'a> {
    pub path: &'a std::path::Path,
}

impl<'a> FirstbaseExtractor<'a> {
    pub fn new(path: &'a std::path::Path) -> Self {
        Self { path }
    }

    pub fn to_hash(&self) -> Result<HashMap<String, FirstbaseItem>> {
        let mut out: HashMap<String, FirstbaseItem> = HashMap::new();
        let f = std::fs::File::open(self.path)?;
        let mut rdr = csv::ReaderBuilder::new().has_headers(true).from_reader(f);
        let strip_leading_zeros = regex::Regex::new(r"^0+").unwrap();
        for rec in rdr.deserialize::<HashMap<String, String>>() {
            let rec = rec?;
            let gtin_raw = rec.get("Gtin").map(|s| s.as_str()).unwrap_or_default();
            let gtin = strip_leading_zeros.replace(gtin_raw, "").to_string();
            if gtin.is_empty() {
                continue;
            }
            out.insert(
                gtin.clone(),
                FirstbaseItem {
                    gtin,
                    gln: rec.get("InformationProviderGln").cloned().unwrap_or_default(),
                    target_market: rec.get("TargetMarketCountryCode").cloned().unwrap_or_default(),
                    gpc: rec.get("GpcCategoryCode").cloned().unwrap_or_default(),
                    trade_item_description_de: rec
                        .get("TradeItemDescription_DE")
                        .cloned()
                        .unwrap_or_default(),
                    trade_item_description_en: String::new(),
                    trade_item_description_fr: rec
                        .get("TradeItemDescription_FR")
                        .cloned()
                        .unwrap_or_default(),
                    trade_item_description_it: rec
                        .get("TradeItemDescription_IT")
                        .cloned()
                        .unwrap_or_default(),
                    manufacturer_name: rec
                        .get("InformationProviderPartyName")
                        .cloned()
                        .unwrap_or_default(),
                    start_availability_date: rec.get("Date_Created_Batch").cloned().unwrap_or_default(),
                    gross_weight: String::new(),
                    net_weight: String::new(),
                },
            );
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------
// Swissmedic-Info (Fachinfo) — HTML dump per language.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SwissmedicInfoItem {
    pub refdata: bool,
    pub data_origin: String,
    pub name: String,
    pub owner: String,
    pub style: String,
    pub paragraph: String,
    pub monid: String,
}

pub struct SwissmedicInfoExtractor {
    pub xml: String,
}

impl SwissmedicInfoExtractor {
    pub fn new(xml: impl Into<String>) -> Self {
        Self { xml: xml.into() }
    }

    pub fn to_hash(&self) -> Result<HashMap<String, Vec<SwissmedicInfoItem>>> {
        let mut out: HashMap<String, Vec<SwissmedicInfoItem>> = HashMap::new();
        if self.xml.is_empty() {
            return Ok(out);
        }
        let stripped = strip_xml_header(&self.xml);
        let stripped = strip_default_namespace(&stripped);
        let doc: MedicalInformationsContent = quick_xml::de::from_str(&stripped)
            .map_err(|e| anyhow::anyhow!("Swissmedic-Info XML parse failed: {e}"))?;

        let monid_re = regex::Regex::new(r"(\d{5})").unwrap();

        for pac in &doc.medicalInformation {
            let lang = pac.lang.clone().unwrap_or_default();
            if lang != "de" && lang != "fr" {
                continue;
            }
            let content = pac.content.clone().unwrap_or_default();
            let style = pac.style.clone().unwrap_or_default();
            let name = pac.title.clone().unwrap_or_default();
            let owner = pac.authHolder.clone().unwrap_or_default();

            let monids: Vec<String> = monid_re
                .captures_iter(&content)
                .map(|c| c[1].to_string())
                .collect();
            for monid in monids {
                let item = SwissmedicInfoItem {
                    refdata: true,
                    data_origin: "swissmedic_info".into(),
                    name: name.clone(),
                    owner: owner.clone(),
                    style: style.clone(),
                    paragraph: content.clone(),
                    monid,
                };
                out.entry(lang.clone()).or_default().push(item);
            }
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------
// ZurRose transfer.dat — ISO-8859-14 fixed-width.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ZurroseItem {
    pub data_origin: String,
    pub line: String,
    pub ean13: String,
    pub clag: String,
    pub vat: String,
    pub description: String,
    pub quantity: String,
    pub pharmacode: String,
    pub price: String,
    pub pub_price: String,
    pub r#type: String,
    pub cmut: String,
}

pub struct ZurroseExtractor {
    pub text: String,
    pub extended: bool,
    pub artikelstamm: bool,
}

impl ZurroseExtractor {
    pub fn new(text: impl Into<String>, extended: bool, artikelstamm: bool) -> Self {
        Self { text: text.into(), extended, artikelstamm }
    }

    pub fn to_hash(&self) -> HashMap<String, ZurroseItem> {
        let vet = regex::Regex::new(r"(?i)(ad us\.* vet)|(\(vet\))").unwrap();
        let pharma_pat = regex::Regex::new(r"(7680\d{9})(\d{1})$").unwrap();
        let extended_pat = regex::Regex::new(r"(\d{13})(\d{1})$").unwrap();
        let mut out: HashMap<String, ZurroseItem> = HashMap::new();

        for raw in self.text.split('\n') {
            let line = util::patch_some_utf8(raw);
            if vet.is_match(&line) {
                continue;
            }
            let caps = if self.extended {
                extended_pat.captures(&line)
            } else {
                pharma_pat.captures(&line)
            };
            let caps = match caps { Some(c) => c, None => continue };
            let bytes: &[u8] = line.as_bytes();
            if bytes.len() < 97 {
                continue;
            }
            let safe_slice = |start: usize, end: usize| -> String {
                if end > bytes.len() {
                    return String::new();
                }
                String::from_utf8_lossy(&bytes[start..end]).into_owned()
            };

            let pharma_code = safe_slice(3, 10);
            let ean13_match = caps.get(1).unwrap().as_str();
            let ean13 = if ean13_match == "0000000000000" {
                if self.artikelstamm && pharma_code.trim().parse::<i64>().unwrap_or(0) == 0 {
                    continue;
                }
                if self.artikelstamm {
                    "-1".to_string()
                } else {
                    format!("{}{}", util::FAKE_GTIN_START, pharma_code)
                }
            } else {
                ean13_match.to_string()
            };

            let key = if ean13.parse::<i64>().unwrap_or(0) <= 0 {
                format!("{}{}", util::FAKE_GTIN_START, pharma_code)
            } else {
                ean13.clone()
            };

            if out.contains_key(&key) {
                continue;
            }

            let pexf_raw = safe_slice(60, 66);
            let ppub_raw = safe_slice(66, 72);
            let pexf = format_money(&pexf_raw);
            let ppub = format_money(&ppub_raw);

            if self.artikelstamm && line.starts_with("113") && pexf == "0.00" && ppub == "0.00" {
                continue;
            }

            let desc = safe_slice(10, 60);
            let description = desc.trim_end().to_string();
            let clag = safe_slice(73, 74);
            let vat_byte = safe_slice(96, 97);
            let cmut = safe_slice(2, 3);

            out.insert(
                key,
                ZurroseItem {
                    data_origin: "zur_rose".into(),
                    line: line.clone(),
                    ean13,
                    clag,
                    vat: vat_byte,
                    description,
                    quantity: String::new(),
                    pharmacode: pharma_code,
                    price: pexf,
                    pub_price: ppub,
                    r#type: "nonpharma".into(),
                    cmut,
                },
            );
        }
        out
    }
}

/// Turn a numeric string with an implied 2-decimal to a `x.xx` string.
fn format_money(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "0.00".into();
    }
    let re = regex::Regex::new(r"(\d{2})$").unwrap();
    let dotted = re.replace(trimmed, ".$1").to_string();
    let val: f64 = dotted.parse().unwrap_or(0.0);
    format!("{val:.2}")
}

// ------------------------------------------------------------------
// Swissmedic Packungen.xlsx (+ orphan xls) — calamine-based.
// ------------------------------------------------------------------

pub use swissmedic::*;

pub mod swissmedic {
    use super::*;
    use crate::util;
    use calamine::{open_workbook_auto, Data, Reader};
    use std::path::Path;

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct SwissmedicPackage {
        pub iksnr: i64,
        pub no8: String,
        pub ean13: String,
        pub prodno: String,
        pub seqnr: String,
        pub ith_swissmedic: String,
        pub swissmedic_category: String,
        pub atc_code: String,
        pub list_code: String,
        pub package_size: String,
        pub einheit_swissmedic: String,
        pub substance_swissmedic: String,
        pub composition_swissmedic: String,
        pub sequence_name: String,
        pub is_tier: bool,
        pub gen_production: String,
        pub insulin_category: String,
        pub drug_index: String,
        pub data_origin: String,
        pub expiry_date: String,
        pub company_name: String,
        pub size: String,
        pub unit: String,
    }

    pub struct SwissmedicExtractor {
        pub filename: std::path::PathBuf,
        pub kind: SwissmedicKind,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum SwissmedicKind {
        Package,
        Orphan,
    }

    impl SwissmedicExtractor {
        pub fn new(filename: impl AsRef<Path>, kind: SwissmedicKind) -> Self {
            Self {
                filename: filename.as_ref().to_path_buf(),
                kind,
            }
        }

        /// Orphan list: returns the (zero-padded, 5-digit) Zulassungsnummern.
        pub fn to_vec(&self) -> Result<Vec<String>, anyhow::Error> {
            if !self.filename.exists() {
                return Ok(Vec::new());
            }
            let mut wb = open_workbook_auto(&self.filename)?;
            let sheet_name = wb
                .sheet_names()
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no sheets"))?;
            let range = wb.worksheet_range(&sheet_name)?;
            let mut out: Vec<String> = Vec::new();
            match self.kind {
                SwissmedicKind::Orphan => {
                    let col_zulassung = 6;
                    for row in range.rows() {
                        if let Some(cell) = row.get(col_zulassung) {
                            if let Some(n) = as_i64(cell) {
                                if n != 0 {
                                    out.push(format!("{n:05}"));
                                }
                            }
                        }
                    }
                    out.sort();
                    out.dedup();
                }
                SwissmedicKind::Package => {}
            }
            Ok(out)
        }

        /// Packages: returns a map keyed by swissmedic_no8 → normalized row.
        pub fn to_hash(&self) -> Result<HashMap<String, SwissmedicPackage>, anyhow::Error> {
            let mut out: HashMap<String, SwissmedicPackage> = HashMap::new();
            if !self.filename.exists() {
                return Ok(out);
            }
            if self.kind != SwissmedicKind::Package {
                return Ok(out);
            }
            let mut wb = open_workbook_auto(&self.filename)?;
            let sheet_name = wb
                .sheet_names()
                .first()
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("no sheets"))?;
            let range = wb.worksheet_range(&sheet_name)?;

            let iksnr = util::column_index("iksnr");
            let ikscd = util::column_index("ikscd");
            let seqnr = util::column_index("seqnr");
            let cat = util::column_index("ikscat");
            let siz = util::column_index("size");
            let atc = util::column_index("atc_class");
            let list_code = util::column_index("production_science");
            let eht = util::column_index("unit");
            let sub = util::column_index("substances");
            let comp = util::column_index("composition");
            let seq_name = util::column_index("name_base");
            let ith = util::column_index("index_therapeuticus");
            let gen_production = util::column_index("gen_production");
            let insulin_category = util::column_index("insulin_category");
            let drug_index = util::column_index("drug_index");
            let expiry_date = util::column_index("expiry_date");
            let company = util::column_index("company");
            let unit = util::column_index("unit");

            for (i, row) in range.rows().enumerate() {
                if i <= 1 {
                    continue;
                }
                let iksnr_val = row.get(iksnr).and_then(as_i64).unwrap_or(0);
                let ikscd_val = row.get(ikscd).and_then(as_i64).unwrap_or(0);
                if iksnr_val == 0 || ikscd_val == 0 {
                    continue;
                }
                let no8 = format!("{iksnr_val:05}{ikscd_val:03}");
                let ean12 = format!("7680{no8}");
                let cd = util::calc_checksum(&ean12);
                let ean13 = format!("{ean12:0<12}{cd}");
                let seqnr_val = row.get(seqnr).and_then(as_i64).unwrap_or(0);
                let prodno = util::gen_prodno(iksnr_val as u64, seqnr_val as u64);
                util::set_ean13_for_prodno(&prodno, &ean13);
                util::set_ean13_for_no8(&no8, &ean13);

                let atc_raw = as_string(row.get(atc));
                let atc_code = util::add_epha_changes_for_atc(&iksnr_val.to_string(), &atc_raw);

                let list_code_val = as_string(row.get(list_code));
                let pkg = SwissmedicPackage {
                    iksnr: iksnr_val,
                    no8: no8.clone(),
                    ean13,
                    prodno,
                    seqnr: as_string(row.get(seqnr)),
                    ith_swissmedic: as_string(row.get(ith)),
                    swissmedic_category: as_string(row.get(cat)),
                    atc_code,
                    list_code: list_code_val.clone(),
                    package_size: as_string(row.get(siz)),
                    einheit_swissmedic: as_string(row.get(eht)),
                    substance_swissmedic: as_string(row.get(sub)),
                    composition_swissmedic: as_string(row.get(comp)),
                    sequence_name: as_string(row.get(seq_name)),
                    is_tier: list_code_val == "Tierarzneimittel",
                    gen_production: as_string(row.get(gen_production)),
                    insulin_category: as_string(row.get(insulin_category)),
                    drug_index: as_string(row.get(drug_index)),
                    data_origin: "swissmedic_package".into(),
                    expiry_date: as_string(row.get(expiry_date)),
                    company_name: as_string(row.get(company)),
                    size: as_string(row.get(siz)),
                    unit: as_string(row.get(unit)),
                };
                out.insert(no8, pkg);
            }
            Ok(out)
        }
    }

    fn as_i64(d: &Data) -> Option<i64> {
        match d {
            Data::Int(n) => Some(*n),
            Data::Float(f) => Some(*f as i64),
            Data::String(s) => s.trim().parse().ok(),
            Data::Empty => None,
            _ => None,
        }
    }

    fn as_string(d: Option<&Data>) -> String {
        match d {
            Some(Data::String(s)) => s.clone(),
            Some(Data::Int(n)) => n.to_string(),
            Some(Data::Float(f)) => {
                if (f.fract()).abs() < f64::EPSILON {
                    (*f as i64).to_string()
                } else {
                    f.to_string()
                }
            }
            Some(Data::Bool(b)) => b.to_string(),
            Some(Data::DateTime(d)) => d.to_string(),
            Some(Data::DateTimeIso(s)) => s.clone(),
            Some(Data::DurationIso(s)) => s.clone(),
            Some(Data::Error(_)) | Some(Data::Empty) | None => String::new(),
        }
    }
}

// ------------------------------------------------------------------
// Medreg (companies / persons) — tab-separated.
// ------------------------------------------------------------------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MedregCompany {
    pub data_origin: String,
    pub gln: String,
    pub name_1: String,
    pub name_2: String,
    pub address: String,
    pub number: String,
    pub post: String,
    pub place: String,
    pub region: String,
    pub country: String,
    pub r#type: String,
    pub authorization: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MedregPerson {
    pub data_origin: String,
    pub gln: String,
    pub last_name: String,
    pub first_name: String,
    pub post: String,
    pub place: String,
    pub region: String,
    pub country: String,
    pub license: String,
    pub certificate: String,
    pub authorization: String,
}

pub enum MedregRecord {
    Company(MedregCompany),
    Person(MedregPerson),
}

pub struct MedregbmExtractor {
    pub text: String,
    pub kind: MedregKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MedregKind {
    Company,
    Person,
}

impl MedregbmExtractor {
    pub fn new(text: impl Into<String>, kind: MedregKind) -> Self {
        Self { text: text.into(), kind }
    }

    pub fn to_vec(&self) -> Vec<MedregRecord> {
        let strip_non_digits = regex::Regex::new(r"[^0-9]").unwrap();
        let mut out = Vec::new();
        for line in self.text.lines() {
            let row: Vec<&str> = line.trim_end_matches('\r').split('\t').collect();
            if row.is_empty() || row[0].starts_with("GLN") {
                continue;
            }
            match self.kind {
                MedregKind::Company => {
                    if row.len() < 11 {
                        continue;
                    }
                    out.push(MedregRecord::Company(MedregCompany {
                        data_origin: "medreg".into(),
                        gln: strip_non_digits.replace_all(row[0], "").to_string(),
                        name_1: row[1].to_string(),
                        name_2: row[2].to_string(),
                        address: row[3].to_string(),
                        number: row[4].to_string(),
                        post: row[5].to_string(),
                        place: row[6].to_string(),
                        region: row[7].to_string(),
                        country: row[8].to_string(),
                        r#type: row[9].to_string(),
                        authorization: row[10].to_string(),
                    }));
                }
                MedregKind::Person => {
                    if row.len() < 10 {
                        continue;
                    }
                    out.push(MedregRecord::Person(MedregPerson {
                        data_origin: "medreg".into(),
                        gln: strip_non_digits.replace_all(row[0], "").to_string(),
                        last_name: row[1].to_string(),
                        first_name: row[2].to_string(),
                        post: row[3].to_string(),
                        place: row[4].to_string(),
                        region: row[5].to_string(),
                        country: row[6].to_string(),
                        license: row[7].to_string(),
                        certificate: row[8].to_string(),
                        authorization: row[9].to_string(),
                    }));
                }
            }
        }
        out
    }
}

// ------------------------------------------------------------------

/// quick-xml's serde backend does not strip the default namespace.  The
/// Ruby `SAXMachine` parser does.  Normalize both feeds by stripping
/// any `xmlns="..."` attribute from the first element we see so element
/// names match our struct field names.
fn strip_default_namespace(xml: &str) -> String {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r#"\s+xmlns(:[A-Za-z0-9_]+)?="[^"]*""#).unwrap()
    });
    RE.replace_all(xml, "").into_owned()
}

fn strip_xml_header(xml: &str) -> String {
    if let Some(rest) = xml.strip_prefix(STRIP_FOR_SAX_MACHINE) {
        return rest.to_string();
    }
    // Also handle the common variant without the trailing newline:
    if let Some(rest) = xml.strip_prefix("<?xml version=\"1.0\" encoding=\"utf-8\"?>") {
        return rest.trim_start().to_string();
    }
    xml.to_string()
}

fn pad_left(s: &str, width: usize, ch: char) -> String {
    if s.chars().count() >= width {
        return s.to_string();
    }
    let missing = width - s.chars().count();
    let prefix: String = std::iter::repeat(ch).take(missing).collect();
    format!("{prefix}{s}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lppv_extracts_ean13s() {
        let e = LppvExtractor::new("7680999999991\nfoo\n\"7680999999991\"\n");
        let h = e.to_hash();
        assert!(h.contains_key("7680999999991"));
    }

    #[test]
    fn epha_csv_skips_header() {
        let csv = "ATC1,Name1,ATC2,Name2,Title,Mech,Effect,Meas,Grad\nA,b,C,d,T,M,E,Me,G";
        let e = EphaExtractor::new(csv.to_string());
        let v = e.to_vec();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].atc1, "A");
    }
}
