//! XML comparison tool — port of `lib/oddb2xml/compare.rb`.
//!
//! Used by `bin/compare_v5`: reads two Artikelstamm/stammdaten XMLs,
//! reports added/removed/changed entries in each section.

use anyhow::Result;
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

#[derive(Debug, Default)]
pub struct CompareReport {
    pub added: BTreeSet<String>,
    pub removed: BTreeSet<String>,
    pub changed: BTreeMap<String, (String, String)>,
}

pub fn compare_files(a: impl AsRef<Path>, b: impl AsRef<Path>) -> Result<CompareReport> {
    let lhs = index_by_gtin(a.as_ref())?;
    let rhs = index_by_gtin(b.as_ref())?;

    let mut report = CompareReport::default();
    for k in lhs.keys() {
        if !rhs.contains_key(k) {
            report.removed.insert(k.clone());
        } else if rhs[k] != lhs[k] {
            report
                .changed
                .insert(k.clone(), (lhs[k].clone(), rhs[k].clone()));
        }
    }
    for k in rhs.keys() {
        if !lhs.contains_key(k) {
            report.added.insert(k.clone());
        }
    }
    Ok(report)
}

fn index_by_gtin(path: &Path) -> Result<BTreeMap<String, String>> {
    let mut reader = Reader::from_file(path)?;
    reader.config_mut().trim_text(true);
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut buf = Vec::new();
    let mut gtin: Option<String> = None;
    let mut in_item = false;
    let mut scratch = String::new();
    let mut current_tag: String = String::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                current_tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if current_tag == "ITEM" || current_tag == "ARTICLE" {
                    in_item = true;
                    gtin = None;
                    scratch.clear();
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if name == "ITEM" || name == "ARTICLE" {
                    if let Some(g) = gtin.take() {
                        out.insert(g, scratch.clone());
                    }
                    in_item = false;
                    scratch.clear();
                }
            }
            Ok(Event::Text(t)) => {
                if in_item {
                    let s = t.unescape().unwrap_or_default().into_owned();
                    if current_tag == "GTIN" {
                        gtin = Some(s.clone());
                    }
                    scratch.push_str(&s);
                    scratch.push('|');
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(e.into()),
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}
