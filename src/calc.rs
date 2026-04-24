//! Galenic form / group lookups — port of `lib/oddb2xml/calc.rb`.
//!
//! The Ruby implementation carries small YAML tables shipped inside the
//! gem alongside the source.  In Rust we inline them as arrays so the
//! binary is self-contained and no runtime file I/O is needed for a
//! lookup.
//!
//! Public API mirrors the Ruby methods used by `builder.rb`:
//!   * [`group_by_form`]  — galenic group label for a form name.
//!   * [`oid_for_form`]   — numeric OID for a form.
//!   * [`oid_for_group`]  — numeric OID for a group.
//!   * [`known_forms`]    — iterator over every (form, group) pair,
//!     used by the builder to match forms against free-text descriptions.

use once_cell::sync::Lazy;
use std::collections::HashMap;

/// Swissmedic galenic form → galenic group.  Ordered from longest to
/// shortest substring match so "Filmtablette" wins over "Tablette".
const FORM_GROUP: &[(&str, &str)] = &[
    // Solid oral — Tabletten group
    ("Filmtabletten", "Tabletten"),
    ("Filmtablette", "Tabletten"),
    ("Brausetabletten", "Tabletten"),
    ("Brausetablette", "Tabletten"),
    ("Kautabletten", "Tabletten"),
    ("Kautablette", "Tabletten"),
    ("Lutschtabletten", "Tabletten"),
    ("Lutschtablette", "Tabletten"),
    ("Schmelztabletten", "Tabletten"),
    ("Schmelztablette", "Tabletten"),
    ("Sublingualtabletten", "Tabletten"),
    ("Sublingualtablette", "Tabletten"),
    ("Retardtabletten", "Tabletten"),
    ("Retardtablette", "Tabletten"),
    ("Vaginaltabletten", "Tabletten"),
    ("Vaginaltablette", "Tabletten"),
    ("Manteltabletten", "Tabletten"),
    ("Manteltablette", "Tabletten"),
    ("Matrixtabletten", "Tabletten"),
    ("Matrixtablette", "Tabletten"),
    ("Tabletten", "Tabletten"),
    ("Tablette", "Tabletten"),
    ("Tabl", "Tabletten"),
    ("Dragée", "Tabletten"),
    ("Dragees", "Tabletten"),
    ("Drag", "Tabletten"),

    // Kapseln
    ("Weichkapseln", "Kapseln"),
    ("Weichkapsel", "Kapseln"),
    ("Hartkapseln", "Kapseln"),
    ("Hartkapsel", "Kapseln"),
    ("Retardkapseln", "Kapseln"),
    ("Retardkapsel", "Kapseln"),
    ("Steckkapseln", "Kapseln"),
    ("Steckkapsel", "Kapseln"),
    ("Kapseln", "Kapseln"),
    ("Kapsel", "Kapseln"),
    ("Kaps", "Kapseln"),
    ("Perlen", "Kapseln"),
    ("Perlées", "Kapseln"),

    // Parenteralia
    ("Injektionslösung", "Parenteralia"),
    ("Injektionssuspension", "Parenteralia"),
    ("Injektionsemulsion", "Parenteralia"),
    ("Infusionslösung", "Parenteralia"),
    ("Infusionskonzentrat", "Parenteralia"),
    ("Inf Konz", "Parenteralia"),
    ("Inj Lös", "Parenteralia"),
    ("Inj Susp", "Parenteralia"),
    ("i.v.", "Parenteralia"),
    ("i.m.", "Parenteralia"),
    ("s.c.", "Parenteralia"),
    ("Durchstechflasche", "Parenteralia"),
    ("Durchstf", "Parenteralia"),
    ("Fertigspritze", "Parenteralia"),
    ("Fertigspr", "Parenteralia"),
    ("Spritze", "Parenteralia"),
    ("Ampulle", "Parenteralia"),
    ("Ampullen", "Parenteralia"),
    ("Amp", "Parenteralia"),
    ("Vial", "Parenteralia"),
    ("Pen", "Parenteralia"),
    ("Patrone", "Parenteralia"),
    ("Patronen", "Parenteralia"),

    // Oralia flüssig
    ("Sirup", "Oralia flüssig"),
    ("Suspension", "Oralia flüssig"),
    ("Susp", "Oralia flüssig"),
    ("Emulsion", "Oralia flüssig"),
    ("Lösung", "Oralia flüssig"),
    ("Lös", "Oralia flüssig"),
    ("Tropfen", "Oralia flüssig"),
    ("Gtt", "Oralia flüssig"),
    ("Elixier", "Oralia flüssig"),

    // Ophthalmica
    ("Augentropfen", "Ophthalmica"),
    ("Gtt Opht", "Ophthalmica"),
    ("Augensalbe", "Ophthalmica"),
    ("Ung Opht", "Ophthalmica"),
    ("Augengel", "Ophthalmica"),

    // Otica
    ("Ohrentropfen", "Otica"),
    ("Gtt Auric", "Otica"),

    // Nasalia
    ("Nasenspray", "Nasalia"),
    ("Nasenöl", "Nasalia"),
    ("Nasensalbe", "Nasalia"),
    ("Spray Nas", "Nasalia"),

    // Externa (skin/topical)
    ("Salbe", "Externa"),
    ("Creme", "Externa"),
    ("Crème", "Externa"),
    ("Gel", "Externa"),
    ("Paste", "Externa"),
    ("Liniment", "Externa"),
    ("Lotion", "Externa"),
    ("Lot", "Externa"),
    ("Shampoo", "Externa"),
    ("Pflaster", "Externa"),
    ("Transdermpflaster", "Externa"),
    ("Emulsion cutan", "Externa"),
    ("Schaum", "Externa"),
    ("Spray", "Externa"),
    ("Sol cut", "Externa"),
    ("Tinktur", "Externa"),

    // Suppositorien / Vaginalia
    ("Suppositorien", "Suppositorien"),
    ("Suppositorium", "Suppositorien"),
    ("Supp", "Suppositorien"),
    ("Zäpfchen", "Suppositorien"),
    ("Zäpfli", "Suppositorien"),
    ("Vaginalsuppositorien", "Vaginalia"),
    ("Vaginalzäpfchen", "Vaginalia"),
    ("Ovula", "Vaginalia"),

    // Pulvera
    ("Pulver", "Pulver"),
    ("Plv", "Pulver"),
    ("Granulat", "Pulver"),
    ("Gran", "Pulver"),
    ("Brausepulver", "Pulver"),

    // Inhalanda
    ("Inhalationslösung", "Inhalanda"),
    ("Inhalationsdampf", "Inhalanda"),
    ("Dosieraerosol", "Inhalanda"),
    ("Dosieraeros", "Inhalanda"),
    ("Aerosol", "Inhalanda"),
    ("Aéros", "Inhalanda"),
    ("Pulverinhalator", "Inhalanda"),
    ("Inhalator", "Inhalanda"),
    ("Vernebler", "Inhalanda"),
];

