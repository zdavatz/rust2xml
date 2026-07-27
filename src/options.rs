//! CLI options parsed by `clap`.  Mirrors `lib/oddb2xml/options.rb`
//! (built on `optimist` in Ruby) — all 18 flags, including the
//! implied-flag cascade at the bottom of the Ruby file.

use clap::{ArgAction, Parser};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Xml,
    Dat,
}

impl std::str::FromStr for Format {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "xml" => Ok(Format::Xml),
            "dat" => Ok(Format::Dat),
            other => Err(format!("unknown format: {other} (expected xml|dat)")),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PriceSource {
    ZurRose,
}

impl std::str::FromStr for PriceSource {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "zurrose" | "zur_rose" | "zur-rose" | "true" | "" => Ok(PriceSource::ZurRose),
            other => Err(format!("unknown price source: {other}")),
        }
    }
}

/// Result of parsing `ARGV`.  Equivalent to the symbol-keyed Hash the Ruby
/// `Oddb2xml::Options.parse` returns.
#[derive(Debug, Clone)]
pub struct Options {
    pub nonpharma: bool,
    pub artikelstamm: bool,
    /// Additionally emit the legacy Artikelstamm v5 (no `<ARTSL>`).
    /// Additive to `artikelstamm` — mirrors oddb2xml's `--artikelstamm-v5`.
    pub artikelstamm_v5: bool,
    /// Emit the Italian-language description leaves (`<DSCRI>`) in the
    /// Artikelstamm.  Off by default: the strict upstream Elexis v6/v5 XSD
    /// (the one the Elexis importer actually validates against) has no
    /// `<DSCRI>` element, so emitting it makes the import fail with a
    /// `cvc-complex-type.2.4.a` error.  Enable with `-it` / `--italian`
    /// only when the consumer uses the oddb2xml-extended XSD.
    pub italian: bool,
    pub compress_ext: Option<String>,
    pub extended: bool,
    pub fhir: bool,
    pub fhir_url: Option<String>,
    pub format: Format,
    pub ean14: bool,
    pub percent: Option<i32>,
    pub fi: bool,
    pub price: Option<PriceSource>,
    pub tag_suffix: Option<String>,
    pub address: bool,
    pub calc: bool,
    pub skip_download: bool,
    pub log: bool,
    pub use_ra11zip: Option<String>,
    pub firstbase: bool,
    /// `-r` / `--rogger`: prefer the German article names from the
    /// "Rogger Mediliste" (Vitabyte / Zur Rose name-conflict corrections,
    /// fetched from the shared Google Sheet with a bundled offline
    /// fallback).  See [`crate::rogger_names`].
    pub rogger: bool,
    /// Optional path for the BAG Indikationscode XLSX export.  When set,
    /// rust2xml writes one row per (code, GTIN) pair to this path after
    /// the normal pipeline finishes.  Requires `fhir` (or `fhir_url`)
    /// since the codes come from `ClinicalUseDefinition` resources.
    pub indc_xlsx: Option<String>,
    /// `transfer_dat` — a path passed positionally as the first free arg.
    pub transfer_dat: Option<String>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            nonpharma: false,
            artikelstamm: false,
            artikelstamm_v5: false,
            italian: false,
            compress_ext: None,
            extended: false,
            fhir: false,
            fhir_url: None,
            format: Format::Xml,
            ean14: false,
            percent: None,
            fi: false,
            price: None,
            tag_suffix: None,
            address: false,
            calc: false,
            skip_download: false,
            log: false,
            use_ra11zip: None,
            firstbase: false,
            rogger: false,
            indc_xlsx: None,
            transfer_dat: None,
        }
    }
}

/// Clap definition (kept separate from the resolved `Options` struct so we
/// can run the implied-flag logic in one pass).
#[derive(Parser, Debug)]
#[command(
    name = "rust2xml",
    version = crate::version::VERSION,
    about = "rust2xml creates XML/DAT files from Swiss drug data sources",
    long_about = None,
)]
struct RawArgs {
    /// Additional target nonpharma
    #[arg(short = 'a', long = "append", action = ArgAction::SetTrue)]
    append: bool,

