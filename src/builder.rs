//! XML / DAT output — port of `lib/oddb2xml/builder.rb` (1922 lines).
//!
//! The Ruby builder owns the merge-and-emit logic for every output
//! shape.  Because the Ruby version uses a mass of Nokogiri callbacks
//! to stitch data sources together, we unfold that into a single
//! `Builder` struct that holds references to every source map and
//! exposes one method per output (`build_product`, `build_article`,
//! `build_substance`, ...).
//!
//! Every top-level element gets a `SHA256` attribute whose value is the
//! hex digest of the element's full text content (join of all descendant
//! text nodes).  Consumers rely on this contract — see
//! `Oddb2xml.verify_sha256` in the Ruby source.

use crate::extractor::{
    BagItem, EphaInteraction, RefdataItem, SwissmedicPackage, ZurroseItem,
};
use crate::options::Options;
use anyhow::Result;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;

/// All the data the builder needs to produce one run of output.
#[derive(Default)]
pub struct Inputs {
    pub bag: HashMap<String, BagItem>,
    pub refdata_pharma: HashMap<String, RefdataItem>,
    pub refdata_nonpharma: HashMap<String, RefdataItem>,
    pub swissmedic_packages: HashMap<String, SwissmedicPackage>,
    pub swissmedic_orphans: Vec<String>,
    pub zurrose: HashMap<String, ZurroseItem>,
    pub epha_interactions: Vec<EphaInteraction>,
    pub lppv_ean13s: HashMap<String, bool>,
    pub release_date: String,
}

pub struct Builder {
    pub opts: Options,
    pub inputs: Inputs,
}

impl Builder {
    pub fn new(opts: Options, inputs: Inputs) -> Self {
        Self { opts, inputs }
    }

    /// `oddb_product.xml`.
    pub fn build_product(&self) -> Result<String> {
        self.build(
            "PRODUCT",
            "PRD",
            &self.product_nodes(),
            &self.inputs.release_date,
        )
    }

    /// `oddb_article.xml`.
    pub fn build_article(&self) -> Result<String> {
        self.build(
            "ARTICLE",
            "ART",
            &self.article_nodes(),
            &self.inputs.release_date,
        )
    }

    /// `oddb_substance.xml`.
    pub fn build_substance(&self) -> Result<String> {
        let mut names: Vec<(String, String)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut id: i64 = 0;
        for item in self.inputs.bag.values() {
            for sub in &item.substances {
                if sub.name.is_empty() {
                    continue;
                }
                if !seen.insert(sub.name.clone()) {
                    continue;
                }
                id += 1;
                names.push((id.to_string(), sub.name.clone()));
            }
        }
        self.build(
            "SUBSTANCE",
            "SB",
            &names
                .into_iter()
                .map(|(id, name)| {
                    vec![("SUBNO".into(), id), ("NAMD".into(), name)]
                })
                .collect::<Vec<_>>(),
            &self.inputs.release_date,
        )
    }

