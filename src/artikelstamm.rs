//! Artikelstamm v6 emitter — port of `Oddb2xml::Builder#build_artikelstamm`.
//!
//! Produces the Elexis `Elexis_Artikelstamm_v6` XML consumed by Elexis
//! (>= 3.1) via the `--artikelstamm` / `-as` CLI flag.  Unlike the
//! `oddb_*.xml` outputs this has a bespoke three-section shape
//! (`<PRODUCTS>` / `<LIMITATIONS>` / `<ITEMS>`) with a document header
//! carrying `CREATION_DATETIME` / `BUILD_DATETIME` / `DATA_SOURCE`, and
//! it carries **no** per-element `SHA256` attribute (Ruby drops it too).
//!
//! The v6 schema adds the `<ARTSL>` block on each `<ITEM>` carrying the
//! BAG Indikationscode(s) (`XXXXX.NN`) for SL price-model drugs (oddb2xml
//! issue #113); the legacy v5 schema has no `<ARTSL>`, so passing
//! `version = 5` suppresses it and switches the namespace.
//!
//! A companion `artikelstamm_DDMMYYYY_v6.csv` is emitted alongside the
//! XML (`Builder::artikelstamm_csv`), mirroring the Ruby CSV.
//!
//! By default the items/products/limitations carry only the German
//! (`<DSCR>`) and French (`<DSCRF>`) descriptions.  The Italian
//! description (`<DSCRI>`) is emitted only when `opts.italian` is set
//! (`-it` / `--italian`): the strict upstream Elexis v6/v5 XSD that the
//! Elexis importer validates against has no `<DSCRI>` element, so
//! emitting it unconditionally makes the import fail with a
//! `cvc-complex-type.2.4.a` error (see `medindex_first_v6.xml`).

use crate::builder::{first_non_empty, galenic_for, Builder, Node};
use crate::extractor::{BagItem, BagPackage};
use anyhow::Result;
use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Writer;
use std::collections::{HashMap, HashSet};
use std::io::Cursor;

/// One `<ITEM>` ready to serialise: the `PHARMATYPE` attribute plus the
/// ordered child nodes.  `csv` is the companion CSV row (12 columns, see
/// [`Builder::artikelstamm_csv`]) so the CSV always mirrors the XML `<ITEM>`
/// set exactly — one row per emitted item, pharma and non-pharma alike.
struct Item {
    pharmatype: &'static str,
    children: Vec<Node>,
    csv: Vec<String>,
}

