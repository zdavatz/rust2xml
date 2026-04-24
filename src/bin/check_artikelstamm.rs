//! Run the semantic checks over an Artikelstamm XML — port of
//! `bin/check_artikelstamm`.

use oddb2xml::semantic_check::SemanticCheck;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} artikelstamm.xml", args[0]);
        return ExitCode::from(2);
    }

    let chk = SemanticCheck::new(&args[1]);
    let prod = match chk.every_product_number_is_unique() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e:#}");
            return ExitCode::from(1);
        }
    };
    let items = match chk.every_item_number_is_unique() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{e:#}");
            return ExitCode::from(1);
        }
    };

    println!("products_unique = {prod}");
    println!("items_unique    = {items}");
    if prod && items {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}
