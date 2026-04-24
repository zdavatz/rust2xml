//! One-off scraper for homeopathic / herbal articles (Chapter 70 SL).
//! Port of `lib/oddb2xml/chapter_70_hack.rb`.
//!
//! Parses a simple HTML table and produces a hash keyed by a synthetic
//! GTIN of the form `999999 + pharmacode` (matching the Ruby behaviour).

use crate::util;
use anyhow::Result;
use scraper::{Html, Selector};
use std::collections::HashMap;

#[derive(Debug, Clone, Default)]
pub struct Chapter70Item {
    pub data_origin: String,
    pub ean13: String,
    pub pharmacode: String,
    pub desc_de: String,
    pub desc_fr: String,
    pub price: String,
    pub pub_price: String,
}

pub fn extract_from_html(html: &str) -> Result<HashMap<String, Chapter70Item>> {
    let doc = Html::parse_document(html);
    let row_sel = Selector::parse("table tr").expect("row selector");
    let td_sel = Selector::parse("td").expect("td selector");

    let mut out = HashMap::new();
    for row in doc.select(&row_sel) {
        let cells: Vec<String> = row
            .select(&td_sel)
            .map(|c| c.text().collect::<String>().trim().to_string())
            .collect();
        if cells.len() < 4 {
            continue;
        }
        let pharma = cells[0].clone();
        if pharma.is_empty() {
            continue;
        }
        let ean13 = format!("{}{}", util::FAKE_GTIN_START, pharma);
        out.insert(
            ean13.clone(),
            Chapter70Item {
                data_origin: "chapter_70".into(),
                ean13,
                pharmacode: pharma,
                desc_de: cells.get(1).cloned().unwrap_or_default(),
                desc_fr: cells.get(2).cloned().unwrap_or_default(),
                price: cells.get(3).cloned().unwrap_or_default(),
                pub_price: cells.get(4).cloned().unwrap_or_default(),
            },
        );
    }
    Ok(out)
}