impl Builder {
    /// Build the Artikelstamm XML.  `version` is 6 (default) or 5 (legacy,
    /// no `<ARTSL>`).  The namespace is `Elexis_Artikelstamm_v{version}`.
    pub fn build_artikelstamm(&self, version: u8) -> Result<String> {
        let (products, used_lims) = self.artikelstamm_products();
        let limitations = self.artikelstamm_limitations(&used_lims);
        let items = self.artikelstamm_items(version);

        let mut writer: Writer<Cursor<Vec<u8>>> =
            Writer::new_with_indent(Cursor::new(Vec::new()), b' ', 4);
        writer.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))?;
        writer.write_event(Event::Comment(BytesText::new(&format!(
            "Produced by rust2xml version {} artikelstamm v{}",
            crate::version::VERSION,
            version
        ))))?;

        let now = chrono::Local::now().format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string();
        let mut root = BytesStart::new("ARTIKELSTAMM");
        root.push_attribute(("xmlns", format!("http://elexis.ch/Elexis_Artikelstamm_v{version}").as_str()));
        root.push_attribute(("CREATION_DATETIME", now.as_str()));
        root.push_attribute(("BUILD_DATETIME", now.as_str()));
        root.push_attribute(("DATA_SOURCE", "oddb2xml"));
        writer.write_event(Event::Start(root))?;

        // <PRODUCTS>
        writer.write_event(Event::Start(BytesStart::new("PRODUCTS")))?;
        for children in &products {
            writer.write_event(Event::Start(BytesStart::new("PRODUCT")))?;
            for n in children {
                emit_node(&mut writer, n)?;
            }
            writer.write_event(Event::End(BytesEnd::new("PRODUCT")))?;
        }
        writer.write_event(Event::End(BytesEnd::new("PRODUCTS")))?;

        // <LIMITATIONS>
        writer.write_event(Event::Start(BytesStart::new("LIMITATIONS")))?;
        for children in &limitations {
            writer.write_event(Event::Start(BytesStart::new("LIMITATION")))?;
            for n in children {
                emit_node(&mut writer, n)?;
            }
            writer.write_event(Event::End(BytesEnd::new("LIMITATION")))?;
        }
        writer.write_event(Event::End(BytesEnd::new("LIMITATIONS")))?;

        // <ITEMS>
        writer.write_event(Event::Start(BytesStart::new("ITEMS")))?;
        for item in &items {
            let mut start = BytesStart::new("ITEM");
            start.push_attribute(("PHARMATYPE", item.pharmatype));
            writer.write_event(Event::Start(start))?;
            for n in &item.children {
                emit_node(&mut writer, n)?;
            }
            writer.write_event(Event::End(BytesEnd::new("ITEM")))?;
        }
        writer.write_event(Event::End(BytesEnd::new("ITEMS")))?;

        writer.write_event(Event::End(BytesEnd::new("ARTIKELSTAMM")))?;
        let bytes = writer.into_inner().into_inner();
        Ok(String::from_utf8(bytes)?)
    }

    /// Companion CSV: **one row per emitted `<ITEM>`** — pharma packs *and*
    /// non-pharma articles — mirroring the Ruby `@csv_file` (which writes a row
    /// for every article, not just the priced SL packs).  Columns: gtin, name,
    /// pkg_size, galenic_form, price_ex_factory, price_public, prodno,
    /// atc_code, active_substance, original, it-code, sl-liste.  The rows are
    /// carried on each [`Item`] so the CSV set is identical to the XML
    /// `<ITEMS>` set; the `version` passed here does not affect the CSV (the
    /// per-version `<ARTSL>` block lives only in the XML).
    pub fn artikelstamm_csv(&self) -> String {
        let mut out = String::new();
        out.push_str("gtin,name,pkg_size,galenic_form,price_ex_factory,price_public,prodno,atc_code,active_substance,original,it-code,sl-liste\n");
        for item in self.artikelstamm_items(6) {
            out.push_str(&item.csv.iter().map(|c| csv_escape(c)).collect::<Vec<_>>().join(","));
            out.push('\n');
        }
        out
    }

    // -- sections ----------------------------------------------------

    /// `<PRODUCTS>` — one `<PRODUCT>` per PRODNO.  Returns the product node
    /// lists plus the set of limitation codes referenced (to filter
    /// `<LIMITATIONS>`).
    fn artikelstamm_products(&self) -> (Vec<Vec<Node>>, HashSet<String>) {
        let mut emitted: HashSet<String> = HashSet::new();
        let mut used_lims: HashSet<String> = HashSet::new();
        // prodno -> nodes; collected into a map first so packs of the same
        // sequence collapse to one PRODUCT, then sorted by prodno.
        let mut by_prodno: Vec<(String, Vec<Node>)> = Vec::new();

        let mut bag_keys: Vec<&String> = self.inputs.bag.keys().collect();
        bag_keys.sort();
        for ean13 in bag_keys {
            let item = &self.inputs.bag[ean13];
            let mut pkg_keys: Vec<&String> = item.packages.keys().collect();
            pkg_keys.sort();
            for pkg_ean in pkg_keys {
                let pkg = &item.packages[pkg_ean];
                let sm = self.inputs.swissmedic_packages.get(&pkg.swissmedic_number8);
                let prodno = sm.map(|s| s.prodno.clone()).unwrap_or_default();
                if prodno.is_empty() {
                    continue;
                }
                // ATC starting with Q are veterinary — skipped like Ruby.
                let atc = first_non_empty(&[
                    item.atc_code.as_str(),
                    sm.map(|s| s.atc_code.as_str()).unwrap_or(""),
                ]);
                if atc.starts_with('Q') || atc.starts_with('q') {
                    continue;
                }
                // Record the referenced limitation regardless of whether the
                // PRODUCT was already emitted for this prodno.
                if let Some(lim) = pkg.limitations.first() {
                    let code = lim_code(lim);
                    if !code.is_empty() {
                        used_lims.insert(code.to_string());
                    }
                }
                if !emitted.insert(prodno.clone()) {
                    continue;
                }

                let refdata = self
                    .inputs
                    .refdata_pharma
                    .get(pkg_ean)
                    .or_else(|| self.inputs.refdata_nonpharma.get(pkg_ean));
                let combined_de = format!("{} {}", item.name_de, item.desc_de);
                let dscr = first_non_empty(&[
                    combined_de.trim(),
                    refdata.map(|r| r.desc_de.as_str()).unwrap_or(""),
                    sm.map(|s| s.sequence_name.as_str()).unwrap_or(""),
                    item.desc_de.as_str(),
                ]);
                let combined_fr = format!("{} {}", item.name_fr, item.desc_fr);
                let dscrf = first_non_empty(&[
                    combined_fr.trim(),
                    refdata.map(|r| r.desc_fr.as_str()).unwrap_or(""),
                ]);
                let combined_it = format!("{} {}", item.name_it, item.desc_it);
                let dscri = combined_it.trim().to_string();

                let mut nodes: Vec<Node> = Vec::with_capacity(8);
                nodes.push(Node::leaf("PRODNO", prodno.clone()));
                nodes.push(Node::leaf("SALECD", "A"));
                nodes.push(Node::leaf("DSCR", dscr));
                nodes.push(Node::leaf("DSCRF", dscrf));
                if self.opts.italian && !dscri.is_empty() {
                    nodes.push(Node::leaf("DSCRI", dscri));
                }
                if !atc.is_empty() {
                    nodes.push(Node::leaf("ATC", atc));
                }
                if let Some(lim) = pkg.limitations.first() {
                    let code = lim_code(lim);
                    if !code.is_empty() {
                        nodes.push(Node::leaf("LIMNAMEBAG", code.to_string()));
                    }
                }
                if let Some(sub) = substance_value(item) {
                    nodes.push(Node::leaf("SUBSTANCE", sub));
                }
                by_prodno.push((prodno, nodes));
            }
        }

        // Swissmedic-only products (registered but absent from BAG/SL).
        let mut sm_keys: Vec<&String> = self.inputs.swissmedic_packages.keys().collect();
        sm_keys.sort();
        for no8 in sm_keys {
            let sm = &self.inputs.swissmedic_packages[no8];
            if sm.is_tier || sm.prodno.is_empty() || sm.sequence_name.is_empty() {
                continue;
            }
            if sm.atc_code.starts_with('Q') || sm.atc_code.starts_with('q') {
                continue;
            }
            if !emitted.insert(sm.prodno.clone()) {
                continue;
            }
            let mut nodes: Vec<Node> = Vec::with_capacity(5);
            nodes.push(Node::leaf("PRODNO", sm.prodno.clone()));
            nodes.push(Node::leaf("SALECD", "A"));
            nodes.push(Node::leaf("DSCR", sm.sequence_name.clone()));
            nodes.push(Node::leaf("DSCRF", String::new()));
            if !sm.atc_code.is_empty() {
                nodes.push(Node::leaf("ATC", sm.atc_code.clone()));
            }
            if !sm.substance_swissmedic.is_empty() {
                nodes.push(Node::leaf("SUBSTANCE", sm.substance_swissmedic.clone()));
            }
            by_prodno.push((sm.prodno.clone(), nodes));
        }

        by_prodno.sort_by(|a, b| a.0.cmp(&b.0));
        (by_prodno.into_iter().map(|(_, n)| n).collect(), used_lims)
    }

    /// `<LIMITATIONS>` — one `<LIMITATION>` per referenced code, sorted.
    fn artikelstamm_limitations(&self, used: &HashSet<String>) -> Vec<Vec<Node>> {
        // code -> limitation (first with a non-empty German text wins).
        let mut by_code: HashMap<String, &crate::extractor::BagLimitation> = HashMap::new();
        for item in self.inputs.bag.values() {
            for pkg in item.packages.values() {
                for lim in &pkg.limitations {
                    let code = lim_code(lim);
                    if code.is_empty() || !used.contains(code) {
                        continue;
                    }
                    let keep_existing = matches!(
                        by_code.get(code),
                        Some(prev) if !prev.desc_de.is_empty()
                    );
                    if !keep_existing {
                        by_code.insert(code.to_string(), lim);
                    }
                }
            }
        }
        let mut codes: Vec<&String> = by_code.keys().collect();
        codes.sort();
        codes
            .into_iter()
            .map(|code| {
                let lim = by_code[code];
                let pts = if lim.value.len() > 1 {
                    lim.value.clone()
                } else {
                    "1".to_string()
                };
                let mut nodes = vec![
                    Node::leaf("LIMNAMEBAG", code.clone()),
                    Node::leaf("DSCR", lim.desc_de.clone()),
                    Node::leaf("DSCRF", lim.desc_fr.clone()),
                ];
                if self.opts.italian && !lim.desc_it.is_empty() {
                    nodes.push(Node::leaf("DSCRI", lim.desc_it.clone()));
                }
                nodes.push(Node::leaf("LIMITATION_PTS", pts));
                nodes
            })
            .collect()
    }

    /// `<ITEMS>` — union of every known GTIN, pharma packs first.
    fn artikelstamm_items(&self, version: u8) -> Vec<Item> {
        // Index bag packs by pack-GTIN for O(1) classification.
        let mut pack_by_ean: HashMap<&str, (&BagItem, &BagPackage)> = HashMap::new();
        for item in self.inputs.bag.values() {
            for (ean, pkg) in &item.packages {
                pack_by_ean.entry(ean.as_str()).or_insert((item, pkg));
            }
        }
        // Index Swissmedic packs by pack-GTIN, so a Swissmedic-registered pack
        // absent from BAG/Refdata/ZurRose can still be emitted (oddb2xml does
        // this via `obj = @packs[no8]` — every Swissmedic pack becomes an ITEM).
        let mut sm_by_ean: HashMap<&str, &crate::extractor::swissmedic::SwissmedicPackage> =
            HashMap::new();
        for sm in self.inputs.swissmedic_packages.values() {
            if !sm.ean13.is_empty() {
                sm_by_ean.entry(sm.ean13.as_str()).or_insert(sm);
            }
        }

        // Union of every GTIN across the sources.
        let mut gtins: HashSet<String> = HashSet::new();
        gtins.extend(pack_by_ean.keys().map(|s| s.to_string()));
        gtins.extend(self.inputs.refdata_pharma.keys().cloned());
        gtins.extend(self.inputs.refdata_nonpharma.keys().cloned());
        gtins.extend(self.inputs.zurrose.keys().cloned());
        for sm in self.inputs.swissmedic_packages.values() {
            if !sm.ean13.is_empty() {
                gtins.insert(sm.ean13.clone());
            }
        }
        let mut gtins: Vec<String> = gtins.into_iter().collect();
        gtins.sort();

        let mut out: Vec<Item> = Vec::new();
        let mut emitted: HashSet<String> = HashSet::new();
        for gtin in &gtins {
            if gtin.is_empty() || gtin.chars().all(|c| c == '0') {
                continue;
            }
            // Skip synthetic 999999+pharmacode placeholders for ZurRose
            // articles that have no real EAN13 — oddb2xml drops these from the
            // Artikelstamm too (`next if /^999999/`, FAKE_GTIN_START).
            if gtin.starts_with(crate::util::FAKE_GTIN_START) {
                continue;
            }
            if !emitted.insert(gtin.clone()) {
                continue;
            }
            let built = if let Some((item, pkg)) = pack_by_ean.get(gtin.as_str()) {
                Some(self.pharma_item(gtin, item, pkg, version))
            } else if let Some(sm) = sm_by_ean.get(gtin.as_str()) {
                // Swissmedic-registered pack absent from the BAG SL feed:
                // build the full pharma ITEM by merging the register with
                // Refdata + ZurRose — Ruby routes every GTIN whose no8 is in
                // Packungen.xlsx through the pharma branch via
                // `obj = @packs[no8].merge(obj)`, so these carry COMP/NAME,
                // prices, PKG_SIZE, MEASURE, DOSAGE_FORM(F), IKSCAT and
                // PRODNO (e.g. TWINRIX, dropped from the July 2026 SL feed).
                self.swissmedic_item(gtin, sm)
            } else if let Some(it) = self.nonpharma_item(gtin) {
                Some(it)
            } else {
                None
            };
            // Global veterinary filter: no "ad us vet" article reaches any
            // output, whatever source it came from (issue: no vet data at all).
            if let Some(it) = built {
                if !is_veterinary_name(&it.csv[1]) {
                    out.push(it);
                }
            }
        }
        out
    }

    /// A pharma `<ITEM>` built from a BAG pack.
    fn pharma_item(
        &self,
        gtin: &str,
        item: &BagItem,
        pkg: &BagPackage,
        version: u8,
    ) -> Item {
        let zr = self.inputs.zurrose.get(gtin);
        let refdata = self
            .inputs
            .refdata_pharma
            .get(gtin)
            .or_else(|| self.inputs.refdata_nonpharma.get(gtin));
        let sm = self.inputs.swissmedic_packages.get(&pkg.swissmedic_number8);

        let dscr = first_non_empty(&[
            refdata.map(|r| r.desc_de.as_str()).unwrap_or(""),
            sm.map(|s| s.sequence_name.as_str()).unwrap_or(""),
            pkg.desc_de.as_str(),
            pkg.name_de.as_str(),
            item.desc_de.as_str(),
            item.name_de.as_str(),
        ]);
        let dscrf = first_non_empty(&[
            refdata.map(|r| r.desc_fr.as_str()).unwrap_or(""),
            pkg.desc_fr.as_str(),
            item.desc_fr.as_str(),
        ]);
        let dscri = first_non_empty(&[
            refdata.map(|r| r.desc_it.as_str()).unwrap_or(""),
            pkg.desc_it.as_str(),
            item.desc_it.as_str(),
        ]);

        let mut nodes: Vec<Node> = Vec::with_capacity(20);
        nodes.push(Node::leaf("GTIN", rjust13(gtin)));
        if let Some(zr) = zr {
            if !zr.pharmacode.is_empty() {
                nodes.push(Node::leaf("PHAR", zr.pharmacode.clone()));
            }
        }
        nodes.push(Node::leaf("SALECD", "A"));
        nodes.push(Node::leaf("DSCR", dscr));
        nodes.push(Node::leaf("DSCRF", dscrf));
        if self.opts.italian && !dscri.is_empty() {
            nodes.push(Node::leaf("DSCRI", dscri));
        }

        // <COMP> manufacturer (refdata GLN + name; Swissmedic name fallback).
        let comp_name = first_non_empty(&[
            refdata.map(|r| r.company_name.as_str()).unwrap_or(""),
            sm.map(|s| s.company_name.as_str()).unwrap_or(""),
        ]);
        let comp_gln = refdata.map(|r| r.company_ean.clone()).unwrap_or_default();
        if !comp_name.is_empty() || !comp_gln.is_empty() {
            let mut comp: Vec<Node> = Vec::new();
            if !comp_name.is_empty() {
                comp.push(Node::leaf("NAME", truncate(&comp_name, 100)));
            }
            if !comp_gln.is_empty() {
                comp.push(Node::leaf("GLN", comp_gln));
            }
            nodes.push(Node::nested("COMP", comp));
        }

        // Prices: BAG SL first, ZurRose fallback.
        let pexf = first_non_empty(&[
            pkg.prices.exf_price.price.as_str(),
            zr.map(|z| z.price.as_str()).unwrap_or(""),
        ]);
        let ppub = first_non_empty(&[
            pkg.prices.pub_price.price.as_str(),
            zr.map(|z| z.pub_price.as_str()).unwrap_or(""),
        ]);
        // Like Ruby's pharma branch, "0.00" is a value, not an absence — the
        // June Artikelstamm carried <PPUB>0.00</PPUB> on ZurRose-priced packs.
        if !pexf.is_empty() {
            nodes.push(Node::leaf("PEXF", pexf));
        }
        if !ppub.is_empty() {
            nodes.push(Node::leaf("PPUB", ppub));
        }

        // Pack size / measure / galenic form from Swissmedic.
        if let Some(sm) = sm {
            if let Ok(n) = sm.package_size.trim().parse::<i64>() {
                nodes.push(Node::leaf("PKG_SIZE", n.to_string()));
            }
            if !sm.einheit_swissmedic.is_empty() {
                nodes.push(Node::leaf("MEASURE", sm.einheit_swissmedic.clone()));
            }
            let (form, _, _) = galenic_for(&sm.sequence_name, &sm.einheit_swissmedic);
            if !form.is_empty() {
                nodes.push(Node::leaf("DOSAGE_FORM", form.clone()));
                if let Some(fr) = crate::calc::form_fr(&form) {
                    nodes.push(Node::leaf("DOSAGE_FORMF", fr));
                }
            }
        }

        if pkg.sl_entry {
            nodes.push(Node::leaf("SL_ENTRY", "true"));
        }
        // IKSCAT: XSD restricts to A-E.
        if let Some(c) = pkg.swissmedic_category.chars().next() {
            if ('A'..='E').contains(&c) {
                nodes.push(Node::leaf("IKSCAT", c.to_string()));
            }
        }
        // GENERIC_TYPE: XSD restricts to O/G/K.
        if let Some(c) = item.org_gen_code.chars().next() {
            if matches!(c, 'O' | 'G' | 'K') {
                nodes.push(Node::leaf("GENERIC_TYPE", c.to_string()));
            }
        }
        if self.inputs.lppv_ean13s.contains_key(gtin) {
            nodes.push(Node::leaf("LPPV", "true"));
        }
        if let Some(d) = deductible(&item.deductible, &item.deductible20) {
            nodes.push(Node::leaf("DEDUCTIBLE", d.to_string()));
        }
        let atc = sm
            .map(|s| s.atc_code.clone())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| item.atc_code.clone());
        let mut prodno = sm.map(|s| s.prodno.clone()).unwrap_or_default();
        if prodno.is_empty() {
            if let Some(p) = self.vaccination_prodno(&atc) {
                prodno = p;
            }
        }
        if !prodno.is_empty() {
            nodes.push(Node::leaf("PRODNO", prodno.clone()));
        }

        // v6 <ARTSL>: BAG Indikationscodes.  Skipped on legacy v5.
        if version != 5 {
            if let Some(artsl) = artsl_node(pkg) {
                nodes.push(artsl);
            }
        }

        // Companion CSV row (matches the Ruby pharma row).  Uses Swissmedic's
        // raw pack size / substance / prodno rather than the XML-normalised
        // node values, exactly as the previous CSV did.
        let csv_name = first_non_empty(&[
            refdata.map(|r| r.desc_de.as_str()).unwrap_or(""),
            sm.map(|s| s.sequence_name.as_str()).unwrap_or(""),
            pkg.desc_de.as_str(),
            item.name_de.as_str(),
        ]);
        let (csv_form, _, _) = galenic_for(
            &csv_name,
            sm.map(|s| s.einheit_swissmedic.as_str()).unwrap_or(""),
        );
        let csv = vec![
            rjust13(gtin),
            csv_name,
            sm.map(|s| s.package_size.clone()).unwrap_or_default(),
            csv_form,
            pkg.prices.exf_price.price.clone(),
            pkg.prices.pub_price.price.clone(),
            prodno,
            atc,
            sm.map(|s| s.substance_swissmedic.clone()).unwrap_or_default(),
            item.org_gen_code.clone(),
            sm.map(|s| s.ith_swissmedic.clone())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| item.it_code.clone()),
            if pkg.sl_entry { "SL".to_string() } else { String::new() },
        ];

        Item {
            pharmatype: "P",
            children: nodes,
            csv,
        }
    }

    /// A non-pharma (or refdata/ZurRose-only) `<ITEM>` for a GTIN that is
    /// not in any BAG pack.  Returns `None` when the GTIN should be skipped.
    fn nonpharma_item(&self, gtin: &str) -> Option<Item> {
        let refdata = self
            .inputs
            .refdata_nonpharma
            .get(gtin)
            .or_else(|| self.inputs.refdata_pharma.get(gtin));
        let zr = self.inputs.zurrose.get(gtin);
        if refdata.is_none() && zr.is_none() {
            return None;
        }
        // Skip inactive Swissmedic (7680) rows that only ZurRose knows.
        if refdata.is_none() && rjust13(gtin).starts_with("7680") {
            if let Some(z) = zr {
                if z.data_origin == "zur_rose" {
                    return None;
                }
            }
        }

        let pharmatype = if rjust13(gtin).starts_with("7680") {
            "P"
        } else {
            "N"
        };
        let dscr = first_non_empty(&[
            refdata.map(|r| r.desc_de.as_str()).unwrap_or(""),
            zr.map(|z| z.description.as_str()).unwrap_or(""),
        ]);
        let dscrf = refdata.map(|r| r.desc_fr.clone()).unwrap_or_default();
        let dscri = refdata.map(|r| r.desc_it.clone()).unwrap_or_default();
        let csv_name = dscr.clone();

        let mut nodes: Vec<Node> = Vec::with_capacity(10);
        nodes.push(Node::leaf("GTIN", rjust13(gtin)));
        if let Some(z) = zr {
            if !z.pharmacode.is_empty() {
                nodes.push(Node::leaf("PHAR", z.pharmacode.clone()));
            }
        }
        // SALECD: active unless ZurRose flags cmut == "3".
        let salecd = match zr {
            Some(z) if z.cmut == "3" => "I",
            _ => "A",
        };
        nodes.push(Node::leaf("SALECD", salecd));
        nodes.push(Node::leaf("DSCR", dscr));
        nodes.push(Node::leaf("DSCRF", dscrf));
        if self.opts.italian && !dscri.is_empty() {
            nodes.push(Node::leaf("DSCRI", dscri));
        }
        if let Some(r) = refdata {
            if !r.company_ean.is_empty() {
                nodes.push(Node::nested("COMP", vec![Node::leaf("GLN", r.company_ean.clone())]));
            }
        }
        let csv_pexf = zr
            .map(|z| z.price.clone())
            .filter(|p| !p.is_empty() && p != "0.00")
            .unwrap_or_default();
        let mut csv_ppub = zr
            .map(|z| z.pub_price.clone())
            .filter(|p| !p.is_empty() && p != "0.00")
            .unwrap_or_default();

        // Weleda / WALA Kapitel-70 SL recovery (issue #121): for a complementary
        // medicine missing from the FHIR feed, restore the SL flag and fill a
        // blank public price from the BAG group price. The FHIR/ZurRose price
        // always wins — the group price only fills a gap.
        let weleda = self.inputs.weleda_sl.get(&rjust13(gtin));
        if let Some(w) = weleda {
            if csv_ppub.is_empty() {
                if let Some(p) = &w.price {
                    csv_ppub = p.clone();
                }
            }
        }

        if !csv_pexf.is_empty() {
            nodes.push(Node::leaf("PEXF", csv_pexf.clone()));
        }
        if !csv_ppub.is_empty() {
            nodes.push(Node::leaf("PPUB", csv_ppub.clone()));
        }
        let sl_recovered = weleda.is_some();
        if sl_recovered {
            nodes.push(Node::leaf("SL_ENTRY", "true"));
        }

        // Companion CSV row (matches the Ruby non-pharma row): gtin, name and
        // the two prices; the pharma-only columns stay empty, and sl-liste is
        // "SL" for a recovered Kapitel-70 product.
        let csv = vec![
            rjust13(gtin),
            csv_name,
            String::new(),
            String::new(),
            csv_pexf,
            csv_ppub,
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            String::new(),
            if sl_recovered { "SL".to_string() } else { String::new() },
        ];
        Some(Item {
            pharmatype,
            children: nodes,
            csv,
        })
    }

    /// A pharma `<ITEM>` for a Swissmedic-registered pack that is absent from
    /// the BAG SL feed.  Mirrors oddb2xml's pharma branch, which routes every
    /// GTIN whose no8 is in Packungen.xlsx through `obj = @packs[no8].merge(obj)`
    /// — the register supplies PKG_SIZE / MEASURE / DOSAGE_FORM(F) / IKSCAT /
    /// PRODNO / company NAME while Refdata contributes the descriptions and the
    /// company GLN and ZurRose the pharmacode and prices.  Returns `None` for
    /// veterinary packs (oddb2xml skips `Tierarzneimittel` / "ad us vet").
    fn swissmedic_item(
        &self,
        gtin: &str,
        sm: &crate::extractor::swissmedic::SwissmedicPackage,
    ) -> Option<Item> {
        // Structural veterinary markers only present on the Swissmedic record;
        // the name-based "ad us vet" filter is applied globally in the caller.
        if sm.is_tier || sm.list_code.contains("Tierarzneimittel") {
            return None;
        }
        let refdata = self
            .inputs
            .refdata_pharma
            .get(gtin)
            .or_else(|| self.inputs.refdata_nonpharma.get(gtin));
        let zr = self.inputs.zurrose.get(gtin);

        let name = first_non_empty(&[
            refdata.map(|r| r.desc_de.as_str()).unwrap_or(""),
            zr.map(|z| z.description.as_str()).unwrap_or(""),
            sm.sequence_name.as_str(),
        ]);
        let dscrf = refdata.map(|r| r.desc_fr.clone()).unwrap_or_default();
        let dscri = refdata.map(|r| r.desc_it.clone()).unwrap_or_default();

        let mut nodes: Vec<Node> = Vec::with_capacity(18);
        nodes.push(Node::leaf("GTIN", rjust13(gtin)));
        if let Some(z) = zr {
            if !z.pharmacode.is_empty() {
                nodes.push(Node::leaf("PHAR", z.pharmacode.clone()));
            }
        }
        // Ruby's pharma branch hard-codes SALECD "A" for register-backed packs.
        nodes.push(Node::leaf("SALECD", "A"));
        nodes.push(Node::leaf("DSCR", name.clone()));
        nodes.push(Node::leaf("DSCRF", dscrf));
        if self.opts.italian && !dscri.is_empty() {
            nodes.push(Node::leaf("DSCRI", dscri));
        }

        // <COMP> manufacturer (refdata GLN + name; Swissmedic name fallback).
        let comp_name = first_non_empty(&[
            refdata.map(|r| r.company_name.as_str()).unwrap_or(""),
            sm.company_name.as_str(),
        ]);
        let comp_gln = refdata.map(|r| r.company_ean.clone()).unwrap_or_default();
        if !comp_name.is_empty() || !comp_gln.is_empty() {
            let mut comp: Vec<Node> = Vec::new();
            if !comp_name.is_empty() {
                comp.push(Node::leaf("NAME", truncate(&comp_name, 100)));
            }
            if !comp_gln.is_empty() {
                comp.push(Node::leaf("GLN", comp_gln));
            }
            nodes.push(Node::nested("COMP", comp));
        }

        // Prices from ZurRose (no BAG SL entry by definition here).  Like the
        // Ruby pharma branch, "0.00" is emitted as-is (June TWINRIX carried
        // <PPUB>0.00</PPUB>); the Weleda/WALA Kapitel-70 group price fills a
        // blank public price and restores the SL flag (issue #121).
        let pexf = zr.map(|z| z.price.clone()).unwrap_or_default();
        let mut ppub = zr.map(|z| z.pub_price.clone()).unwrap_or_default();
        let weleda = self.inputs.weleda_sl.get(&rjust13(gtin));
        if let Some(w) = weleda {
            if ppub.is_empty() {
                if let Some(p) = &w.price {
                    ppub = p.clone();
                }
            }
        }
        if !pexf.is_empty() {
            nodes.push(Node::leaf("PEXF", pexf.clone()));
        }
        if !ppub.is_empty() {
            nodes.push(Node::leaf("PPUB", ppub.clone()));
        }

        // Pack size / measure / galenic form from Swissmedic.
        if let Ok(n) = sm.package_size.trim().parse::<i64>() {
            nodes.push(Node::leaf("PKG_SIZE", n.to_string()));
        }
        if !sm.einheit_swissmedic.is_empty() {
            nodes.push(Node::leaf("MEASURE", sm.einheit_swissmedic.clone()));
        }
        let (form, _, _) = galenic_for(&sm.sequence_name, &sm.einheit_swissmedic);
        if !form.is_empty() {
            nodes.push(Node::leaf("DOSAGE_FORM", form.clone()));
            if let Some(fr) = crate::calc::form_fr(&form) {
                nodes.push(Node::leaf("DOSAGE_FORMF", fr));
            }
        }
        if weleda.is_some() {
            nodes.push(Node::leaf("SL_ENTRY", "true"));
        }
        // IKSCAT: XSD restricts to A-E.
        if let Some(c) = sm.swissmedic_category.chars().next() {
            if ('A'..='E').contains(&c) {
                nodes.push(Node::leaf("IKSCAT", c.to_string()));
            }
        }
        if self.inputs.lppv_ean13s.contains_key(gtin) {
            nodes.push(Node::leaf("LPPV", "true"));
        }
        let mut prodno = sm.prodno.clone();
        if prodno.is_empty() {
            if let Some(p) = self.vaccination_prodno(&sm.atc_code) {
                prodno = p;
            }
        }
        if !prodno.is_empty() {
            nodes.push(Node::leaf("PRODNO", prodno.clone()));
        }

        // 7680-registered packs are pharma; anything else non-pharma.
        let pharmatype = if rjust13(gtin).starts_with("7680") {
            "P"
        } else {
            "N"
        };

        // Companion CSV row (matches the Ruby pharma row): PRODNO/ATC/substance/
        // IT-code from Swissmedic, prices from ZurRose, SL only via Weleda.
        let csv = vec![
            rjust13(gtin),
            name,
            sm.package_size.clone(),
            form,
            pexf,
            ppub,
            prodno,
            sm.atc_code.clone(),
            sm.substance_swissmedic.clone(),
            String::new(),
            sm.ith_swissmedic.clone(),
            if weleda.is_some() { "SL".to_string() } else { String::new() },
        ];

        Some(Item {
            pharmatype,
            children: nodes,
            csv,
        })
    }

    /// Ruby's vaccination PRODNO patch: an item without a PRODNO whose ATC is
    /// a vaccine code (`^J07`, but not the "other" group `J07AX`) borrows the
    /// PRODNO of a Swissmedic pack with the same ATC, so the Elexis
    /// vaccination list can still map it to a product.  Deterministic pick:
    /// the pack with the smallest no8 (first Packungen.xlsx row).
    fn vaccination_prodno(&self, atc: &str) -> Option<String> {
        let atc = atc.trim();
        if atc.is_empty() || !atc.starts_with("J07") || atc.starts_with("J07AX") {
            return None;
        }
        self.inputs
            .swissmedic_packages
            .values()
            .filter(|p| p.atc_code == atc && !p.prodno.is_empty())
            .min_by(|a, b| a.no8.cmp(&b.no8))
            .map(|p| p.prodno.clone())
    }
}

