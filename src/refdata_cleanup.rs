//! Compensates for known data-quality issues in upstream
//! Refdata.Articles.xml before they reach the generated output.
//!
//! Each fix is opt-in and guarded by a heuristic against Swissmedic
//! data so we never alter genuine combination products.  See GitHub
//! issue [zdavatz/oddb2xml#112][1] for the catalogue of upstream
//! problems and the parallel fix in `oddb2xml` (Ruby) 3.0.5.
//!
//! [1]: https://github.com/zdavatz/oddb2xml/issues/112
//!
//! Currently active rules:
//!
//! * **Doubled dose token** — Refdata sometimes emits the strength
//!   twice in `<FullName>`, e.g.
//!   `MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette`.  When the
//!   matching Swissmedic entry shows a single active substance, the
//!   duplicate token is collapsed to a single occurrence.  Real
//!   combination products like
//!   `PHESGO Inj Lös 600 mg/600 mg/10 ml Durchstf`
//!   (pertuzumab + trastuzumab) are detected via the comma in
//!   `substance_swissmedic` and left untouched.

use crate::builder::Inputs;
use crate::extractor::RefdataItem;
use once_cell::sync::Lazy;
use regex::Regex;

/// Matches `<dose> / <dose> /` where the two dose tokens are captured
/// separately.  Rust's `regex` crate is RE2-style and rejects
/// backreferences, so equality of the two captures is checked in the
/// `fix_double_dose` callback — combined with `single_substance` this
/// is safe even for real combinations that happen to share the same
/// numeric strength.
static DOUBLE_DOSE_RE: Lazy<Regex> = Lazy::new(|| {
    let token = r"\d+(?:[.,]\d+)?\s*(?:mg|µg|mcg|g|ml|UI|U\.I\.|IE|%)";
    Regex::new(&format!(r"({token})\s*/\s*({token})\s*/\s*"))
        .expect("valid double-dose regex")
});

/// A Swissmedic compositions cell like `mirtazapinum` indicates a mono
/// product; `atovaquonum, proguanili hydrochloridum` or
/// `pertuzumabum, trastuzumabum` indicates a real combination.
pub fn single_substance(swissmedic_substance: &str) -> bool {
    let trimmed = swissmedic_substance.trim();
    !trimmed.is_empty() && !trimmed.contains(',')
}

/// Returns the cleaned description; if no rule applies, returns
/// `desc` unchanged (cloned).
pub fn fix_double_dose(desc: &str, swissmedic_substance: &str) -> String {
    if desc.is_empty() {
        return String::new();
    }
    if !single_substance(swissmedic_substance) {
        return desc.to_string();
    }
    DOUBLE_DOSE_RE
        .replace(desc, |caps: &regex::Captures| {
            let first = caps.get(1).map(|m| m.as_str().trim()).unwrap_or("");
            let second = caps.get(2).map(|m| m.as_str().trim()).unwrap_or("");
            if first == second && !first.is_empty() {
                format!("{} / ", caps.get(1).unwrap().as_str())
            } else {
                caps.get(0).unwrap().as_str().to_string()
            }
        })
        .into_owned()
}

/// Walks every Refdata item and applies the active cleanup rules in
/// place.  Idempotent — running twice is a no-op on already-clean data.
pub fn apply(inputs: &mut Inputs) {
    let mut fixed = 0usize;
    fixed += clean_map(&mut inputs.refdata_pharma, &inputs.swissmedic_packages);
    fixed += clean_map(&mut inputs.refdata_nonpharma, &inputs.swissmedic_packages);
    if fixed > 0 {
        crate::util::log(format!(
            "Refdata cleanup: fixed double-dose pattern in {fixed} description(s)"
        ));
    }
}

