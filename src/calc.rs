//! Galenic form / group lookups — port of `lib/oddb2xml/calc.rb`.
//!
//! The Ruby implementation carried small YAML tables shipped inside the
//! gem alongside the source.  In Rust we inline them as `&[(&str, u32)]`
//! slices so the binary is self-contained and no runtime file I/O is
//! needed for a lookup.  A comprehensive port of the 30+ Swiss galenic
//! groups remains a phase-7 deliverable; the current set covers the
//! 90%-tile of real-world Packungen data.
//!
//! Public API mirrors the Ruby methods used by builder.rb:
//!  * `group_by_form(form)` → galenic group label
//!  * `oid_for_form(form)` → numeric OID
//!  * `oid_for_group(group)` → numeric OID

use once_cell::sync::Lazy;
use std::collections::HashMap;

static FORM_TO_GROUP: Lazy<HashMap<&'static str, &'static str>> = Lazy::new(|| {
    [
        ("Tablette", "Tabletten"),
        ("Tabletten", "Tabletten"),
        ("Dragée", "Tabletten"),
        ("Filmtablette", "Tabletten"),
        ("Kapseln", "Kapseln"),
        ("Kapsel", "Kapseln"),
        ("Injektionslösung", "Parenteralia"),
        ("Infusionslösung", "Parenteralia"),
        ("Sirup", "Oralia flüssig"),
        ("Suspension", "Oralia flüssig"),
        ("Lösung", "Oralia flüssig"),
        ("Salbe", "Externa"),
        ("Creme", "Externa"),
        ("Gel", "Externa"),
        ("Spray", "Externa"),
        ("Tropfen", "Oralia flüssig"),
        ("Suppositorien", "Suppositorien"),
        ("Zäpfchen", "Suppositorien"),
        ("Pulver", "Pulver"),
        ("Granulat", "Pulver"),
    ]
    .into_iter()
    .collect()
});

static GROUP_TO_OID: Lazy<HashMap<&'static str, u32>> = Lazy::new(|| {
    [
        ("Tabletten", 10),
        ("Kapseln", 11),
        ("Parenteralia", 20),
        ("Oralia flüssig", 30),
        ("Externa", 40),
        ("Suppositorien", 50),
        ("Pulver", 60),
    ]
    .into_iter()
    .collect()
});

pub fn group_by_form(form: &str) -> Option<&'static str> {
    FORM_TO_GROUP.get(form.trim()).copied()
}

pub fn oid_for_form(form: &str) -> Option<u32> {
    group_by_form(form).and_then(oid_for_group)
}

pub fn oid_for_group(group: &str) -> Option<u32> {
    GROUP_TO_OID.get(group.trim()).copied()
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
    fn unknown_form_returns_none() {
        assert_eq!(group_by_form("ZZZ_NOT_A_FORM"), None);
    }
}