/// The BAG limitation code used as `<LIMNAMEBAG>` / `<LIMCD>`.  FHIR carries
/// no native LIMCD, so — mirroring oddb2xml's `LimitationCode = cud_ref` — the
/// ClinicalUseDefinition id (`cud_ref`) identifies each limitation text.
/// Falls back to the legacy `code` field for the non-FHIR (BAG-XML) path.
fn lim_code(lim: &crate::extractor::BagLimitation) -> &str {
    if !lim.code.is_empty() {
        &lim.code
    } else {
        &lim.cud_ref
    }
}

/// Veterinary marker: oddb2xml drops any article whose German description
/// contains "ad us vet" (ad usum veterinarium).  We want no veterinary data
/// in the Artikelstamm at all, so this is applied to every emitted item.
fn is_veterinary_name(name: &str) -> bool {
    name.to_lowercase().contains("ad us vet")
}

/// Zero-pad a GTIN to 13 chars, matching Ruby's `.rjust(13, "0")`.
fn rjust13(gtin: &str) -> String {
    if gtin.len() >= 13 {
        gtin.to_string()
    } else {
        format!("{:0>13}", gtin)
    }
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// SUBSTANCE value: "Verschiedene Kombinationen" for >1 substance, else the
/// single substance name; `None` when there are none.
fn substance_value(item: &BagItem) -> Option<String> {
    let named: Vec<&str> = item
        .substances
        .iter()
        .map(|s| s.name.as_str())
        .filter(|n| !n.is_empty())
        .collect();
    match named.len() {
        0 => None,
        1 => Some(named[0].to_string()),
        _ => Some("Verschiedene Kombinationen".to_string()),
    }
}

/// DEDUCTIBLE percent from the BAG `deductible` ("Y"->40, "N"->10) with the
/// legacy `deductible20` fallback.
fn deductible(deductible: &str, deductible20: &str) -> Option<u32> {
    match deductible {
        "Y" => return Some(40),
        "N" => return Some(10),
        _ => {}
    }
    match deductible20 {
        "Y" => Some(40),
        "N" => Some(10),
        _ => None,
    }
}

/// Build the v6 `<ARTSL>` node from a pack's limitations that carry a BAG
/// Indikationscode, deduped by (code, indcd).  `None` when none apply.
fn artsl_node(pkg: &BagPackage) -> Option<Node> {
    let mut seen: HashSet<(String, String)> = HashSet::new();
    let mut artlims: Vec<Node> = Vec::new();
    for lim in &pkg.limitations {
        let indcd = lim.indication_code.trim();
        if indcd.is_empty() {
            continue;
        }
        if !seen.insert((lim.code.clone(), indcd.to_string())) {
            continue;
        }
        let mut fields = vec![
            Node::leaf("LIMCD", lim_code(lim).to_string()),
            Node::leaf("INDCD", indcd.to_string()),
        ];
        if let Some(vdat) = elexis_datetime(&lim.vdate) {
            fields.push(Node::leaf("VDAT", vdat));
        }
        artlims.push(Node::nested("ARTLIM", fields));
    }
    if artlims.is_empty() {
        return None;
    }
    Some(Node::nested(
        "ARTSL",
        vec![
            Node::leaf("PM", "true"),
            Node::nested("ARTLIMS", artlims),
        ],
    ))
}

/// Normalise a BAG validity date to an xs:dateTime: a bare `YYYY-MM-DD`
/// gets midnight appended; a full timestamp passes through; blank -> None.
fn elexis_datetime(s: &str) -> Option<String> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if s.len() == 10 && s.as_bytes().iter().enumerate().all(|(i, &b)| {
        if i == 4 || i == 7 {
            b == b'-'
        } else {
            b.is_ascii_digit()
        }
    }) {
        Some(format!("{s}T00:00:00"))
    } else {
        Some(s.to_string())
    }
}