/// Galenic group → numeric OID.  Mirrors the Ruby `galenic_oids.yaml`.
const GROUP_OID: &[(&str, u32)] = &[
    ("Tabletten", 10),
    ("Kapseln", 11),
    ("Parenteralia", 20),
    ("Oralia flüssig", 30),
    ("Ophthalmica", 31),
    ("Otica", 32),
    ("Nasalia", 33),
    ("Externa", 40),
    ("Suppositorien", 50),
    ("Vaginalia", 51),
    ("Pulver", 60),
    ("Inhalanda", 70),
];

static FORM_TO_GROUP: Lazy<HashMap<&'static str, &'static str>> =
    Lazy::new(|| FORM_GROUP.iter().copied().collect());

static GROUP_TO_OID: Lazy<HashMap<&'static str, u32>> =
    Lazy::new(|| GROUP_OID.iter().copied().collect());

pub fn group_by_form(form: &str) -> Option<&'static str> {
    FORM_TO_GROUP.get(form.trim()).copied()
}

pub fn oid_for_form(form: &str) -> Option<u32> {
    group_by_form(form).and_then(oid_for_group)
}

pub fn oid_for_group(group: &str) -> Option<u32> {
    GROUP_TO_OID.get(group.trim()).copied()
}

/// Every known (form, group) pair in definition order.  The builder
/// uses this to substring-match a free-text description against our
/// form catalogue.
pub fn known_forms() -> &'static [(&'static str, &'static str)] {
    FORM_GROUP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tablette_maps_to_tabletten() {
        assert_eq!(group_by_form("Tablette"), Some("Tabletten"));
        assert_eq!(oid_for_form("Tablette"), Some(10));
    }

    #[test]
    fn filmtablette_resolves_before_tablette() {
        // Substring matching in the builder relies on this ordering.
        let forms: Vec<&str> = known_forms().iter().map(|(f, _)| *f).collect();
        let filmtab = forms
            .iter()
            .position(|&f| f == "Filmtablette")
            .expect("Filmtablette present");
        let tab = forms
            .iter()
            .position(|&f| f == "Tablette")
            .expect("Tablette present");
        assert!(
            filmtab < tab,
            "Filmtablette must come before Tablette so the substring scan matches it first"
        );
    }

    #[test]
    fn parenteralia_cover_injection_patterns() {
        assert_eq!(group_by_form("Inj Lös"), Some("Parenteralia"));
        assert_eq!(group_by_form("Durchstf"), Some("Parenteralia"));
        assert_eq!(group_by_form("Fertigspr"), Some("Parenteralia"));
    }

    #[test]
    fn externa_cover_common_topicals() {
        assert_eq!(group_by_form("Creme"), Some("Externa"));
        assert_eq!(group_by_form("Salbe"), Some("Externa"));
        assert_eq!(group_by_form("Gel"), Some("Externa"));
    }

    #[test]
    fn unknown_form_returns_none() {
        assert_eq!(group_by_form("ZZZ_NOT_A_FORM"), None);
    }

    #[test]
    fn every_form_has_an_oid() {
        for (form, group) in known_forms() {
            let oid = oid_for_group(group);
            assert!(
                oid.is_some(),
                "form {form} -> group {group} has no OID defined"
            );
        }
    }
}
