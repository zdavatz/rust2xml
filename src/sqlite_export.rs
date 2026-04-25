//! Export a finished `Builder` run into a SQLite database.
//!
//! One table per output kind (`products`, `articles`, `substances`,
//! `limitations`, `interactions`, `codes`, `calc`).  Every top-level
//! field becomes a TEXT column.  Nested children (the `<ARTBAR>` and
//! repeated `<ARTPRI>` blocks inside `<ART>`) are JSON-encoded into a
//! single column so the GUI can show them without losing data.
//!
//! Callers run a normal `cli::Cli` pipeline-equivalent and then hand the
//! resulting `Builder` to `write_sqlite`.  This is a write-only module —
//! readers (the GUI tables) just open the file with rusqlite directly.

use crate::builder::{Builder, Node};
use anyhow::{Context, Result};
use rusqlite::{params_from_iter, Connection};
use std::collections::BTreeSet;
use std::path::Path;

/// All seven record kinds the builder produces.
pub fn record_sets(b: &Builder) -> Vec<(&'static str, Vec<Vec<Node>>)> {
    vec![
        ("products", b.product_records()),
        ("articles", b.article_records()),
        ("substances", b.substance_records()),
        ("limitations", b.limitation_records()),
        ("interactions", b.interaction_records()),
        ("codes", b.code_records()),
        ("calc", b.calc_records()),
    ]
}

/// Row representation: ordered (column, value) pairs.
///
/// Flattening rules for nested children — applied uniformly so the
/// resulting schema is consistent across rows that have one vs. many
/// instances of the same nested name:
///
///   * Empty nested → single empty column named after the parent.
///   * Nested with exactly one leaf child →
///     `PARENT_CHILD` = value.
///   * Nested with 2+ children where the first child is a leaf —
///     treat the first leaf's *value* as a discriminator (this is
///     `<PTYP>` for `<ARTPRI>` and `<CDTYP>` for `<ARTBAR>`):
///       * Single remaining child →
///         `PARENT_DISCVALUE` = value (e.g. `ARTPRI_FACTORY` = `42.99`).
///       * Multiple remaining children →
///         `PARENT_DISCVALUE_CHILD` = value
///         (e.g. `ARTBAR_E13_BC`, `ARTBAR_E13_BCSTAT`).
///     The same rule fires whether one or many ARTPRI siblings are
///     present, so all rows produce the same columns.
///   * Nested whose first child is itself nested (no leaf
///     discriminator): fall back to `PARENT_INDEX_CHILD` numbering.
fn record_to_row(record: &[Node]) -> Vec<(String, String)> {
    let mut row: Vec<(String, String)> = Vec::new();
    let mut handled_groups: BTreeSet<String> = BTreeSet::new();

    for node in record {
        match node {
            Node::Leaf(name, text) => {
                row.push((name.clone(), text.clone()));
            }
            Node::Nested(name, _) => {
                if handled_groups.contains(name) {
                    continue;
                }
                handled_groups.insert(name.clone());

                let instances: Vec<&[Node]> = record
                    .iter()
                    .filter_map(|n| match n {
                        Node::Nested(n2, c) if n2 == name => Some(c.as_slice()),
                        _ => None,
                    })
                    .collect();
                emit_nested_group(&mut row, name, &instances);
            }
        }
    }
    row
}

fn emit_nested_group(row: &mut Vec<(String, String)>, parent: &str, instances: &[&[Node]]) {
    if instances.is_empty() || (instances.len() == 1 && instances[0].is_empty()) {
        row.push((parent.to_string(), String::new()));
        return;
    }

    for (idx, inst) in instances.iter().enumerate() {
        emit_nested_instance(row, parent, idx, inst);
    }
}

