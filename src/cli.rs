//! Pipeline orchestration — port of `lib/oddb2xml/cli.rb`.
//!
//! Downloads data sources in parallel, runs each through its extractor,
//! hands the normalized maps to `Builder`, writes each output file
//! into the current working directory, optionally compresses the
//! bundle.  Mirrors the Ruby `Cli#run` method.

use crate::builder::{Builder, Inputs};
use crate::compressor::{CompressExt, Compressor};
use crate::downloader;
use crate::extractor::{
    BagXmlExtractor, EphaExtractor, LppvExtractor, RefdataExtractor,
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
                let jobs: Vec<(&str, Box<dyn Fn() -> Result<String> + Send + Sync>)> = vec![
                    ("oddb_product.xml", Box::new(|| b.build_product().map_err(Into::into))),
                    ("oddb_article.xml", Box::new(|| b.build_article().map_err(Into::into))),
                    ("oddb_substance.xml", Box::new(|| b.build_substance().map_err(Into::into))),
                    ("oddb_limitation.xml", Box::new(|| b.build_limitation().map_err(Into::into))),
                    ("oddb_interaction.xml", Box::new(|| b.build_interaction().map_err(Into::into))),
                    ("oddb_code.xml", Box::new(|| b.build_code().map_err(Into::into))),
                ];
                for (name, task) in jobs {
                    let xml = task()?;
                    let path = PathBuf::from(name);
                    fs::write(&path, xml)
                        .with_context(|| format!("writing {}", path.display()))?;
                    outputs.push(path);
                }
                if self.opts.calc || self.opts.extended {
                    let path = PathBuf::from("oddb_calc.xml");
                    fs::write(&path, b.build_calc()?)?;
                    outputs.push(path);
                }
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

        // Describe each job as a boxed closure that mutates `inputs`.
        type Job = Box<dyn Fn(&Mutex<Inputs>) -> Result<()> + Send + Sync>;
        let mut jobs: Vec<Job> = Vec::new();

        let use_fhir = self.opts.fhir;
        let fhir_url = self.opts.fhir_url.clone();

        if use_fhir {
            let url = fhir_url.unwrap_or_else(|| DEFAULT_FHIR_URL.to_string());
            jobs.push(Box::new(move |store: &Mutex<Inputs>| {
                let d = FhirDownloader::new(url.clone())?;
                let body = d.download()?;
                let e = FhirExtractor::new(body);
                let bag = e.to_hash()?;
                store.lock().unwrap().bag.extend(bag);
                Ok(())
            }));
        } else {
            jobs.push(Box::new(|store: &Mutex<Inputs>| {
                let d = downloader::BagXmlDownloader::new()?;
                let xml = d.download()?;
                let e = BagXmlExtractor::new(xml);
                let bag = e.to_hash()?;
                store.lock().unwrap().bag.extend(bag);
                Ok(())
            }));
        }

        jobs.push(Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::RefdataDownloader::new()?;
            let xml = d.download()?;
            let pharma = RefdataExtractor::new(xml.clone(), "PHARMA").to_hash()?;
            let non = RefdataExtractor::new(xml, "NONPHARMA").to_hash()?;
            let mut s = store.lock().unwrap();
            s.refdata_pharma.extend(pharma);
            s.refdata_nonpharma.extend(non);
            Ok(())
        }));

        jobs.push(Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::EphaDownloader::new()?;
            let bytes = d.download()?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let v = EphaExtractor::new(text).to_vec();
            store.lock().unwrap().epha_interactions.extend(v);
            Ok(())
        }));

        jobs.push(Box::new(|store: &Mutex<Inputs>| {
            let d = downloader::LppvDownloader::new()?;
            let bytes = d.download()?;
            let text = String::from_utf8_lossy(&bytes).into_owned();
            let h = LppvExtractor::new(text).to_hash();
            store.lock().unwrap().lppv_ean13s.extend(h);
            Ok(())
        }));

        // Run the jobs in parallel.  Any single failure is logged but
        // does not abort the whole run — matches the Ruby behaviour of
        // warning and pressing on.
        jobs.par_iter().for_each(|job| {
            if let Err(e) = job(&inputs) {
                util::log(format!("download/extract failed: {e}"));
                eprintln!("{e:#}");
            }
        });

        let mut inputs = inputs.into_inner().unwrap();
        inputs.release_date = chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string();
        Ok(inputs)
    }
}
