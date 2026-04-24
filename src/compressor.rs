//! Post-generation compression — ports `lib/oddb2xml/compressor.rb`.
//!
//! Picks tar.gz or zip based on `compress_ext`, writes the archive named
//! `{prefix}_{format}_{dd.mm.yyyy_HH.MM.{ext}}`, and removes the source
//! files on success (matching Ruby behaviour).

use crate::options::Format;
use anyhow::{Context, Result};
use chrono::Local;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressExt {
    TarGz,
    Zip,
}

impl CompressExt {
    pub fn extension(&self) -> &'static str {
        match self {
            Self::TarGz => "tar.gz",
            Self::Zip => "zip",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "tar.gz" | "tgz" | "tar" => Some(Self::TarGz),
            "zip" => Some(Self::Zip),
            _ => None,
        }
    }
}

pub struct Compressor {
    pub contents: Vec<PathBuf>,
    compress_file: PathBuf,
    ext: CompressExt,
}

impl Compressor {
    /// `prefix` is usually `"oddb"`. `format` is the output format (xml / dat)
    /// appearing in the filename.  `ext` defaults to tar.gz.
    pub fn new(
        prefix: impl AsRef<str>,
        format: Format,
        ext: Option<CompressExt>,
    ) -> Self {
        let ext = ext.unwrap_or(CompressExt::TarGz);
        let stamp = Local::now().format("%d.%m.%Y_%H.%M").to_string();
        let fmt = match format {
            Format::Xml => "xml",
            Format::Dat => "dat",
        };
        let filename = format!(
            "{prefix}_{fmt}_{stamp}.{ext}",
            prefix = prefix.as_ref(),
            fmt = fmt,
            stamp = stamp,
            ext = ext.extension()
        );
        Self {
            contents: Vec::new(),
            compress_file: PathBuf::from(filename),
            ext,
        }
    }

    /// Produce the archive, then delete each member file on success.
    /// Returns `Ok(false)` if `contents` is empty (Ruby analogue).
    pub fn finalize(&mut self) -> Result<bool> {
        if self.contents.is_empty() {
            return Ok(false);
        }
        match self.ext {
            CompressExt::TarGz => self.write_targz().with_context(|| {
                format!("writing tar.gz archive {}", self.compress_file.display())
            })?,
            CompressExt::Zip => self.write_zip().with_context(|| {
                format!("writing zip archive {}", self.compress_file.display())
            })?,
        }
        if self.compress_file.exists() {
            for file in &self.contents {
                if file.exists() {
                    let _ = fs::remove_file(file);
                }
            }
        }
        Ok(true)
    }

    fn write_targz(&self) -> Result<()> {
        let tgz = File::create(&self.compress_file)?;
        let enc = GzEncoder::new(tgz, Compression::default());
        let mut tar_builder = tar::Builder::new(enc);
        for path in &self.contents {
            if let Some(name) = path.file_name() {
                let mut f = File::open(path)
                    .with_context(|| format!("open {} for tar", path.display()))?;
                tar_builder.append_file(Path::new(name), &mut f)?;
            }
        }
        let enc = tar_builder.into_inner()?;
        enc.finish()?.flush()?;
        Ok(())
    }

    fn write_zip(&self) -> Result<()> {
        let f = File::create(&self.compress_file)?;
        let mut zip = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for path in &self.contents {
            let name = path
                .file_name()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            zip.start_file(name, opts)?;
            let mut f = File::open(path)?;
            io::copy(&mut f, &mut zip)?;
        }
        zip.finish()?;
        Ok(())
    }

    pub fn compress_file(&self) -> &Path {
        &self.compress_file
    }
}