fn emit_nested_instance(
    row: &mut Vec<(String, String)>,
    parent: &str,
    idx: usize,
    instance: &[Node],
) {
    if instance.is_empty() {
        return;
    }

    // Single-leaf instance → PARENT_CHILD = value.
    if instance.len() == 1 {
        if let Node::Leaf(child_name, child_text) = &instance[0] {
            row.push((format!("{parent}_{child_name}"), child_text.clone()));
        }
        return;
    }

    // 2+ children with a leaf as the first → use first leaf's *value*
    // as discriminator suffix on the remaining children.
    if let Node::Leaf(_disc_name, disc_value) = &instance[0] {
        let remaining: Vec<&Node> = instance.iter().skip(1).collect();
        if remaining.len() == 1 {
            if let Node::Leaf(_, v) = remaining[0] {
                row.push((format!("{parent}_{disc_value}"), v.clone()));
            }
        } else {
            for child in remaining {
                if let Node::Leaf(child_name, child_text) = child {
                    row.push((
                        format!("{parent}_{disc_value}_{child_name}"),
                        child_text.clone(),
                    ));
                }
            }
        }
        return;
    }

    // No leaf discriminator available → index-suffix fallback.
    let n = idx + 1;
    for child in instance {
        if let Node::Leaf(child_name, child_text) = child {
            row.push((format!("{parent}_{n}_{child_name}"), child_text.clone()));
        }
    }
}

fn quote_ident(name: &str) -> String {
    format!("\"{}\"", name.replace('"', "\"\""))
}

/// Write all records into the SQLite file at `path`.  Each kind produces
/// one table.  Existing tables are dropped — this is idempotent within a
/// run and the filename is timestamped so old DBs are kept on disk.
pub fn write_sqlite(b: &Builder, path: &Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let mut conn = Connection::open(path)
        .with_context(|| format!("opening {}", path.display()))?;
    // Bulk-load tuning: we control the file, no concurrent readers.
    conn.execute_batch(
        "PRAGMA journal_mode = OFF;
         PRAGMA synchronous = OFF;
         PRAGMA temp_store = MEMORY;
         PRAGMA locking_mode = EXCLUSIVE;",
    )?;

    for (table, records) in record_sets(b) {
        // First pass: collect column union (preserves first-seen order).
        let mut columns: Vec<String> = Vec::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        let rows: Vec<Vec<(String, String)>> = records
            .iter()
            .map(|r| {
                let row = record_to_row(r);
                for (k, _) in &row {
                    if seen.insert(k.clone()) {
                        columns.push(k.clone());
                    }
                }
                row
            })
            .collect();

        let create = if columns.is_empty() {
            format!(
                "DROP TABLE IF EXISTS {0}; CREATE TABLE {0} (id INTEGER PRIMARY KEY);",
                quote_ident(table)
            )
        } else {
            let cols_sql = columns
                .iter()
                .map(|c| format!("{} TEXT", quote_ident(c)))
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "DROP TABLE IF EXISTS {0}; CREATE TABLE {0} (id INTEGER PRIMARY KEY, {1});",
                quote_ident(table),
                cols_sql
            )
        };
        conn.execute_batch(&create)
            .with_context(|| format!("creating table {table}"))?;

        if rows.is_empty() {
            continue;
        }

        let placeholders = std::iter::repeat("?").take(columns.len()).collect::<Vec<_>>().join(", ");
        let cols_sql = columns
            .iter()
            .map(|c| quote_ident(c))
            .collect::<Vec<_>>()
            .join(", ");
        let insert_sql = format!(
            "INSERT INTO {} ({}) VALUES ({});",
            quote_ident(table),
            cols_sql,
            placeholders
        );

        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare(&insert_sql)?;
            for row in &rows {
                let values: Vec<String> = columns
                    .iter()
                    .map(|c| {
                        row.iter()
                            .find(|(k, _)| k == c)
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default()
                    })
                    .collect();
                stmt.execute(params_from_iter(values.iter()))?;
            }
        }
        tx.commit()?;
    }

    // Tiny meta table — useful for the GUI to confirm the run mode and
    // when the file was written.
    conn.execute_batch(
        "DROP TABLE IF EXISTS meta;
         CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT);",
    )?;
    let release_date = b.inputs.release_date.clone();
    conn.execute(
        "INSERT INTO meta (key, value) VALUES ('release_date', ?1);",
        rusqlite::params![release_date],
    )?;
    Ok(())
}

