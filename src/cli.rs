//! Pipeline orchestration — port of `lib/oddb2xml/cli.rb`.
//!
//! Downloads data sources in parallel, runs each through its extractor,
//! hands the normalized maps to `Builder`, writes each output file
//! into the current working directory, optionally compresses the
//! bundle.  Mirrors the Ruby `Cli#run` method.

use crate::builder::{Builder, Inputs};
use crate::compressor::{CompressExt, Compressor};
use crate::downloader::{self, SwissmedicKind};
use crate::extractor::{
    BagXmlExtractor, EphaExtractor, FirstbaseExtractor, LppvExtractor,
    RefdataExtractor,
    swissmedic::{SwissmedicExtractor, SwissmedicKind as ExtKind},
    ZurroseExtractor,
};
use crate::fhir_support::{FhirDownloader, FhirExtractor, DEFAULT_FHIR_URL};
use crate::options::{Format, Options};
use crate::util;
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct Cli {
    pub opts: Options,
}

impl Cli {
    pub fn new(opts: Options) -> Self {
        Self { opts }
    }

    /// Run the same download/extract pipeline as `run()`, but write
    /// the result into a SQLite database at `sqlite_path` instead of
    /// emitting XML files.  Used by the GUI binary.
    pub fn run_to_sqlite(self, sqlite_path: &std::path::Path) -> Result<()> {
        util::save_options(util::GlobalOptions {
            skip_download: self.opts.skip_download,
            log: self.opts.log,
            work_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            downloads_dir: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("downloads"),
        });
        let _ = fs::create_dir_all(util::downloads_dir());

        util::progress(0.02, "Starting pipeline");
        let inputs = self.collect_inputs()?;
        util::progress(0.85, "Building records");
        let b = Builder::new(self.opts.clone(), inputs);
        util::progress(0.92, "Writing SQLite database");
        crate::sqlite_export::write_sqlite(&b, sqlite_path)?;
        util::progress(1.0, "Done");
        Ok(())
    }

    pub fn run(self) -> Result<Vec<PathBuf>> {
        // Push options into the global holder so util::log / skip_download
        // work exactly as their Ruby counterparts.
        util::save_options(util::GlobalOptions {
            skip_download: self.opts.skip_download,
            log: self.opts.log,
            work_dir: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            downloads_dir: std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join("downloads"),
        });
        let _ = fs::create_dir_all(util::downloads_dir());

        let inputs = self.collect_inputs()?;
        let b = Builder::new(self.opts.clone(), inputs);

        let mut outputs: Vec<PathBuf> = Vec::new();

        match self.opts.format {
            Format::Xml => {
                type BuildFn = fn(&Builder) -> Result<String>;
                let mut jobs: Vec<(&str, BuildFn)> = vec![
                    ("oddb_product.xml", Builder::build_product),
                    ("oddb_article.xml", Builder::build_article),
                    ("oddb_substance.xml", Builder::build_substance),
                    ("oddb_limitation.xml", Builder::build_limitation),
                    ("oddb_interaction.xml", Builder::build_interaction),
                    ("oddb_code.xml", Builder::build_code),
                ];
                if self.opts.calc || self.opts.extended {
                    jobs.push(("oddb_calc.xml", Builder::build_calc));
                }
                let built: Result<Vec<PathBuf>> = jobs
                    .par_iter()
                    .map(|(name, task)| {
                        let xml = task(&b)?;
                        let path = PathBuf::from(*name);
                        fs::write(&path, xml)
                            .with_context(|| format!("writing {}", path.display()))?;
                        Ok(path)
                    })
                    .collect();
                outputs.extend(built?);
            }
            Format::Dat => {
                let path = PathBuf::from("oddb.dat");
                fs::write(&path, b.build_dat())?;
                outputs.push(path);
            }
        }

        // Optional compression.
        if let Some(ext) = self.opts.compress_ext.as_deref() {
            if let Some(ce) = CompressExt::from_str(ext) {
                let mut c = Compressor::new("oddb", self.opts.format, Some(ce));
                c.contents = outputs.clone();
                c.finalize().context("compressing outputs")?;
                // Compressor removes the originals on success.
                outputs.clear();
                outputs.push(c.compress_file().to_path_buf());
            }
        }

        Ok(outputs)
    }

