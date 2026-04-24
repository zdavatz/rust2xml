//! Compare two Artikelstamm v5 files.

use rust2xml::compare::compare_files;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} file_a.xml file_b.xml", args[0]);
        return ExitCode::from(2);
    }

    match compare_files(&args[1], &args[2]) {
        Ok(rep) => {
            println!("Added: {}", rep.added.len());
            for k in &rep.added {
                println!("  + {k}");
            }
            println!("Removed: {}", rep.removed.len());
            for k in &rep.removed {
                println!("  - {k}");
            }
            println!("Changed: {}", rep.changed.len());
            for (k, (a, b)) in &rep.changed {
                println!("  ~ {k}\n      was: {a}\n      now: {b}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::from(1)
        }
    }
}