    /// Create Artikelstamm v6 for Elexis >= 3.1
    #[arg(long = "artikelstamm", action = ArgAction::SetTrue)]
    artikelstamm: bool,

    /// Additionally create the legacy Artikelstamm v5 (without the
    /// <ARTSL> BAG indication codes). Implies --artikelstamm.
    #[arg(long = "artikelstamm-v5", action = ArgAction::SetTrue)]
    artikelstamm_v5: bool,

    /// Include the Italian description (<DSCRI>) in the Artikelstamm.
    /// Off by default: the strict Elexis v6/v5 XSD has no DSCRI element,
    /// so the default output carries only German + French.
    #[arg(long = "italian", visible_alias = "it", action = ArgAction::SetTrue)]
    italian: bool,

    /// Compression format: tar.gz | zip
    #[arg(short = 'c', long = "compress-ext", value_name = "FMT")]
    compress_ext: Option<String>,

    /// pharma, non-pharma plus prices and non-pharma from zurrose.
    /// Products without EAN-Code will also be listed.
    /// File oddb_calc.xml will also be generated.
    #[arg(short = 'e', long = "extended", action = ArgAction::SetTrue)]
    extended: bool,

    /// Use FHIR NDJSON from FOPH/BAG instead of SL XML.
    /// Default ON for -e/--extended and -b/--firstbase since June 2026.
    #[arg(long = "fhir", action = ArgAction::SetTrue)]
    fhir: bool,

    /// Use the legacy SL XML instead of FHIR NDJSON (opt out of the -e/-b FHIR default)
    #[arg(long = "no-fhir", action = ArgAction::SetTrue)]
    no_fhir: bool,

    /// Specific FHIR NDJSON URL (implies --fhir)
    #[arg(long = "fhir-url", value_name = "URL")]
    fhir_url: Option<String>,

    /// File format: xml | dat. Default: xml.  If set, -o is ignored.
    #[arg(short = 'f', long = "format", value_name = "FMT", default_value = "xml")]
    format: String,

    /// Include target option (only used by 'dat' format; 'xml' always includes ean14).
    #[arg(short = 'i', long = "include", action = ArgAction::SetTrue)]
    include: bool,

    /// Increment price by X percent. Forces -f dat -p zurrose.
    #[arg(short = 'I', long = "increment", value_name = "PCT")]
    increment: Option<i32>,

    /// Optional fachinfo output
    #[arg(short = 'o', long = "fi", action = ArgAction::SetTrue)]
    fi: bool,

    /// Price source (transfer.dat) from ZurRose
    #[arg(short = 'p', long = "price", value_name = "SRC", num_args = 0..=1, default_missing_value = "zurrose")]
    price: Option<String>,

    /// XML tag suffix. Also used as filename prefix.
    #[arg(short = 't', long = "tag-suffix", value_name = "S")]
    tag_suffix: Option<String>,

    /// {product|address}. product is default.
    #[arg(short = 'x', long = "context", value_name = "CTX", default_value = "product")]
    context: String,

    /// Create only oddb_calc.xml with GTIN, name and galenic info
    #[arg(long = "calc", action = ArgAction::SetTrue)]
    calc: bool,

    /// Skip downloads if the file is already under downloads/
    #[arg(long = "skip-download", action = ArgAction::SetTrue)]
    skip_download: bool,

    /// Log important actions
    #[arg(long = "log", action = ArgAction::SetTrue)]
    log: bool,

    /// Use the ra11.zip (a zipped transfer.dat from Galexis)
    #[arg(long = "use-ra11zip", value_name = "PATH")]
    use_ra11zip: Option<String>,

    /// Build all NONPHARMA articles on firstbase (GS1 Switzerland CSV)
    #[arg(short = 'b', long = "firstbase", action = ArgAction::SetTrue)]
    firstbase: bool,