fn clean_map(
    refdata: &mut std::collections::HashMap<String, RefdataItem>,
    packs: &std::collections::HashMap<String, crate::extractor::SwissmedicPackage>,
) -> usize {
    let mut fixed = 0usize;
    for item in refdata.values_mut() {
        if item.no8.is_empty() {
            continue;
        }
        let pack = match packs.get(&item.no8) {
            Some(p) => p,
            None => continue,
        };
        let substance = pack.substance_swissmedic.as_str();
        for field in [&mut item.desc_de, &mut item.desc_fr, &mut item.desc_it] {
            let cleaned = fix_double_dose(field, substance);
            if &cleaned != field {
                *field = cleaned;
                fixed += 1;
            }
        }
    }
    fixed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extractor::SwissmedicPackage;

    fn pack_with(substance: &str) -> SwissmedicPackage {
        SwissmedicPackage {
            substance_swissmedic: substance.into(),
            ..Default::default()
        }
    }

    #[test]
    fn single_substance_distinguishes_mono_from_combo() {
        assert!(single_substance("mirtazapinum"));
        assert!(single_substance("methotrexatum"));
        assert!(!single_substance("pertuzumabum, trastuzumabum"));
        assert!(!single_substance("atovaquonum, proguanili hydrochloridum"));
        assert!(!single_substance(""));
        assert!(!single_substance("   "));
    }

    #[test]
    fn fix_double_dose_collapses_duplicate_for_mono() {
        let input = "MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette";
        let expected = "MIRTAZAPIN Sandoz eco 30 mg / 100 Tablette";
        assert_eq!(fix_double_dose(input, "mirtazapinum"), expected);
    }

    #[test]
    fn fix_double_dose_handles_icatibant_spacing() {
        let input = "ICATIBANT Spirig HC 30 mg / 30 mg / 1 x 3 ml";
        let expected = "ICATIBANT Spirig HC 30 mg / 1 x 3 ml";
        assert_eq!(fix_double_dose(input, "icatibantum"), expected);
    }

    #[test]
    fn fix_double_dose_leaves_phesgo_combo_untouched() {
        let input = "PHESGO Inj Lös 600 mg/600 mg/10 ml Durchstf";
        assert_eq!(
            fix_double_dose(input, "pertuzumabum, trastuzumabum"),
            input
        );
    }

    #[test]
    fn fix_double_dose_leaves_normal_descriptions_untouched() {
        let input = "LEVOCETIRIZIN Spirig HC Filmtabl 5 mg 10 Stk";
        assert_eq!(fix_double_dose(input, "mirtazapinum"), input);
    }

    #[test]
    fn fix_double_dose_skips_when_substance_unknown() {
        let input = "MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette";
        assert_eq!(fix_double_dose(input, ""), input);
    }

    #[test]
    fn fix_double_dose_is_noop_for_empty_input() {
        assert_eq!(fix_double_dose("", "mirtazapinum"), "");
    }

    #[test]
    fn fix_double_dose_does_not_collapse_different_doses() {
        let input = "FOO 250 mg / 100 mg / 12 Stk";
        assert_eq!(fix_double_dose(input, "atovaquonum, proguanili"), input);
    }

    #[test]
    fn apply_mutates_pharma_descriptions_in_place() {
        let mut inputs = Inputs::default();
        inputs.swissmedic_packages.insert(
            "69475006".into(),
            pack_with("mirtazapinum"),
        );
        let mut item = RefdataItem::default();
        item.ean13 = "7680694750066".into();
        item.no8 = "69475006".into();
        item.desc_de = "MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette".into();
        item.desc_fr = "MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 comprimé(".into();
        item.desc_it = String::new();
        inputs.refdata_pharma.insert(item.ean13.clone(), item);

        apply(&mut inputs);

        let cleaned = &inputs.refdata_pharma["7680694750066"];
        assert_eq!(cleaned.desc_de, "MIRTAZAPIN Sandoz eco 30 mg / 100 Tablette");
        assert_eq!(cleaned.desc_fr, "MIRTAZAPIN Sandoz eco 30 mg / 100 comprimé(");
    }

    #[test]
    fn apply_leaves_combo_products_untouched() {
        let mut inputs = Inputs::default();
        inputs.swissmedic_packages.insert(
            "67828001".into(),
            pack_with("pertuzumabum, trastuzumabum"),
        );
        let original = "PHESGO Inj Lös 600 mg/600 mg/10 ml Durchstf";
        let mut item = RefdataItem::default();
        item.ean13 = "7680678280013".into();
        item.no8 = "67828001".into();
        item.desc_de = original.into();
        inputs.refdata_pharma.insert(item.ean13.clone(), item);

        apply(&mut inputs);

        assert_eq!(inputs.refdata_pharma["7680678280013"].desc_de, original);
    }

    #[test]
    fn apply_is_idempotent() {
        let mut inputs = Inputs::default();
        inputs.swissmedic_packages.insert(
            "69475006".into(),
            pack_with("mirtazapinum"),
        );
        let mut item = RefdataItem::default();
        item.no8 = "69475006".into();
        item.desc_de = "MIRTAZAPIN Sandoz eco 30 mg / 30 mg / 100 Tablette".into();
        inputs.refdata_pharma.insert("g".into(), item);

        apply(&mut inputs);
        apply(&mut inputs);

        assert_eq!(
            inputs.refdata_pharma["g"].desc_de,
            "MIRTAZAPIN Sandoz eco 30 mg / 100 Tablette"
        );
    }
}
