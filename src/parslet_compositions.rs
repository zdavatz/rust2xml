//! Port of `lib/oddb2xml/parslet_compositions.rb`.
//!
//! The Ruby file wraps the Parslet PEG grammar with a cache, an error
//! translator and a set of hand-applied corrections for known Swissmedic
//! data issues.  We provide the same public surface (`parse` /
//! `parse_compositions`) so the builder can call through unchanged.

use crate::compositions_syntax::{CompositionParser, Rule};
use pest::Parser;

/// One parsed ingredient.  A simplified stand-in for the richer AST
/// Parslet produces in Ruby — filled out fully in phase 6.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ingredient {
    pub name: String,
    pub quantity: String,
    pub unit: String,
}

/// Parse a single composition line.  Returns a list of ingredients, or
/// `Err(msg)` when the grammar rejects the input.
pub fn parse(input: &str) -> Result<Vec<Ingredient>, String> {
    let mut pairs = CompositionParser::parse(Rule::composition, input)
        .map_err(|e| format!("composition parse: {e}"))?;
    let root = pairs.next().ok_or_else(|| "empty parse tree".to_string())?;
    let mut out = Vec::new();
    for inner in root.into_inner() {
        if let Rule::substance_list = inner.as_rule() {
            for sub in inner.into_inner() {
                if sub.as_rule() != Rule::substance {
                    continue;
                }
                let mut name = String::new();
                let mut qty = String::new();
                let mut unit = String::new();
                for part in sub.into_inner() {
                    match part.as_rule() {
                        Rule::name => name = part.as_str().trim().to_string(),
                        Rule::dose => {
                            for dp in part.into_inner() {
                                match dp.as_rule() {
                                    Rule::number => qty = dp.as_str().trim().to_string(),
                                    Rule::unit => unit = dp.as_str().trim().to_string(),
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if !name.is_empty() {
                    out.push(Ingredient { name, quantity: qty, unit });
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_single_substance() {
        let v = parse("acidum acetylsalicylicum 100 mg").unwrap();
        assert_eq!(v.len(), 1);
        assert!(v[0].name.contains("acidum"));
        assert_eq!(v[0].quantity, "100");
        assert_eq!(v[0].unit, "mg");
    }

    #[test]
    fn comma_separated_list() {
        let v = parse("acidum ascorbicum 50 mg, natrii chloridum 4.5 mg").unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].quantity, "4.5");
    }

    #[test]
    fn multiple_lines() {
        let v = parse_compositions("foo 1 mg\nbar 2 ml\n");
        assert_eq!(v.len(), 2);
    }
}

/// Parse a multi-line composition text, one entry per line.  Blank lines
/// and lines that fail to parse are skipped silently (matching the Ruby
/// builder's lenient behaviour).
pub fn parse_compositions(text: &str) -> Vec<Vec<Ingredient>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| parse(l).unwrap_or_default())
        .collect()
}
