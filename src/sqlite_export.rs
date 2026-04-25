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

/// Convert one node into its column value (string).  Nested nodes are
/// JSON-encoded so callers can decode them back to structured form.
fn node_value(node: &Node) -> String {
    match node {
        Node::Leaf(_, text) => text.clone(),
        Node::Nested(_, children) => {
            // {"FIELD":"value", ...} — preserves leaf field order via Vec.
            let pairs: Vec<(String, String)> = children
                .iter()
                .map(|c| match c {
                    Node::Leaf(k, v) => (k.clone(), v.clone()),
                    Node::Nested(k, _) => (k.clone(), node_value(c)),
                })
                .collect();
            serde_json::to_string(&pairs).unwrap_or_default()
        }
    }
}

/// Row representation: ordered (column, value) pairs.  Multi-valued
/// columns (e.g. four `<ARTPRI>` siblings) are gathered into a JSON
/// array so the column count stays predictable.
fn record_to_row(record: &[Node]) -> Vec<(String, String)> {
    let mut grouped: Vec<(String, Vec<String>)> = Vec::new();
    for node in record {
        let name = match node {
            Node::Leaf(n, _) | Node::Nested(n, _) => n.clone(),
        };
        let value = node_value(node);
        if let Some(slot) = grouped.iter_mut().find(|(k, _)| k == &name) {
            slot.1.push(value);
        } else {
            grouped.push((name, vec![value]));
        }
    }
    grouped
        .into_iter()
        .map(|(k, v)| {
            let value = if v.len() == 1 {
                v.into_iter().next().unwrap_or_default()
            } else {
                serde_json::to_string(&v).unwrap_or_default()
            };
            (k, value)
        })
        .collect()
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
}
