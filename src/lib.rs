//! rust2xml — Swiss drug database XML/DAT generator.
//!
//! Generates XML (+ legacy `.dat`) from public sources:
//! Refdata, BAG-XML / FOPH FHIR, Swissmedic, ZurRose, EPha, Migel, Firstbase.

pub mod version;

pub mod bag_fhir_extractor;
pub mod builder;
pub mod calc;
pub mod chapter_70_hack;
pub mod cli;
pub mod compare;
pub mod compositions_syntax;
pub mod compressor;
pub mod downloader;
pub mod extractor;
pub mod fhir_support;
pub mod foph_sl_downloader;
pub mod options;
pub mod parslet_compositions;
pub mod refdata_cleanup;
pub mod semantic_check;
pub mod util;
pub mod xml_definitions;

pub use options::Options;
pub use version::VERSION;

/// Top-level error type, mirroring `Oddb2xml::Error` in Ruby.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("ZIP error: {0}")]
    Zip(#[from] zip::result::ZipError),

    #[error("XML parse error: {0}")]
    Xml(#[from] quick_xml::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("CSV error: {0}")]
    Csv(#[from] csv::Error),

    #[error("Excel error: {0}")]
    Excel(#[from] calamine::Error),

    #[error("Excel xlsx error: {0}")]
    ExcelXlsx(#[from] calamine::XlsxError),

    #[error("date parse: {0}")]
    Date(#[from] chrono::ParseError),

    #[error("unsupported column layout: {0}")]
    Column(String),

    #[error("composition parse: {0}")]
    Composition(String),

    #[error("SHA256 mismatch in node {node}: expected {expected}, got {actual}")]
    Sha256Mismatch {
        node: String,
        expected: String,
        actual: String,
    },

    #[error("{0}")]
    Other(String),
}

pub type Result<T> = std::result::Result<T, Error>;
