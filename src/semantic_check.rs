//! Post-generation validator.  Port of `lib/oddb2xml/semantic_check.rb`.
//!
//! Two checks the Ruby version exposes:
//!   * `every_product_number_is_unique` — each `PRODNO` attribute shows
//!     up at most once.
//!   * `every_item_number_is_unique` — each `GTIN` / `PHAR` shows up at
//!     most once.

use anyhow::{Context, Result};
use quick_xml::events::Event;
use quick_xml::Reader;
use std::collections::HashSet;
use std::path::Path;

pub struct SemanticCheck {
    pub path: std::path::PathBuf,
}

impl SemanticCheck {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self { path: path.as_ref().to_path_buf() }
    }

    pub fn every_product_number_is_unique(&self) -> Result<bool> {
        self.assert_unique("PRODNO")
    }

    pub fn every_item_number_is_unique(&self) -> Result<bool> {
        self.assert_unique("GTIN")
    }

    fn assert_unique(&self, element: &str) -> Result<bool> {
        let mut reader = Reader::from_file(&self.path)
            .with_context(|| format!("opening {}", self.path.display()))?;
        reader.config_mut().trim_text(true);
        let mut seen: HashSet<String> = HashSet::new();
        let mut buf = Vec::new();
        let mut in_target = false;
        loop {
            match reader.read_event_into(&mut buf) {
                Ok(Event::Start(e)) if e.name().as_ref() == element.as_bytes() => {
                    in_target = true;
                }
                Ok(Event::End(e)) if e.name().as_ref() == element.as_bytes() => {
                    in_target = false;
                }
                Ok(Event::Text(t)) if in_target => {
                    let val = String::from_utf8_lossy(&t).to_string();
                    if !seen.insert(val) {
                        return Ok(false);
                    }
                }
                Ok(Event::Eof) => break,
                Err(e) => return Err(e.into()),
                _ => {}
            }
            buf.clear();
        }
        Ok(true)
    }
}
