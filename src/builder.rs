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

use crate::calc;
use crate::extractor::{
    BagItem, EphaInteraction, FirstbaseItem, RefdataItem, SwissmedicPackage, ZurroseItem,
};
use crate::options::Options;
use anyhow::Result;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Cursor;

/// An XML child node.  Leafs carry text; Nested owns more children.
/// Used by both the flat output shapes (PRD, SB, LIM, IX, CD, CAL)
/// and the nested ART shape.
#[derive(Debug, Clone)]
pub enum Node {
    Leaf(String, String),
    Nested(String, Vec<Node>),
}

impl Node {
    pub fn leaf(tag: impl Into<String>, text: impl Into<String>) -> Self {
        Node::Leaf(tag.into(), text.into())
    }
    pub fn nested(tag: impl Into<String>, children: Vec<Node>) -> Self {
        Node::Nested(tag.into(), children)
    }

    /// Concatenated text of all leaf descendants — the input to
    /// `Digest::SHA256.hexdigest(node.text)` in the Ruby builder.
    fn text(&self) -> String {
        match self {
            Node::Leaf(_, t) => t.clone(),
            Node::Nested(_, cs) => cs.iter().map(Node::text).collect::<Vec<_>>().join(""),
        }
    }
}

fn joined_text(children: &[Node]) -> String {
    children.iter().map(Node::text).collect::<Vec<_>>().join("")
}

fn write_node(writer: &mut Writer<Cursor<Vec<u8>>>, node: &Node) -> Result<()> {
    match node {
        Node::Leaf(tag, text) => {
            writer.write_event(Event::Start(BytesStart::new(tag.as_str())))?;
            if !text.is_empty() {
                writer.write_event(Event::Text(BytesText::new(text)))?;
            }
            writer.write_event(Event::End(BytesEnd::new(tag.as_str())))?;
        }
        Node::Nested(tag, children) => {
            if children.is_empty() {
                // `<FOO/>` for an empty element, matching Ruby/nokogiri.
                writer.write_event(Event::Empty(BytesStart::new(tag.as_str())))?;
                return Ok(());
            }
            writer.write_event(Event::Start(BytesStart::new(tag.as_str())))?;
            for child in children {
                write_node(writer, child)?;
            }
            writer.write_event(Event::End(BytesEnd::new(tag.as_str())))?;
        }
    }
    Ok(())
}

/// All the data the builder needs to produce one run of output.
#[derive(Default)]
pub struct Inputs {
    pub bag: HashMap<String, BagItem>,
    pub refdata_pharma: HashMap<String, RefdataItem>,
    pub refdata_nonpharma: HashMap<String, RefdataItem>,
    pub swissmedic_packages: HashMap<String, SwissmedicPackage>,
    pub swissmedic_orphans: Vec<String>,
    pub zurrose: HashMap<String, ZurroseItem>,
    pub firstbase: HashMap<String, FirstbaseItem>,
    pub epha_interactions: Vec<EphaInteraction>,
    pub lppv_ean13s: HashMap<String, bool>,
    pub release_date: String,
}

pub struct Builder {
    pub opts: Options,
    pub inputs: Inputs,
}

impl Builder {
    pub fn new(opts: Options, mut inputs: Inputs) -> Self {
        crate::refdata_cleanup::apply(&mut inputs);
        Self { opts, inputs }
    }

    /// `oddb_product.xml`.
    pub fn build_product(&self) -> Result<String> {
        self.build("PRODUCT", "PRD", &self.product_records(), &self.inputs.release_date)
    }

    /// `oddb_article.xml` — emits Ruby's nested schema with
    /// <ARTBAR>/<ARTPRI> children.
    pub fn build_article(&self) -> Result<String> {
        self.build(
            "ARTICLE",
            "ART",
            &self.article_nodes(),
            &self.inputs.release_date,
        )
    }

    /// Records for `oddb_product.xml` as Node trees.
    pub fn product_records(&self) -> Vec<Vec<Node>> {
        self.product_nodes().into_iter().map(flat).collect()
    }

    /// Records for `oddb_article.xml` as Node trees (with nested ARTBAR/ARTPRI).
    pub fn article_records(&self) -> Vec<Vec<Node>> {
        self.article_nodes()
    }

    /// `oddb_substance.xml`.
    pub fn build_substance(&self) -> Result<String> {
        self.build("SUBSTANCE", "SB", &self.substance_records(), &self.inputs.release_date)
    }

