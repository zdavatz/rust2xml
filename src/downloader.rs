//! Download adapters for the 11 Swiss drug data sources.
//! Port of `lib/oddb2xml/downloader.rb`.
//!
//! Design notes:
//!  * Everything is `blocking` — the Ruby original was synchronous and
//!    used threads at the CLI layer; the Rust CLI will do the same.
//!  * `skip_download` short-circuits a fetch when a cached copy is
//!    already sitting in `downloads/`.
//!  * Successful fetches copy the downloaded file into `downloads/`
//!    for the next run (same behaviour as `Oddb2xml.download_finished`).

use crate::util;
use anyhow::{Context, Result};
use reqwest::blocking::Client;
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

fn default_client() -> Result<Client> {
    let client = Client::builder()
        .cookie_store(true)
        .user_agent("Mozilla/5.0 (X11; Linux x86_64; rv:16.0) Gecko/20100101 Firefox/16.0")
        .timeout(Duration::from_secs(300))
        .redirect(reqwest::redirect::Policy::limited(5))
        .danger_accept_invalid_certs(true)
        .build()
        .context("building HTTP client")?;
    Ok(client)
}

/// Common helper: download `url` to `file`, respecting skip_download cache.
/// Returns the raw bytes of what was downloaded (or re-read from cache).
pub fn download_as<P: AsRef<Path>>(
    client: &Client,
    url: &str,
    file: P,
) -> Result<Vec<u8>> {
    let file = file.as_ref();
    let basename = file
        .file_name()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("download.tmp"));
    let work_dir = util::work_dir();
    let temp_file = work_dir.join(&basename);
    let dest = util::downloads_dir().join(&basename);

    if let Some(parent) = file.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent).ok();
        }
    }

    util::log(format!(
        "download_as {} from {}",
        basename.display(),
        url
    ));

    if util::skip_download_cached(file) {
        let data = fs::read(file)?;
        util::log(format!(
            "skip_download: reused cached {} ({} bytes)",
            file.display(),
            data.len()
        ));
        return Ok(data);
    }

    let mut resp = client
        .get(url)
        .send()
        .with_context(|| format!("GET {url}"))?;

    if !resp.status().is_success() {
        anyhow::bail!("HTTP {} fetching {}", resp.status(), url);
    }

    let mut buf: Vec<u8> = Vec::new();
    resp.read_to_end(&mut buf)?;

    // Write to the caller's path and also seed the downloads/ cache.
    {
        let mut out = File::create(file)
            .with_context(|| format!("creating {}", file.display()))?;
        out.write_all(&buf)?;
    }

    // Mirror Oddb2xml.download_finished: copy work-dir file to downloads/.
    let _ = fs::create_dir_all(&util::downloads_dir());
    if temp_file.exists() && temp_file != dest {
        let _ = fs::copy(&temp_file, &dest);
    } else if file != dest {
        let _ = fs::copy(file, &dest);
    }

    Ok(buf)
}

/// Base type mirroring the Ruby `Oddb2xml::Downloader` superclass.  Concrete
/// downloaders compose a `BaseDownloader` rather than inheriting.
pub struct BaseDownloader {
    pub client: Client,
    pub url: String,
}

impl BaseDownloader {
    pub fn new(url: impl Into<String>) -> Result<Self> {
        Ok(Self {
            client: default_client()?,
            url: url.into(),
        })
    }
}

// -----------------------------------------------------------------
// Concrete downloaders — real implementations to be filled in
// phase 3.  Each has its canonical URL and produces either bytes
// (for XML/CSV/text) or a file path (for large binaries like xlsx).
// -----------------------------------------------------------------

pub struct EphaDownloader {
    pub base: BaseDownloader,
}

impl EphaDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new(
                "https://raw.githubusercontent.com/zdavatz/oddb2xml_files/master/interactions_de_utf8.csv",
            )?,
        })
    }

    pub fn download(&self) -> Result<Vec<u8>> {
        let path = util::work_dir().join("epha_interactions.csv");
        download_as(&self.base.client, &self.base.url, &path)
    }
}

pub struct LppvDownloader {
    pub base: BaseDownloader,
}

impl LppvDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new(
                "https://raw.githubusercontent.com/zdavatz/oddb2xml_files/master/LPPV.txt",
            )?,
        })
    }

    pub fn download(&self) -> Result<Vec<u8>> {
        let path = util::work_dir().join("rust2xml_lppv.txt");
        download_as(&self.base.client, &self.base.url, &path)
    }
}

pub struct BagXmlDownloader {
    pub base: BaseDownloader,
}

impl BagXmlDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new(
                "https://www.spezialitaetenliste.ch/File.axd?file=XMLPublications.zip",
            )?,
        })
    }

    pub fn download(&self) -> Result<String> {
        let zip_path = util::work_dir().join("XMLPublications.zip");
        download_as(&self.base.client, &self.base.url, &zip_path)?;
        let xml = read_xml_from_zip(&zip_path, "Preparations.xml")?;
        Ok(xml)
    }
}