    /// Download + extract every source this run needs.  Threaded — each
    /// source is independent until the builder consumes them all.
    fn collect_inputs(&self) -> Result<Inputs> {
        let inputs = Mutex::new(Inputs::default());

        // Describe each job as a labelled closure that mutates `inputs`.
        // The label feeds the GUI progress bar.
        type Job = Box<dyn Fn(&Mutex<Inputs>) -> Result<()> + Send + Sync>;
        let mut jobs: Vec<(&'static str, Job)> = Vec::new();

        let use_fhir = self.opts.fhir;
        let fhir_url = self.opts.fhir_url.clone();

        if use_fhir {
            let url = fhir_url.unwrap_or_else(|| DEFAULT_FHIR_URL.to_string());
            jobs.push(("BAG (FHIR de/fr/it)", Box::new(move |store: &Mutex<Inputs>| {
                // Primary German bundle.
                let de_d = FhirDownloader::new(url.clone())?;
                let de_body = de_d.download()?;
                let de_extractor = FhirExtractor::new(de_body);
                let mut bag = de_extractor.to_hash()?;

                // FR + IT translations — same URL with the language
                // suffix swapped.  Failures are logged and ignored so
                // the run still completes if a language file is down.
                for lang in ["fr", "it"] {
                    let lang_url = url.replace("-de.ndjson", &format!("-{lang}.ndjson"));
                    if lang_url == url {
                        continue;
                    }
                    let dl = match FhirDownloader::new(lang_url.clone()) {
                        Ok(d) => d,
                        Err(e) => {
                            util::log(format!("FHIR {lang} downloader: {e}"));
                            continue;
                        }
                    };
                    match dl.download() {
                        Ok(body) => {
                            let ext = FhirExtractor::new_with_lang(body, lang);
                            match ext.to_hash() {
                                Ok(translation) => {
                                    crate::fhir_support::merge_translations(
                                        &mut bag, translation,
                                    );
                                }
                                Err(e) => util::log(format!("FHIR {lang} extract: {e}")),
                            }
                        }
                        Err(e) => util::log(format!("FHIR {lang} download: {e}")),
                    }
                }

                store.lock().unwrap().bag.extend(bag);
                Ok(())
            })));
        } else {
            jobs.push(("BAG XMLPublications", Box::new(|store: &Mutex<Inputs>| {
                let d = downloader::BagXmlDownloader::new()?;
                let xml = d.download()?;
                let e = BagXmlExtractor::new(xml);
                let bag = e.to_hash()?;
                store.lock().unwrap().bag.extend(bag);
                Ok(())
            })));
        }

        jobs.push(("Refdata Articles", Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::RefdataDownloader::new()?;
            let xml = d.download()?;
            let pharma = RefdataExtractor::new(xml.clone(), "PHARMA").to_hash()?;
            let non = RefdataExtractor::new(xml, "NONPHARMA").to_hash()?;
            let mut s = store.lock().unwrap();
            s.refdata_pharma.extend(pharma);
            s.refdata_nonpharma.extend(non);
            Ok(())
        })));

        jobs.push(("EPha interactions", Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::EphaDownloader::new()?;
            let bytes = d.download()?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let v = EphaExtractor::new(text).to_vec();
            store.lock().unwrap().epha_interactions.extend(v);
            Ok(())
        })));

        jobs.push(("LPPV list", Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::LppvDownloader::new()?;
            let bytes = d.download()?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let h = LppvExtractor::new(text).to_hash();
            store.lock().unwrap().lppv_ean13s.extend(h);
            Ok(())
        })));

        // Swissmedic packages.xlsx — supplies GTIN, PRODNO, IT,
        // PackGrSwissmedic, EinheitSwissmedic, SubstanceSwissmedic,
        // CompositionSwissmedic per no8.
        jobs.push(("Swissmedic packages.xlsx", Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::SwissmedicDownloader::new(SwissmedicKind::Package)?;
            let path = d.download()?;
            let e = SwissmedicExtractor::new(&path, ExtKind::Package);
            let h = e.to_hash()?;
            store.lock().unwrap().swissmedic_packages.extend(h);
            Ok(())
        })));

        // Firstbase GS1 CSV — non-pharma items published by GS1 CH.
        if self.opts.firstbase {
            jobs.push(("Firstbase GS1 CSV", Box::new(|store: &Mutex<Inputs>| {
                let d = downloader::FirstbaseDownloader::new()?;
                let path = d.download()?;
                let h = FirstbaseExtractor::new(&path).to_hash()?;
                store.lock().unwrap().firstbase.extend(h);
                Ok(())
            })));
        }

        // ZurRose transfer.dat — supplies PHAR / PRICE / VAT / VDAT.
        let want_zurrose = self.opts.price.is_some()
            || self.opts.extended
            || self.opts.artikelstamm
            || self.opts.percent.is_some();
        if want_zurrose {
            let transfer_dat_path = self.opts.transfer_dat.clone();
            jobs.push(("ZurRose transfer.dat", Box::new(move |store: &Mutex<Inputs>| {
                let text = if let Some(path) = &transfer_dat_path {
                    // Operator passed a path — read it as ISO-8859 and decode.
                    let bytes = fs::read(path)
                        .with_context(|| format!("reading {path}"))?;
                    let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(&bytes);
                    cow.into_owned()
                } else {
                    let d = downloader::ZurroseDownloader::new()?;
                    d.download()?
                };
                let e = ZurroseExtractor::new(text, true, false);
                let h = e.to_hash();
                store.lock().unwrap().zurrose.extend(h);
                Ok(())
            })));
        }

        // Run the jobs in parallel.  Any single failure is logged but
        // does not abort the whole run — matches the Ruby behaviour of
        // warning and pressing on.  Progress: 5%–82% across the parallel
        // job set, leaving headroom for builder + sqlite write.
        let total = jobs.len() as f32;
        let done_counter = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        util::progress(0.05, format!("Starting {} parallel sources", jobs.len()));
        jobs.par_iter().for_each(|(label, job)| {
            if let Err(e) = job(&inputs) {
                util::log(format!("download/extract failed ({label}): {e}"));
                eprintln!("{e:#}");
            }
            let n = done_counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
            let frac = 0.05 + (n as f32 / total) * 0.77;
            util::progress(frac, format!("{label} done ({n}/{})", total as usize));
        });

        let mut inputs = inputs.into_inner().unwrap();
        inputs.release_date = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        if self.opts.log {
            eprintln!(
                "  sources: bag={}, refdata_pharma={}, refdata_nonpharma={}, swissmedic={}, zurrose={}, firstbase={}, epha={}, lppv={}",
                inputs.bag.len(),
                inputs.refdata_pharma.len(),
                inputs.refdata_nonpharma.len(),
                inputs.swissmedic_packages.len(),
                inputs.zurrose.len(),
                inputs.firstbase.len(),
                inputs.epha_interactions.len(),
                inputs.lppv_ean13s.len(),
            );
        }
        Ok(inputs)
    }
}