    /// Prefer the German article names from the Rogger list
    /// (Vitabyte/Zur Rose name-conflict corrections, fetched directly
    /// from the shared 'Rogger Mediliste' Google Sheet)
    #[arg(short = 'r', long = "rogger", action = ArgAction::SetTrue)]
    rogger: bool,

    /// Write a BAG Indikationscode export to <PATH> (XLSX). One row per
    /// (XXXXX.NN code, GTIN) pair with brand name, pack description,
    /// ATC, ex-factory + public price, and the indication text.
    /// Requires --fhir.
    #[arg(long = "indc-xlsx", value_name = "PATH")]
    indc_xlsx: Option<String>,

    /// Positional: optional path to a transfer.dat file.
    #[arg(trailing_var_arg = true)]
    free_args: Vec<String>,
}

impl Options {
    /// Parse from `ARGV`-style strings.  Applies the same implied-flag
    /// cascade as the Ruby `Oddb2xml::Options.parse`.
    pub fn parse<I, T>(argv: I) -> Result<Self, String>
    where
        I: IntoIterator<Item = T>,
        T: Into<std::ffi::OsString> + Clone,
    {
        let raw = RawArgs::try_parse_from(
            std::iter::once(std::ffi::OsString::from("rust2xml"))
                .chain(argv.into_iter().map(Into::into)),
        )
        .map_err(|e| e.to_string())?;

        let mut opts = Options::default();

        opts.artikelstamm = raw.artikelstamm;
        opts.artikelstamm_v5 = raw.artikelstamm_v5;
        opts.italian = raw.italian;
        opts.compress_ext = raw.compress_ext;
        opts.extended = raw.extended;
        opts.fhir = raw.fhir;
        opts.fhir_url = raw.fhir_url;
        opts.format = raw.format.parse()?;
        opts.fi = raw.fi;
        opts.tag_suffix = raw.tag_suffix;
        opts.calc = raw.calc;
        opts.skip_download = raw.skip_download;
        opts.log = raw.log;
        opts.use_ra11zip = raw.use_ra11zip;
        opts.firstbase = raw.firstbase;
        opts.rogger = raw.rogger;
        opts.indc_xlsx = raw.indc_xlsx;

        if let Some(pct) = raw.increment {
            opts.percent = Some(pct);
            opts.nonpharma = true;
            opts.price = Some(PriceSource::ZurRose);
            opts.ean14 = true;
        } else {
            opts.ean14 = raw.include;
        }

        opts.nonpharma = opts.nonpharma || raw.append;

        if opts.firstbase {
            opts.nonpharma = true;
            opts.calc = true;
        }

        if opts.extended {
            opts.nonpharma = true;
            opts.price = Some(PriceSource::ZurRose);
            opts.calc = true;
        }

        // --artikelstamm-v5 is additive: it needs the full artikelstamm build.
        if opts.artikelstamm_v5 {
            opts.artikelstamm = true;
        }

        if opts.artikelstamm {
            opts.extended = true;
            opts.price = Some(PriceSource::ZurRose);
        }

        if opts.fhir_url.is_some() {
            opts.fhir = true;
        }
        // The Indikationscode export requires the FHIR feed (CUDs only
        // exist there).  Implying --fhir lets the user run a one-shot
        // `rust2xml --indc-xlsx out.xlsx`.
        if opts.indc_xlsx.is_some() {
            opts.fhir = true;
        }

        // From June 2026, -e/--extended and -b/--firstbase default to the
        // FHIR (FOPH/BAG NDJSON) source. Opt out with --no-fhir.
        if (opts.extended || opts.firstbase) && !raw.fhir && !raw.no_fhir {
            opts.fhir = true;
        }

        // --price overrides anything set above.
        if let Some(p) = raw.price.as_deref() {
            opts.price = Some(p.parse()?);
        }

        if matches!(opts.format, Format::Xml) {
            opts.ean14 = true; // xml format always forces ean14.
        }

        opts.address = matches!(raw.context.to_lowercase().as_str(), "address" | "addr");

        opts.transfer_dat = raw.free_args.into_iter().next();

        Ok(opts)
    }
}