    /// Records for `oddb_substance.xml`.
    pub fn substance_records(&self) -> Vec<Vec<Node>> {
        let mut seen = std::collections::HashSet::new();
        let mut records: Vec<Vec<Node>> = Vec::new();
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
                records.push(vec![
                    Node::leaf("SUBNO", id.to_string()),
                    Node::leaf("NAMD", sub.name.clone()),
                ]);
            }
        }
        records
    }

    /// `oddb_limitation.xml`.  Emits Ruby's LIM schema:
    /// `<SwissmedicNo5>`, `<IT>`, `<LIMTYP>`, `<LIMVAL>`, `<LIMNAMEBAG>`,
    /// `<LIMNIV>`, `<DSCRD>`, `<DSCRF>`, `<VDAT>`.  Deduplicated by
    /// (SwissmedicNo5, LIMNAMEBAG, LIMTYP, LIMVAL).
    pub fn build_limitation(&self) -> Result<String> {
        self.build("LIMITATION", "LIM", &self.limitation_records(), &self.inputs.release_date)
    }

    /// Records for `oddb_limitation.xml`.  Keyed at the package level
    /// (Swissmedic-No8) — a limitation applies to a specific Packung,
    /// not to the whole preparation.  The dedup key includes the
    /// description text so distinct limitations don't collapse when
    /// the FHIR feed leaves the categorical fields blank.
    pub fn limitation_records(&self) -> Vec<Vec<Node>> {
        let mut seen: std::collections::HashSet<(String, String, String, String, String)> =
            std::collections::HashSet::new();
        let mut nodes: Vec<Vec<Node>> = Vec::new();
        for item in self.inputs.bag.values() {
            for pkg in item.packages.values() {
                for lim in &pkg.limitations {
                    let key = (
                        pkg.swissmedic_number8.clone(),
                        lim.code.clone(),
                        lim.r#type.clone(),
                        lim.value.clone(),
                        lim.desc_de.clone(),
                    );
                    if !seen.insert(key) {
                        continue;
                    }
                    nodes.push(vec![
                        Node::leaf("SwissmedicNo8", pkg.swissmedic_number8.clone()),
                        Node::leaf("GTIN", pkg.ean13.clone()),
                        Node::leaf("IT", lim.it.clone()),
                        Node::leaf("LIMTYP", lim.r#type.clone()),
                        Node::leaf("LIMVAL", lim.value.clone()),
                        Node::leaf("LIMNAMEBAG", lim.code.clone()),
                        Node::leaf("LIMNIV", lim.niv.clone()),
                        Node::leaf("DSCRD", lim.desc_de.clone()),
                        Node::leaf("DSCRF", lim.desc_fr.clone()),
                        Node::leaf("VDAT", lim.vdate.clone()),
                    ]);
                }
            }
        }
        nodes
    }

    /// `oddb_interaction.xml`.
    pub fn build_interaction(&self) -> Result<String> {
        self.build("INTERACTION", "IX", &self.interaction_records(), &self.inputs.release_date)
    }

    /// Records for `oddb_interaction.xml`.
    pub fn interaction_records(&self) -> Vec<Vec<Node>> {
        self.inputs
            .epha_interactions
            .iter()
            .map(|i| {
                vec![
                    Node::leaf("IXNO", i.ixno.to_string()),
                    Node::leaf("TITD", i.title.clone()),
                    Node::leaf("ATC1", i.atc1.clone()),
                    Node::leaf("ATC2", i.atc2.clone()),
                    Node::leaf("MECH", i.mechanism.clone()),
                    Node::leaf("EFFD", i.effect.clone()),
                    Node::leaf("MEAS", i.measures.clone()),
                    Node::leaf("GRAD", i.grad.clone()),
                ]
            })
            .collect()
    }

    /// `oddb_code.xml` — static catalog of status codes.  Matches
    /// the Ruby builder's hard-coded list.
    pub fn build_code(&self) -> Result<String> {
        self.build("CODE", "CD", &self.code_records(), &self.inputs.release_date)
    }

    /// Records for `oddb_code.xml`.
    pub fn code_records(&self) -> Vec<Vec<Node>> {
        let mk = |val: &str, dscr: &str| -> Vec<Node> {
            vec![
                Node::leaf("CDTYP", "11"),
                Node::leaf("CDVAL", val),
                Node::leaf("DSCRSD", dscr),
                Node::leaf("DEL", "false"),
            ]
        };
        vec![
            mk("X", "Kontraindiziert"),
            mk("O", "Nur in Ausnahmefällen"),
            mk("R", "Strenge Indikationsstellung"),
            mk("V", "Vorsichtsmassnahmen"),
            mk("U", "Unbedenklich"),
        ]
    }

    /// `oddb_calc.xml` — galenic calculations.  One CAL per article:
    /// GTIN + names + ATC + IT + pack size & unit + galenic form &
    /// group (looked up via `calc::group_by_form`) + OID + composition.
    pub fn build_calc(&self) -> Result<String> {
        self.build("CALC", "CAL", &self.calc_records(), &self.inputs.release_date)
    }

    /// Records for `oddb_calc.xml`.
    pub fn calc_records(&self) -> Vec<Vec<Node>> {
        let mut nodes: Vec<Vec<Node>> = Vec::new();
        let mut emitted: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        for (ean13, item) in &self.inputs.bag {
            if !emitted.insert(ean13.clone()) {
                continue;
            }
            let pkg = item.packages.get(ean13);
            let no8 = pkg.map(|p| p.swissmedic_number8.clone()).unwrap_or_default();
            let sm = self.inputs.swissmedic_packages.get(&no8);
            let pack_size = sm.map(|s| s.package_size.clone()).unwrap_or_default();
            let unit = sm.map(|s| s.einheit_swissmedic.clone()).unwrap_or_default();
            let composition = sm
                .map(|s| s.composition_swissmedic.clone())
                .unwrap_or_default();
            let substance = sm
                .map(|s| s.substance_swissmedic.clone())
                .unwrap_or_default();
            let (form, group, oid) = galenic_for(&item.desc_de, &unit);

            nodes.push(vec![
                Node::leaf("GTIN", ean13.clone()),
                Node::leaf("PHAR", String::new()),
                Node::leaf("NAMD", item.name_de.clone()),
                Node::leaf("NAMF", item.name_fr.clone()),
                Node::leaf("ATC", item.atc_code.clone()),
                Node::leaf("IT", item.it_code.clone()),
                Node::leaf("PACKSIZE", pack_size),
                Node::leaf("UNIT", unit),
                Node::leaf("FORM", form),
                Node::leaf("GROUP", group),
                Node::leaf("OID", oid),
                Node::leaf("SUBSTANCE", substance),
                Node::leaf("COMPOSITION", composition),
            ]);
        }

        // Also include Swissmedic-only packages so every known article
        // has a calc row — Ruby's version does the same union.
        for (_no8, sm) in &self.inputs.swissmedic_packages {
            if sm.ean13.is_empty() || !emitted.insert(sm.ean13.clone()) {
                continue;
            }
            let (form, group, oid) = galenic_for(&sm.sequence_name, &sm.einheit_swissmedic);
            nodes.push(vec![
                Node::leaf("GTIN", sm.ean13.clone()),
                Node::leaf("PHAR", String::new()),
                Node::leaf("NAMD", sm.sequence_name.clone()),
                Node::leaf("NAMF", String::new()),
                Node::leaf("ATC", sm.atc_code.clone()),
                Node::leaf("IT", sm.ith_swissmedic.clone()),
                Node::leaf("PACKSIZE", sm.package_size.clone()),
                Node::leaf("UNIT", sm.einheit_swissmedic.clone()),
                Node::leaf("FORM", form),
                Node::leaf("GROUP", group),
                Node::leaf("OID", oid),
                Node::leaf("SUBSTANCE", sm.substance_swissmedic.clone()),
                Node::leaf("COMPOSITION", sm.composition_swissmedic.clone()),
            ]);
        }

        nodes
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
        // One PRD per EAN-13 in the merged bag/swissmedic map — Ruby
        // emits at this granularity, not per-preparation.
        let mut out: Vec<Vec<(String, String)>> = Vec::new();
        let mut emitted_ean13: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        let suffix = self.opts.tag_suffix.clone().unwrap_or_default();

        for (ean13, item) in &self.inputs.bag {
            // Look up Swissmedic data for this pack specifically
            // (ean13 keys our bag map, and each bag item carries the
            // pack's no8 via packages[ean13]).
            let no8 = item
                .packages
                .get(ean13)
                .map(|p| p.swissmedic_number8.clone())
                .unwrap_or_default();
            let smdata = self.inputs.swissmedic_packages.get(&no8);
            emitted_ean13.insert(ean13.clone());

            let prodno = smdata.map(|s| s.prodno.clone()).unwrap_or_default();
            let it_code = smdata
                .map(|s| s.ith_swissmedic.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| item.it_code.clone());
            let pack_size = smdata.map(|s| s.package_size.clone()).unwrap_or_default();
            let einheit = smdata.map(|s| s.einheit_swissmedic.clone()).unwrap_or_default();
            let substance = smdata
                .map(|s| s.substance_swissmedic.clone())
                .unwrap_or_default();
            let composition = smdata
                .map(|s| s.composition_swissmedic.clone())
                .unwrap_or_default();
            let gtin = smdata
                .map(|s| s.ean13.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| ean13.clone());
            let atc = smdata
                .map(|s| s.atc_code.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| item.atc_code.clone());

            let mut fields: Vec<(String, String)> = vec![
                ("GTIN".into(), gtin),
                ("PRODNO".into(), prodno),
                ("DSCRD".into(), item.desc_de.clone()),
                ("DSCRF".into(), item.desc_fr.clone()),
                ("ATC".into(), atc),
                ("IT".into(), it_code),
                ("CPT".into(), String::new()),
                ("PackGrSwissmedic".into(), pack_size),
                ("EinheitSwissmedic".into(), einheit),
                ("SubstanceSwissmedic".into(), substance),
                ("CompositionSwissmedic".into(), composition),
            ];
            if !suffix.is_empty() {
                for (k, _) in &mut fields {
                    k.push('_');
                    k.push_str(&suffix);
                }
            }
            out.push(fields);
        }

        // Add Swissmedic-only packages (products Swissmedic knows about
        // but that never made it into BAG / SL).  Ruby's builder pulls
        // from the merged map — we replicate with an explicit union.
        for (_no8, sm) in &self.inputs.swissmedic_packages {
            let ean13 = &sm.ean13;
            if emitted_ean13.contains(ean13) {
                continue;
            }
            emitted_ean13.insert(ean13.clone());
            let mut fields: Vec<(String, String)> = vec![
                ("GTIN".into(), ean13.clone()),
                ("PRODNO".into(), sm.prodno.clone()),
                ("DSCRD".into(), sm.sequence_name.clone()),
                ("DSCRF".into(), String::new()),
                ("ATC".into(), sm.atc_code.clone()),
                ("IT".into(), sm.ith_swissmedic.clone()),
                ("CPT".into(), String::new()),
                ("PackGrSwissmedic".into(), sm.package_size.clone()),
                ("EinheitSwissmedic".into(), sm.einheit_swissmedic.clone()),
                ("SubstanceSwissmedic".into(), sm.substance_swissmedic.clone()),
                ("CompositionSwissmedic".into(), sm.composition_swissmedic.clone()),
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

    /// Produce Ruby's nested ART schema.
    fn article_nodes(&self) -> Vec<Vec<Node>> {
        let mut out: Vec<Vec<Node>> = Vec::new();
        let mut emitted: std::collections::HashSet<String> =
            std::collections::HashSet::new();

        // 1. Every pack of every BAG preparation — SL/pharma articles.
        for (_, item) in &self.inputs.bag {
            for (pkg_ean13, pkg) in &item.packages {
                if !emitted.insert(pkg_ean13.clone()) {
                    continue;
                }
                let zr = self.inputs.zurrose.get(pkg_ean13);
                let nt = ArtFields {
                    ref_data: "1", // from refdata/SL
                    phar: zr.map(|z| z.pharmacode.clone()).unwrap_or_default(),
                    vat: zr.map(|z| z.vat.clone()).unwrap_or_default(),
                    salecd: "A",
                    dscrd: pkg.desc_de.clone(),
                    dscrf: pkg.desc_fr.clone(),
                    barcodes: vec![(String::from("E13"), pkg_ean13.clone(), "A")],
                    prices: art_prices(
                        &pkg.prices.exf_price.price,
                        &pkg.prices.pub_price.price,
                        zr.map(|z| z.price.as_str()).unwrap_or(""),
                        zr.map(|z| z.pub_price.as_str()).unwrap_or(""),
                    ),
                    smcat: pkg.swissmedic_category.clone(),
                    smno: pkg.swissmedic_number8.clone(),
                    limpts: pkg.limitation_points.clone(),
                };
                out.push(nt.into_nodes());
            }
        }

        // 2. Refdata non-pharma.
        for (ean13, r) in &self.inputs.refdata_nonpharma {
            if !emitted.insert(ean13.clone()) {
                continue;
            }
            let zr = self.inputs.zurrose.get(ean13);
            let nt = ArtFields {
                ref_data: "1",
                phar: zr.map(|z| z.pharmacode.clone()).unwrap_or_default(),
                vat: zr.map(|z| z.vat.clone()).unwrap_or_default(),
                salecd: "A",
                dscrd: r.desc_de.clone(),
                dscrf: r.desc_fr.clone(),
                barcodes: vec![(String::from("E13"), ean13.clone(), "A")],
                prices: art_prices(
                    "",
                    "",
                    zr.map(|z| z.price.as_str()).unwrap_or(""),
                    zr.map(|z| z.pub_price.as_str()).unwrap_or(""),
                ),
                smcat: String::new(),
                smno: r.no8.clone(),
                limpts: String::new(),
            };
            out.push(nt.into_nodes());
        }

        // 3. Refdata pharma not already in BAG.
        for (ean13, r) in &self.inputs.refdata_pharma {
            if !emitted.insert(ean13.clone()) {
                continue;
            }
            let zr = self.inputs.zurrose.get(ean13);
            let nt = ArtFields {
                ref_data: "1",
                phar: zr.map(|z| z.pharmacode.clone()).unwrap_or_default(),
                vat: zr.map(|z| z.vat.clone()).unwrap_or_default(),
                salecd: "A",
                dscrd: r.desc_de.clone(),
                dscrf: r.desc_fr.clone(),
                barcodes: vec![(String::from("E13"), ean13.clone(), "A")],
                prices: art_prices(
                    "",
                    "",
                    zr.map(|z| z.price.as_str()).unwrap_or(""),
                    zr.map(|z| z.pub_price.as_str()).unwrap_or(""),
                ),
                smcat: String::new(),
                smno: r.no8.clone(),
                limpts: String::new(),
            };
            out.push(nt.into_nodes());
        }

        // 4. ZurRose-only articles.
        for (ean13, zr) in &self.inputs.zurrose {
            if !emitted.insert(ean13.clone()) {
                continue;
            }
            let nt = ArtFields {
                ref_data: "0",
                phar: zr.pharmacode.clone(),
                vat: zr.vat.clone(),
                salecd: "I",
                dscrd: zr.description.clone(),
                dscrf: zr.description.clone(),
                barcodes: vec![(String::from("E13"), ean13.clone(), "A")],
                prices: art_prices("", "", &zr.price, &zr.pub_price),
                smcat: String::new(),
                smno: String::new(),
                limpts: String::new(),
            };
            out.push(nt.into_nodes());
        }

        // 5. Firstbase GS1 items.
        for (gtin, fb) in &self.inputs.firstbase {
            if !emitted.insert(gtin.clone()) {
                continue;
            }
            let nt = ArtFields {
                ref_data: "0",
                phar: String::new(),
                vat: String::new(),
                salecd: "A",
                dscrd: fb.trade_item_description_de.clone(),
                dscrf: fb.trade_item_description_fr.clone(),
                barcodes: vec![(String::from("E13"), gtin.clone(), "A")],
                prices: Vec::new(),
                smcat: String::new(),
                smno: String::new(),
                limpts: String::new(),
            };
            out.push(nt.into_nodes());
        }

        out
    }

    /// Shared emitter.  Wraps a `<root>` element with per-child
    /// `SHA256` attributes.  Accepts nested-capable `Node` children.
    fn build(
        &self,
        root: &str,
        subject: &str,
        records: &[Vec<Node>],
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

        for children in records {
            let sha = hash_of_nodes(children);
            let mut start = BytesStart::new(subject);
            start.push_attribute(("DT", ""));
            start.push_attribute(("SHA256", sha.as_str()));
            writer.write_event(Event::Start(start))?;
            for node in children {
                write_node(&mut writer, node)?;
            }
            writer.write_event(Event::End(BytesEnd::new(subject)))?;
        }

        writer.write_event(Event::End(BytesEnd::new(root)))?;
        let bytes = writer.into_inner().into_inner();
        Ok(String::from_utf8(bytes)?)
    }
}

/// Intermediate representation of a single ART record before it's
/// serialised.  Keeps the per-source branches readable.
struct ArtFields {
    ref_data: &'static str,
    phar: String,
    vat: String,
    salecd: &'static str,
    dscrd: String,
    dscrf: String,
    /// (code-type, barcode, bcstat)
    barcodes: Vec<(String, String, &'static str)>,
    /// (price-type, price-value)
    prices: Vec<(String, String)>,
    smcat: String,
    smno: String,
    limpts: String,
}

impl ArtFields {
    fn into_nodes(self) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::with_capacity(16);
        out.push(Node::leaf("REF_DATA", self.ref_data));
        out.push(Node::leaf("PHAR", self.phar));
        out.push(Node::leaf("VAT", self.vat));
        out.push(Node::leaf("SALECD", self.salecd));
        out.push(Node::leaf("CDBG", "N"));
        out.push(Node::leaf("BG", "N"));
        out.push(Node::leaf("DSCRD", self.dscrd.clone()));
        out.push(Node::leaf("DSCRF", self.dscrf.clone()));
        out.push(Node::leaf("SORTD", self.dscrd.to_uppercase()));
        out.push(Node::leaf("SORTF", self.dscrf.to_uppercase()));
        out.push(Node::Nested("ARTCOMP".into(), Vec::new()));
        for (cdtyp, bc, stat) in self.barcodes {
            out.push(Node::Nested(
                "ARTBAR".into(),
                vec![
                    Node::leaf("CDTYP", cdtyp),
                    Node::leaf("BC", bc),
                    Node::leaf("BCSTAT", stat),
                ],
            ));
        }
        for (ptyp, price) in self.prices {
            out.push(Node::Nested(
                "ARTPRI".into(),
                vec![Node::leaf("PTYP", ptyp), Node::leaf("PRICE", price)],
            ));
        }
        if !self.smcat.is_empty() {
            out.push(Node::leaf("SMCAT", self.smcat));
        }
        if !self.smno.is_empty() {
            out.push(Node::leaf("SMNO", self.smno));
        }
        if !self.limpts.is_empty() {
            out.push(Node::leaf("LIMPTS", self.limpts));
        }
        out
    }
}

fn art_prices(
    pexf: &str,
    ppub: &str,
    zur_rose: &str,
    zur_rose_pub: &str,
) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    if !pexf.is_empty() {
        out.push(("FACTORY".into(), pexf.to_string()));
    }
    if !ppub.is_empty() {
        out.push(("PUBLIC".into(), ppub.to_string()));
    }
    if !zur_rose.is_empty() && zur_rose != "0.00" {
        out.push(("ZURROSE".into(), zur_rose.to_string()));
    }
    if !zur_rose_pub.is_empty() && zur_rose_pub != "0.00" {
        out.push(("ZURROSEPUB".into(), zur_rose_pub.to_string()));
    }
    out
}

/// Look up a galenic form from a free-text `desc` (e.g. "Filmtabletten
/// 100 mg") or a Swissmedic unit ("Tablette(n)").  Returns (form,
/// group, oid_string).  Empty strings when nothing matches — Ruby does
/// the same.
fn galenic_for(desc: &str, unit: &str) -> (String, String, String) {
    let candidates = [desc, unit];
    for hay in candidates {
        let hay = hay.trim();
        if hay.is_empty() {
            continue;
        }
        // Try each known form as a substring match so "Filmtabletten
        // 100 mg" still resolves via "Filmtablette".
        for (form, _) in calc::known_forms() {
            if hay.contains(form) {
                if let Some(group) = calc::group_by_form(form) {
                    let oid = calc::oid_for_group(group)
                        .map(|n| n.to_string())
                        .unwrap_or_default();
                    return (form.to_string(), group.to_string(), oid);
                }
            }
        }
    }
    (String::new(), String::new(), String::new())
}

fn hash_of_nodes(children: &[Node]) -> String {
    let joined = joined_text(children);
    let mut hasher = Sha256::new();
    hasher.update(joined.as_bytes());
    hex::encode(hasher.finalize())
}

/// Adapt a flat tag→text list into `Vec<Node>` for the shared emitter.
fn flat(pairs: Vec<(String, String)>) -> Vec<Node> {
    pairs.into_iter().map(|(k, v)| Node::Leaf(k, v)).collect()
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