/// Build the timestamped filename `rust2xml_<flag>_HHMM_DD.MM.YYYY.sqlite`.
/// `flag` is the single-letter option (`'e'` or `'b'`).
pub fn timestamped_filename(flag: char, now: chrono::DateTime<chrono::Local>) -> String {
    format!(
        "rust2xml_{}_{}_{}.sqlite",
        flag,
        now.format("%H%M"),
        now.format("%d.%m.%Y")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::options::Options;
    use crate::builder::Inputs;

    #[test]
    fn timestamp_format_is_hhmm_then_ddmmyyyy() {
        let dt = chrono::Local
            .with_ymd_and_hms(2026, 4, 25, 14, 30, 0)
            .single()
            .unwrap();
        assert_eq!(
            timestamped_filename('e', dt),
            "rust2xml_e_1430_25.04.2026.sqlite"
        );
    }

    #[test]
    fn write_empty_builder_creates_all_tables() {
        let b = Builder::new(Options::default(), Inputs::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sqlite");
        write_sqlite(&b, &path).unwrap();
        let conn = Connection::open(&path).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        for expected in [
            "articles",
            "calc",
            "codes",
            "interactions",
            "limitations",
            "meta",
            "products",
            "substances",
        ] {
            assert!(tables.contains(&expected.to_string()), "missing {expected}");
        }
    }

    use chrono::TimeZone;

    #[test]
    fn flatten_artbar_uses_cdtyp_discriminator() {
        let record = vec![
            Node::leaf("PHAR", "12345"),
            Node::Nested(
                "ARTBAR".into(),
                vec![
                    Node::leaf("CDTYP", "E13"),
                    Node::leaf("BC", "7680616570161"),
                    Node::leaf("BCSTAT", "A"),
                ],
            ),
        ];
        let row = record_to_row(&record);
        assert_eq!(
            row,
            vec![
                ("PHAR".to_string(), "12345".to_string()),
                ("ARTBAR_E13_BC".to_string(), "7680616570161".to_string()),
                ("ARTBAR_E13_BCSTAT".to_string(), "A".to_string()),
            ]
        );
    }

    #[test]
    fn single_artpri_uses_same_discriminator_as_multi() {
        // Schema must be uniform: one or many ARTPRI siblings should
        // both produce columns named ARTPRI_<PTYP>.
        let mk_pri = |ptyp: &str, price: &str| {
            Node::Nested(
                "ARTPRI".into(),
                vec![Node::leaf("PTYP", ptyp), Node::leaf("PRICE", price)],
            )
        };
        let single = vec![mk_pri("FACTORY", "42.99")];
        let row = record_to_row(&single);
        assert_eq!(
            row,
            vec![("ARTPRI_FACTORY".to_string(), "42.99".to_string())]
        );
    }

    #[test]
    fn flatten_multi_nested_uses_discriminator_suffix() {
        let mk_pri = |ptyp: &str, price: &str| {
            Node::Nested(
                "ARTPRI".into(),
                vec![Node::leaf("PTYP", ptyp), Node::leaf("PRICE", price)],
            )
        };
        let record = vec![
            Node::leaf("PHAR", "12345"),
            mk_pri("FACTORY", "42.99"),
            mk_pri("PUBLIC", "63.05"),
            mk_pri("ZURROSE", "45.57"),
            mk_pri("ZURROSEPUB", "63.05"),
        ];
        let row = record_to_row(&record);
        assert_eq!(
            row,
            vec![
                ("PHAR".to_string(), "12345".to_string()),
                ("ARTPRI_FACTORY".to_string(), "42.99".to_string()),
                ("ARTPRI_PUBLIC".to_string(), "63.05".to_string()),
                ("ARTPRI_ZURROSE".to_string(), "45.57".to_string()),
                ("ARTPRI_ZURROSEPUB".to_string(), "63.05".to_string()),
            ]
        );
    }

    #[test]
    fn flatten_empty_nested_emits_empty_column() {
        let record = vec![
            Node::leaf("PHAR", "12345"),
            Node::Nested("ARTCOMP".into(), Vec::new()),
        ];
        let row = record_to_row(&record);
        assert_eq!(
            row,
            vec![
                ("PHAR".to_string(), "12345".to_string()),
                ("ARTCOMP".to_string(), String::new()),
            ]
        );
    }
}