pub struct RefdataDownloader {
    pub base: BaseDownloader,
}

impl RefdataDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new(
                "https://files.refdata.ch/simis-public-prod/Articles/1.0/Refdata.Articles.zip",
            )?,
        })
    }

    pub fn download(&self) -> Result<String> {
        let zip_path = util::work_dir().join("Refdata.Articles.zip");
        download_as(&self.base.client, &self.base.url, &zip_path)?;
        read_xml_from_zip(&zip_path, "Refdata.Articles.xml")
    }
}

pub struct FirstbaseDownloader {
    pub base: BaseDownloader,
}

impl FirstbaseDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new("https://id.gs1.ch/01/07612345000961")?,
        })
    }

    pub fn download(&self) -> Result<PathBuf> {
        let path = util::downloads_dir().join("firstbase.csv");
        download_as(&self.base.client, &self.base.url, &path)?;
        Ok(path)
    }
}

/// Unpack a single entry matching `needle` from `zip_path` into
/// `downloads/` and return the UTF-8 text.
pub fn read_xml_from_zip(zip_path: &Path, needle: &str) -> Result<String> {
    let f = File::open(zip_path)
        .with_context(|| format!("opening zip {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(f)?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name.contains(needle) {
            let mut buf = String::new();
            entry.read_to_string(&mut buf)?;
            let basename = Path::new(&name)
                .file_name()
                .unwrap_or(Path::new(needle).as_os_str());
            let dest = util::downloads_dir().join(basename);
            let _ = fs::create_dir_all(util::downloads_dir());
            if let Err(e) = fs::write(&dest, &buf) {
                util::log(format!("failed to cache unzipped {}: {e}", dest.display()));
            }
            return Ok(buf);
        }
    }
    anyhow::bail!(
        "no entry matching '{}' in {}",
        needle,
        zip_path.display()
    )
}

// -----------------------------------------------------------------
// Swissmedic packages.xlsx / orphan.xlsx
// -----------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwissmedicKind {
    Package,
    Orphan,
}

pub struct SwissmedicDownloader {
    pub base: BaseDownloader,
    pub kind: SwissmedicKind,
    pub listing_url: String,
}

impl SwissmedicDownloader {
    pub fn new(kind: SwissmedicKind) -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new("")?,
            kind,
            listing_url: "https://www.swissmedic.ch/swissmedic/de/home/services/listen_neu.html"
                .into(),
        })
    }

    /// Scrape the current Packungen / Orphan Drug URL off Swissmedic's
    /// listing page (the direct URL rotates each release).
    fn resolve_direct_url(&self) -> Result<String> {
        let html = self.base.client.get(&self.listing_url).send()?.text()?;
        let doc = scraper::Html::parse_document(&html);
        let anchor_sel = scraper::Selector::parse("a").unwrap();
        let needle: &str = match self.kind {
            SwissmedicKind::Package => "Zugelassene Packungen",
            SwissmedicKind::Orphan => "Humanarzneimittel mit Status Orphan Drug",
        };
        for a in doc.select(&anchor_sel) {
            let text = a.text().collect::<String>();
            if text.contains(needle) {
                if let Some(href) = a.value().attr("href") {
                    return Ok(format!("https://www.swissmedic.ch{href}"));
                }
            }
        }
        anyhow::bail!("could not find Swissmedic link for {needle:?} on {}", self.listing_url)
    }

    pub fn download(&self) -> Result<PathBuf> {
        let direct = self.resolve_direct_url()?;
        let fname = match self.kind {
            SwissmedicKind::Package => "swissmedic_package.xlsx",
            SwissmedicKind::Orphan => "swissmedic_orphan.xlsx",
        };
        let path = util::downloads_dir().join(fname);
        download_as(&self.base.client, &direct, &path)?;
        Ok(path)
    }
}

// -----------------------------------------------------------------
// Migel non-pharma.xls (legacy — optional)
// -----------------------------------------------------------------

pub struct MigelDownloader {
    pub base: BaseDownloader,
}

impl MigelDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new(
                "https://github.com/zdavatz/oddb2xml_files/raw/master/NON-Pharma.xls",
            )?,
        })
    }

    pub fn download(&self) -> Result<PathBuf> {
        let path = util::downloads_dir().join("rust2xml_nonpharma.xls");
        download_as(&self.base.client, &self.base.url, &path)?;
        Ok(path)
    }
}

// -----------------------------------------------------------------
// ZurRose transfer.dat via FTP + ZIP.
// -----------------------------------------------------------------

pub struct ZurroseDownloader {
    pub base: BaseDownloader,
}

