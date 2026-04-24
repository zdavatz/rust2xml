//! Port of `lib/oddb2xml/foph_sl_downloader.rb`.
//!
//! The Ruby file was a 43-line stub scaffolded for the older FOPH
//! endpoint.  It has been superseded by [`FhirDownloader`] but is
//! retained for compatibility with `test_fhir_standalone.rb`.

use anyhow::Result;

pub struct FophSlDownloader {
    pub url: String,
    pub client: reqwest::blocking::Client,
}

impl FophSlDownloader {
    pub fn new(url: impl Into<String>) -> Result<Self> {
        let client = reqwest::blocking::Client::builder()
            .user_agent("rust2xml/foph-sl")
            .timeout(std::time::Duration::from_secs(600))
            .build()?;
        Ok(Self { url: url.into(), client })
    }

    pub fn download(&self) -> Result<String> {
        let resp = self.client.get(&self.url).send()?;
        Ok(resp.text()?)
    }
}
