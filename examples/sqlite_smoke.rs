//! End-to-end smoke test for `Cli::run_to_sqlite`.
//!
//! Runs the pipeline with `--skip-download` against the cached
//! downloads in the current directory and writes a SQLite file at
//! `sqlite/smoke.sqlite`, then prints table row counts.
//!
//! Run with:
//!   cd /path/with/downloads/
//!   cargo run --release --example sqlite_smoke

use rust2xml::cli::Cli;
use rust2xml::options::{Options, PriceSource};
use rusqlite::Connection;
use std::path::PathBuf;

fn main() -> anyhow::Result<()> {
    let mut opts = Options::default();
    opts.extended = true;
    opts.nonpharma = true;
    opts.calc = true;
    opts.price = Some(PriceSource::ZurRose);
    opts.skip_download = true;
    opts.log = true;

    let path = PathBuf::from("sqlite/smoke.sqlite");
    println!("Writing {}", path.display());
    let start = std::time::Instant::now();
    Cli::new(opts).run_to_sqlite(&path)?;
    println!("Done in {:.1}s", start.elapsed().as_secs_f64());

    let conn = Connection::open(&path)?;
    let mut stmt = conn.prepare(
        "SELECT name FROM sqlite_master WHERE type='table' ORDER BY name",
    )?;
    let names: Vec<String> = stmt
        .query_map([], |row| row.get::<_, String>(0))?
        .flatten()
        .collect();
    for name in &names {
        let n: i64 = conn.query_row(
            &format!("SELECT COUNT(*) FROM \"{}\"", name.replace('"', "\"\"")),
            [],
            |row| row.get(0),
        )?;
        println!("  {name}: {n} rows");
    }

    Ok(())
}