impl ZurroseDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new("http://pillbox.oddb.org/TRANSFER.ZIP")?,
        })
    }

    /// Fetch TRANSFER.ZIP, unzip `transfer.dat` into `downloads/` and
    /// return its text re-encoded as UTF-8.  Matches the Ruby
    /// `ZurroseDownloader`'s `File.open(dest, "r:iso-8859-1:utf-8").read`.
    pub fn download(&self) -> Result<String> {
        let zip_path = util::work_dir().join("transfer.zip");
        download_as(&self.base.client, &self.base.url, &zip_path)?;
        let f = File::open(&zip_path)?;
        let mut archive = zip::ZipArchive::new(f)?;
        for i in 0..archive.len() {
            let mut entry = archive.by_index(i)?;
            let name = entry.name().to_string();
            if !name.to_lowercase().contains("transfer.dat") {
                continue;
            }
            let mut raw: Vec<u8> = Vec::new();
            entry.read_to_end(&mut raw)?;
            let dest = util::downloads_dir().join("transfer.dat");
            let _ = fs::write(&dest, &raw);
            // ISO-8859-1 → UTF-8 conversion.
            let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(&raw);
            return Ok(cow.into_owned());
        }
        anyhow::bail!("TRANSFER.ZIP did not contain transfer.dat")
    }
}

// -----------------------------------------------------------------
// SwissmedicInfo (Fachinfo) — the Mechanize form flow for the
// http://download.swissmedicinfo.ch accept screen.  Ports
// `SwissmedicInfoDownloader` in downloader.rb.
// -----------------------------------------------------------------

pub struct SwissmedicInfoDownloader {
    pub base: BaseDownloader,
}

impl SwissmedicInfoDownloader {
    pub fn new() -> Result<Self> {
        Ok(Self {
            base: BaseDownloader::new(
                "http://download.swissmedicinfo.ch/Accept.aspx?ReturnUrl=%2f",
            )?,
        })
    }

    /// Two-step form submission on the Swissmedic Accept page.
    ///
    /// The Ruby version uses `Mechanize.form_with(id: 'Form1')` + button
    /// click twice; we parse the hidden fields and replay the POST.
    pub fn download(&self) -> Result<String> {
        let dest_zip = util::downloads_dir().join("swissmedic_info.zip");
        if dest_zip.exists() && util::skip_download_flag() {
            let xml = read_xml_from_zip(&dest_zip, "AipsDownload_")?;
            return Ok(xml);
        }

        let mut current_url = self.base.url.clone();
        for step in 0..2 {
            let html = self.base.client.get(&current_url).send()?.text()?;
            let doc = scraper::Html::parse_document(&html);
            let form_sel = scraper::Selector::parse("form#Form1").unwrap();
            let form = match doc.select(&form_sel).next() {
                Some(f) => f,
                None => anyhow::bail!("Form1 not found at step {step}"),
            };
            let action = form
                .value()
                .attr("action")
                .unwrap_or_else(|| self.base.url.as_str())
                .to_string();
            let input_sel = scraper::Selector::parse("input[type=hidden]").unwrap();
            let mut form_fields: Vec<(String, String)> = Vec::new();
            for inp in form.select(&input_sel) {
                if let (Some(n), Some(v)) = (
                    inp.value().attr("name"),
                    inp.value().attr("value"),
                ) {
                    form_fields.push((n.to_string(), v.to_string()));
                }
            }
            let btn_name = if step == 0 {
                "ctl00$MainContent$btnOK"
            } else {
                "ctl00$MainContent$BtnYes"
            };
            form_fields.push((btn_name.to_string(), "on".to_string()));
            let resp = self.base.client.post(&action).form(&form_fields).send()?;
            if step == 1 {
                let bytes = resp.bytes()?;
                fs::write(&dest_zip, &bytes)?;
            } else {
                current_url = resp.url().to_string();
            }
        }

        read_xml_from_zip(&dest_zip, "AipsDownload_")
    }
}

// -----------------------------------------------------------------
// Medregbm companies/persons — served behind NTLM.
// -----------------------------------------------------------------

pub struct MedregbmDownloader {
    pub base: BaseDownloader,
    pub kind: MedregKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MedregKind {
    Company,
    Person,
}

impl MedregbmDownloader {
    pub fn new(kind: MedregKind) -> Result<Self> {
        let action = match kind {
            MedregKind::Company => "CreateExcelListBetriebs",
            MedregKind::Person => "CreateExcelListMedizinalPersons",
        };
        let url = format!("https://www.medregbm.admin.ch/Publikation/{action}");
        Ok(Self {
            base: BaseDownloader::new(url)?,
            kind,
        })
    }

    pub fn download(&self) -> Result<String> {
        let fname = match self.kind {
            MedregKind::Company => "medregbm_company.txt",
            MedregKind::Person => "medregbm_person.txt",
        };
        let path = util::downloads_dir().join(fname);
        let bytes = download_as(&self.base.client, &self.base.url, &path)?;
        let (cow, _, _) = encoding_rs::WINDOWS_1252.decode(&bytes);
        Ok(cow.into_owned())
    }
}