#[cfg(test)]
mod tests {
    //! Option parity tests — one per Ruby `opt :…` entry in
    //! `lib/oddb2xml/options.rb`, plus cascade rules.

    use super::*;

    #[test]
    fn default_is_xml() {
        let o = Options::parse::<_, &str>(std::iter::empty()).unwrap();
        assert!(matches!(o.format, Format::Xml));
        assert!(!o.nonpharma);
        assert!(!o.artikelstamm);
        assert!(o.ean14, "xml format forces ean14");
    }

    // ---------- short-flag parity ----------

    #[test]
    fn dash_a_is_append() {
        assert!(Options::parse(["-a"]).unwrap().nonpharma);
        assert!(Options::parse(["--append"]).unwrap().nonpharma);
    }

    #[test]
    fn dash_c_is_compress_ext() {
        let o = Options::parse(["-c", "zip"]).unwrap();
        assert_eq!(o.compress_ext.as_deref(), Some("zip"));
    }

    #[test]
    fn dash_e_is_extended() {
        assert!(Options::parse(["-e"]).unwrap().extended);
    }

    #[test]
    fn dash_f_is_format() {
        let o = Options::parse(["-f", "dat"]).unwrap();
        assert!(matches!(o.format, Format::Dat));
    }

    #[test]
    fn dash_i_is_include() {
        // With -f dat to avoid xml-forces-ean14.
        let o = Options::parse(["-f", "dat", "-i"]).unwrap();
        assert!(o.ean14);
    }

    #[test]
    fn dash_cap_i_is_increment() {
        let o = Options::parse(["-I", "5"]).unwrap();
        assert_eq!(o.percent, Some(5));
    }

    #[test]
    fn dash_o_is_fi() {
        assert!(Options::parse(["-o"]).unwrap().fi);
    }

    #[test]
    fn dash_p_is_price() {
        let o = Options::parse(["-p", "zurrose"]).unwrap();
        assert_eq!(o.price, Some(PriceSource::ZurRose));
    }

    #[test]
    fn dash_t_is_tag_suffix() {
        let o = Options::parse(["-t", "v2"]).unwrap();
        assert_eq!(o.tag_suffix.as_deref(), Some("v2"));
    }

    #[test]
    fn dash_x_is_context_address() {
        assert!(Options::parse(["-x", "address"]).unwrap().address);
        assert!(Options::parse(["-x", "addr"]).unwrap().address);
        assert!(!Options::parse(["-x", "product"]).unwrap().address);
    }

    #[test]
    fn dash_b_is_firstbase() {
        let o = Options::parse(["-b"]).unwrap();
        assert!(o.firstbase);
        assert!(o.nonpharma, "firstbase implies nonpharma");
        assert!(o.calc, "firstbase implies calc");
    }

    // ---------- long-flag parity for flags Ruby keeps long-only ----------

    #[test]
    fn long_only_artikelstamm() {
        let o = Options::parse(["--artikelstamm"]).unwrap();
        assert!(o.artikelstamm);
        assert!(!o.artikelstamm_v5, "v5 stays off unless requested");
        assert!(o.extended, "artikelstamm implies extended");
        assert_eq!(o.price, Some(PriceSource::ZurRose));
    }

    #[test]
    fn artikelstamm_v5_implies_artikelstamm() {
        let o = Options::parse(["--artikelstamm-v5"]).unwrap();
        assert!(o.artikelstamm_v5);
        assert!(o.artikelstamm, "v5 implies the full artikelstamm build");
        assert!(o.extended, "artikelstamm implies extended");
        assert_eq!(o.price, Some(PriceSource::ZurRose));
    }