/// Minimal CSV field escaping (RFC 4180): quote when the field contains a
/// comma, quote or newline; double interior quotes.
fn csv_escape(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Serialise one `Node`.  Unlike the shared `builder::write_node`, an empty
/// leaf/nested is emitted self-closing (`<DSCRF/>`) so the indenting writer
/// doesn't inject whitespace text into an otherwise-empty element — matching
/// the Nokogiri `<DSCRF/>` shape the Ruby Artikelstamm produces.
fn emit_node(writer: &mut Writer<Cursor<Vec<u8>>>, node: &Node) -> Result<()> {
    match node {
        Node::Leaf(tag, text) => {
            if text.is_empty() {
                writer.write_event(Event::Empty(BytesStart::new(tag.as_str())))?;
            } else {
                writer.write_event(Event::Start(BytesStart::new(tag.as_str())))?;
                writer.write_event(Event::Text(BytesText::new(text)))?;
                writer.write_event(Event::End(BytesEnd::new(tag.as_str())))?;
            }
        }
        Node::Nested(tag, children) => {
            if children.is_empty() {
                writer.write_event(Event::Empty(BytesStart::new(tag.as_str())))?;
            } else {
                writer.write_event(Event::Start(BytesStart::new(tag.as_str())))?;
                for child in children {
                    emit_node(writer, child)?;
                }
                writer.write_event(Event::End(BytesEnd::new(tag.as_str())))?;
            }
        }
    }
    Ok(())
}
