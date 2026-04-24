//! Main CLI entry — port of `bin/oddb2xml`.

use oddb2xml::cli::Cli;
use oddb2xml::options::Options;
use std::process::ExitCode;
use std::time::Instant;

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let argv: Vec<String> = std::env::args().skip(1).collect();
    let opts = match Options::parse(argv) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };

    let start = Instant::now();
    let cli = Cli::new(opts);
    match cli.run() {
        Ok(files) => {
            let elapsed = start.elapsed().as_secs();
            println!(
                "{now}: done. Wrote {n} file(s) in {elapsed} seconds",
                now = chrono::Utc::now().to_rfc3339(),
                n = files.len()
            );
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::from(1)
        }
    }
}
