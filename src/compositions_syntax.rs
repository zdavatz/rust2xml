//! Port of `lib/oddb2xml/compositions_syntax.rb` — the Parslet PEG
//! grammar for Swiss pharmaceutical compositions.
//!
//! Phase-6 deliverable.  This module currently exposes the public
//! constants the rest of the code needs (known abbreviations, units,
//! special substance names) so that callers compile.  The full grammar
//! lives in `src/compositions.pest` and is driven by `pest_derive`.

use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "compositions.pest"]
pub struct CompositionParser;

/// Substance-specific fixups the Ruby grammar hard-codes.  Left
/// intentionally empty while the pest grammar stabilises — builder
/// callsites receive `None` for unknown substances and fall back to the
/// raw string.
pub const PATCH_MAP: &[(&str, &str)] = &[];