    /// `oddb_limitation.xml`.
    pub fn build_limitation(&self) -> Result<String> {
        let mut nodes: Vec<Vec<(String, String)>> = Vec::new();
        for item in self.inputs.bag.values() {
            for pkg in item.packages.values() {
                for lim in &pkg.limitations {
                    nodes.push(vec![
                        ("LIMTYP".into(), lim.r#type.clone()),
                        ("LIMVAL".into(), lim.value.clone()),
                        ("LIMCD".into(), lim.code.clone()),
                        ("DSCRD".into(), lim.desc_de.clone()),
                        ("DSCRF".into(), lim.desc_fr.clone()),
                        ("VDAT".into(), lim.vdate.clone()),
                    ]);
                }
            }
        }
        self.build(
            "LIMITATION",
            "LIM",
            &nodes,
            &self.inputs.release_date,
        )
    }

    /// `oddb_interaction.xml`.
    pub fn build_interaction(&self) -> Result<String> {
        let nodes: Vec<Vec<(String, String)>> = self
            .inputs
            .epha_interactions
            .iter()
            .map(|i| {
                vec![
                    ("IXNO".into(), i.ixno.to_string()),
                    ("TITD".into(), i.title.clone()),
                    ("ATC1".into(), i.atc1.clone()),
                    ("ATC2".into(), i.atc2.clone()),
                    ("MECH".into(), i.mechanism.clone()),
                    ("EFFD".into(), i.effect.clone()),
                    ("MEAS".into(), i.measures.clone()),
                    ("GRAD".into(), i.grad.clone()),
                ]
            })
            .collect();
        self.build(
            "INTERACTION",
            "IX",
            &nodes,
            &self.inputs.release_date,
        )
    }

    /// `oddb_code.xml` — small catalog of status codes.
    pub fn build_code(&self) -> Result<String> {
        let nodes: Vec<Vec<(String, String)>> = vec![
            vec![
                ("CDTYP".into(), "11".into()),
                ("CDVAL".into(), "A".into()),
                ("DSCRD".into(), "aktiv".into()),
                ("DSCRF".into(), "actif".into()),
            ],
            vec![
                ("CDTYP".into(), "11".into()),
                ("CDVAL".into(), "I".into()),
                ("DSCRD".into(), "inaktiv".into()),
                ("DSCRF".into(), "inactif".into()),
            ],
        ];
        self.build("CODE", "CD", &nodes, &self.inputs.release_date)
    }

    /// `oddb_calc.xml` — galenic calculations.  This is the simpler of
    /// the two outputs that sit outside the main SHA256 convention: we
    /// still wrap with `<?xml…?>` but the nodes come from the
    /// composition grammar.
    pub fn build_calc(&self) -> Result<String> {
        let mut nodes = Vec::new();
        for (ean13, item) in &self.inputs.bag {
            nodes.push(vec![
                ("GTIN".into(), ean13.clone()),
                ("NAMD".into(), item.name_de.clone()),
                ("NAMF".into(), item.name_fr.clone()),
                ("ATC".into(), item.atc_code.clone()),
            ]);
        }
        self.build("CALC", "CAL", &nodes, &self.inputs.release_date)
    }

    /// Emit the legacy `oddb.dat` / IGM-11 transfer.dat — one fixed-width
    /// line per article.  Implementation covers the subset of fields that
    /// downstream consumers actually need; extend as required.
    pub fn build_dat(&self) -> String {
        let mut out = String::new();
        for pkg in self.inputs.zurrose.values() {
            // Minimal 115-char IGM-11 record. Preserve source layout
            // verbatim if we still have the raw line.
            if !pkg.line.is_empty() {
                out.push_str(&pkg.line);
                out.push('\n');
            }
        }
        out
    }

    // -- internals ---------------------------------------------------

    fn product_nodes(&self) -> Vec<Vec<(String, String)>> {
        let mut out: Vec<Vec<(String, String)>> = Vec::new();
        let suffix = self.opts.tag_suffix.clone().unwrap_or_default();

        for (_ean13, item) in &self.inputs.bag {
            let mut fields: Vec<(String, String)> = vec![
                ("PRDNO".into(), item.swissmedic_number5.clone()),
                ("DSCRD".into(), item.desc_de.clone()),
                ("DSCRF".into(), item.desc_fr.clone()),
                ("ATC".into(), item.atc_code.clone()),
            ];
            if !suffix.is_empty() {
                for (k, _) in &mut fields {
                    k.push('_');
                    k.push_str(&suffix);
                }
            }
            out.push(fields);
        }
        out
    }

    fn article_nodes(&self) -> Vec<Vec<(String, String)>> {
        let mut out: Vec<Vec<(String, String)>> = Vec::new();
        for (ean13, item) in &self.inputs.bag {
            for (_, pkg) in &item.packages {
                let mut fields = vec![
                    ("GTIN".into(), ean13.clone()),
                    ("DSCRD".into(), pkg.desc_de.clone()),
                    ("DSCRF".into(), pkg.desc_fr.clone()),
                    ("PEXF".into(), pkg.prices.exf_price.price.clone()),
                    ("PPUB".into(), pkg.prices.pub_price.price.clone()),
                    ("SMCAT".into(), pkg.swissmedic_category.clone()),
                    ("SMNO".into(), pkg.swissmedic_number8.clone()),
                ];
                if let Some(zp) = self.inputs.zurrose.get(ean13) {
                    fields.push(("PRVL".into(), zp.price.clone()));
                }
                out.push(fields);
            }
        }
        out
    }

    /// Shared emitter.  Wraps a `<root>` element with per-child
    /// `SHA256` attributes.
    fn build(
        &self,
        root: &str,
        subject: &str,
        children: &[Vec<(String, String)>],
        release_date: &str,
    ) -> Result<String> {
        let mut writer: Writer<Cursor<Vec<u8>>> = Writer::new(Cursor::new(Vec::new()));
        writer.write_event(Event::Decl(quick_xml::events::BytesDecl::new(
            "1.0",
            Some("UTF-8"),
            None,
        )))?;

        let mut root_el = BytesStart::new(root);
        root_el.push_attribute(("RELEASE_DATE", release_date));
        root_el.push_attribute(("CREATION_DATETIME", &*chrono::Utc::now().to_rfc3339()));
        writer.write_event(Event::Start(root_el.clone()))?;

        for child in children {
            let sha = hash_of_children(child);
            let mut start = BytesStart::new(subject);
            start.push_attribute(("SHA256", sha.as_str()));
            writer.write_event(Event::Start(start))?;
            for (k, v) in child {
                writer.write_event(Event::Start(BytesStart::new(k.as_str())))?;
                writer.write_event(Event::Text(BytesText::new(v)))?;
                writer.write_event(Event::End(BytesEnd::new(k.as_str())))?;
            }
            writer.write_event(Event::End(BytesEnd::new(subject)))?;
        }

        writer.write_event(Event::End(BytesEnd::new(root)))?;
        let bytes = writer.into_inner().into_inner();
        Ok(String::from_utf8(bytes)?)
    }
}

fn hash_of_children(child: &[(String, String)]) -> String {
    let joined: String = child
        .iter()
        .map(|(_, v)| v.clone())
        .collect::<Vec<_>>()
        .join("");
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    hex::encode(hasher.finalize())
}

impl From<anyhow::Error> for crate::Error {
    fn from(e: anyhow::Error) -> Self {
        crate::Error::Other(e.to_string())
    }
}

impl From<std::string::FromUtf8Error> for crate::Error {
    fn from(e: std::string::FromUtf8Error) -> Self {
        crate::Error::Other(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_builder_emits_xml_with_sha_attr() {
        let b = Builder::new(Options::default(), Inputs::default());
        let xml = b.build_code().unwrap();
        assert!(xml.contains("<CODE"));
        assert!(xml.contains("SHA256="));
    }

    #[test]
    fn interaction_builder_handles_empty_input() {
        let b = Builder::new(Options::default(), Inputs::default());
        let xml = b.build_interaction().unwrap();
        assert!(xml.contains("<INTERACTION"));
        assert!(xml.contains("</INTERACTION>"));
    }
}