    #[test]
    fn italian_is_off_by_default_and_opt_in() {
        assert!(!Options::parse(["--artikelstamm"]).unwrap().italian,
            "Italian description off by default (strict Elexis XSD)");
        assert!(Options::parse(["--artikelstamm", "--italian"]).unwrap().italian);
        assert!(Options::parse(["--artikelstamm", "--it"]).unwrap().italian,
            "--it alias enables Italian");
    }

    #[test]
    fn rogger_is_off_by_default_and_opt_in() {
        assert!(!Options::parse([] as [&str; 0]).unwrap().rogger,
            "Rogger name overrides off by default");
        assert!(Options::parse(["-r"]).unwrap().rogger, "-r short flag");
        assert!(Options::parse(["--rogger"]).unwrap().rogger, "--rogger long flag");
        // Purely additive: it must not drag in any other flag.
        let o = Options::parse(["--rogger"]).unwrap();
        assert!(!o.extended && !o.nonpharma && !o.calc && o.price.is_none());
    }

    #[test]
    fn long_only_fhir() {
        let o = Options::parse(["--fhir"]).unwrap();
        assert!(o.fhir);
    }

    #[test]
    fn long_only_fhir_url_implies_fhir() {
        let o = Options::parse(["--fhir-url", "https://example.com/x.ndjson"]).unwrap();
        assert!(o.fhir);
        assert_eq!(o.fhir_url.as_deref(), Some("https://example.com/x.ndjson"));
    }

    #[test]
    fn extended_defaults_to_fhir() {
        assert!(Options::parse(["--extended"]).unwrap().fhir);
        assert!(Options::parse(["-e"]).unwrap().fhir);
    }

    #[test]
    fn firstbase_defaults_to_fhir() {
        assert!(Options::parse(["--firstbase"]).unwrap().fhir);
        assert!(Options::parse(["-b"]).unwrap().fhir);
    }

    #[test]
    fn no_fhir_opts_out_of_extended_default() {
        assert!(!Options::parse(["--extended", "--no-fhir"]).unwrap().fhir);
        assert!(!Options::parse(["--firstbase", "--no-fhir"]).unwrap().fhir);
    }

    #[test]
    fn plain_run_stays_non_fhir() {
        assert!(!Options::parse([] as [&str; 0]).unwrap().fhir);
    }

    #[test]
    fn long_only_calc() {
        assert!(Options::parse(["--calc"]).unwrap().calc);
    }

    #[test]
    fn long_only_skip_download() {
        assert!(Options::parse(["--skip-download"]).unwrap().skip_download);
    }

    #[test]
    fn long_only_log() {
        assert!(Options::parse(["--log"]).unwrap().log);
    }

    #[test]
    fn long_only_use_ra11zip() {
        let o = Options::parse(["--use-ra11zip", "ra11.zip"]).unwrap();
        assert_eq!(o.use_ra11zip.as_deref(), Some("ra11.zip"));
    }

    // ---------- implied-flag cascade ----------

    #[test]
    fn extended_cascade() {
        let o = Options::parse(["-e"]).unwrap();
        assert!(o.extended);
        assert!(o.nonpharma);
        assert!(o.calc);
        assert_eq!(o.price, Some(PriceSource::ZurRose));
    }

    #[test]
    fn increment_cascade() {
        let o = Options::parse(["-I", "5"]).unwrap();
        assert_eq!(o.percent, Some(5));
        assert!(o.nonpharma);
        assert_eq!(o.price, Some(PriceSource::ZurRose));
        assert!(o.ean14);
    }

    #[test]
    fn positional_transfer_dat() {
        let o = Options::parse(["/tmp/transfer.dat"]).unwrap();
        assert_eq!(o.transfer_dat.as_deref(), Some("/tmp/transfer.dat"));
    }

    #[test]
    fn xml_format_always_forces_ean14() {
        // No -i, no -I, no -f => default xml => ean14 should still be true.
        let o = Options::parse::<_, &str>(std::iter::empty()).unwrap();
        assert!(o.ean14);
    }
}
