//! Thin alias module for `lib/oddb2xml/bag_fhir_extractor.rb`.  The
//! Ruby file was a near-duplicate of the canonical FHIR extractor that
//! referenced `FhirExtractor`; we re-export it under the same name here
//! so historical callsites still resolve.

pub use crate::fhir_support::{FhirExtractor as BagFhirExtractor, FhirDownloader as BagFhirDownloader};
