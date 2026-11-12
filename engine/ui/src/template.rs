//! Text templates: literal text with `{name}` and `{name:N}` slots filled from [`Bindings`].
//!
//! Syntax: `{name}` prints a bound number with no decimals or a bound text as it is; `{name:2}`
//! prints a number with two decimals; `{{` and `}}` print a brace. Anything else inside braces is a
//! parse error, so a typo is found when the document loads, not when it is drawn.
//!
//! Invariant: a parsed template never holds an empty or malformed slot.

use std::fmt::Write;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::bindings::{Bindings, ListRecord, Value};
use crate::error::UiError;

/// One piece of a template.
#[derive(Clone, Debug, PartialEq)]
enum Segment {
    Literal(String),
    Slot { name: String, decimals: usize },
}

/// A parsed text template.
#[derive(Clone, Debug, PartialEq)]
pub struct Template {
    segments: Vec<Segment>,
}

/// The most decimals a slot may ask for.
const MAX_DECIMALS: usize = 6;

impl Template {
    /// Parses `source`.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::DocParse`] for an unclosed brace, an empty or invalid slot name, or a
    /// decimals count that is not a number from 0 to 6.
    pub fn parse(source: &str) -> Result<Self, UiError> {
        let mut segments = Vec::new();
        let mut literal = String::new();
        let mut chars = source.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    literal.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    literal.push('}');
                }
                '{' => {
                    let mut slot = String::new();
                    let mut closed = false;
                    for inner in chars.by_ref() {
                        if inner == '}' {
                            closed = true;
                            break;
                        }
                        slot.push(inner);
                    }
                    if !closed {
                        return Err(bad(source, "unclosed `{`"));
                    }
                    if !literal.is_empty() {
                        segments.push(Segment::Literal(std::mem::take(&mut literal)));
                    }
                    segments.push(parse_slot(source, &slot)?);
                }
                '}' => return Err(bad(source, "unmatched `}`")),
                other => literal.push(other),
            }
        }
        if !literal.is_empty() {
            segments.push(Segment::Literal(literal));
        }
        Ok(Self { segments })
    }

    /// The template as source text that [`Template::parse`] reads back to an equal template.
    #[must_use]
    pub fn source(&self) -> String {
        let mut out = String::new();
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => {
                    for c in text.chars() {
                        match c {
                            '{' => out.push_str("{{"),
                            '}' => out.push_str("}}"),
                            other => out.push(other),
                        }
                    }
                }
                Segment::Slot { name, decimals } => {
                    out.push('{');
                    out.push_str(name);
                    if *decimals > 0 {
                        out.push(':');
                        out.push_str(&decimals.to_string());
                    }
                    out.push('}');
                }
            }
        }
        out
    }

    /// Names of the variables this template reads.
    pub fn variables(&self) -> impl Iterator<Item = &str> {
        self.segments.iter().filter_map(|s| match s {
            Segment::Slot { name, .. } => Some(name.as_str()),
            Segment::Literal(_) => None,
        })
    }

    /// Appends the filled-in text to `out`. `node` names the node in errors.
    ///
    /// # Errors
    ///
    /// Returns [`UiError::UnboundVariable`] when a slot's name is not bound, and
    /// [`UiError::BindingType`] when it is bound to a flag.
    pub fn render(&self, bindings: &Bindings, node: &str, out: &mut String) -> Result<(), UiError> {
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Slot { name, decimals } => match bindings.get(name) {
                    Some(Value::Num(n)) => write!(out, "{n:.decimals$}")?,
                    Some(Value::Text(t)) => out.push_str(t),
                    Some(Value::Flag(_)) => {
                        return Err(UiError::BindingType {
                            node: node.to_owned(),
                            name: name.clone(),
                            expected: "number or text",
                        });
                    }
                    Some(Value::List(_)) => {
                        return Err(UiError::BindingType {
                            node: node.to_owned(),
                            name: name.clone(),
                            expected: "number or text",
                        });
                    }
                    None => {
                        return Err(UiError::UnboundVariable {
                            node: node.to_owned(),
                            name: name.clone(),
                        });
                    }
                },
            }
        }
        Ok(())
    }

    /// Appends text while resolving `{item.field}` slots from one repeated-list row.
    pub fn render_list_row(
        &self,
        bindings: &Bindings,
        row: &ListRecord,
        node: &str,
        out: &mut String,
    ) -> Result<(), UiError> {
        for segment in &self.segments {
            match segment {
                Segment::Literal(text) => out.push_str(text),
                Segment::Slot { name, decimals } if name.starts_with("item.") => {
                    let field = name.strip_prefix("item.").unwrap_or(name);
                    match row.get(field) {
                        Some(Value::Num(value)) => write!(out, "{value:.decimals$}")?,
                        Some(Value::Text(value)) => out.push_str(value),
                        Some(Value::Flag(value)) => {
                            out.push_str(if *value { "true" } else { "false" })
                        }
                        Some(Value::List(_)) => {
                            return Err(UiError::BindingType {
                                node: node.to_owned(),
                                name: name.clone(),
                                expected: "number, flag or text",
                            });
                        }
                        None => {
                            return Err(UiError::UnboundVariable {
                                node: node.to_owned(),
                                name: name.clone(),
                            });
                        }
                    }
                }
                Segment::Slot { name, decimals } => match bindings.get(name) {
                    Some(Value::Num(value)) => write!(out, "{value:.decimals$}")?,
                    Some(Value::Text(value)) => out.push_str(value),
                    Some(Value::Flag(_)) | Some(Value::List(_)) => {
                        return Err(UiError::BindingType {
                            node: node.to_owned(),
                            name: name.clone(),
                            expected: "number or text",
                        });
                    }
                    None => {
                        return Err(UiError::UnboundVariable {
                            node: node.to_owned(),
                            name: name.clone(),
                        });
                    }
                },
            }
        }
        Ok(())
    }
}

fn bad(source: &str, why: &str) -> UiError {
    UiError::DocParse(format!("text {source:?}: {why}"))
}

fn parse_slot(source: &str, slot: &str) -> Result<Segment, UiError> {
    let (name, decimals) = match slot.split_once(':') {
        Some((name, places)) => {
            let decimals: usize = places.parse().map_err(|e| {
                UiError::DocParse(format!("text {source:?}: decimals {places:?}: {e}"))
            })?;
            if decimals > MAX_DECIMALS {
                return Err(bad(source, "at most 6 decimals"));
            }
            (name, decimals)
        }
        None => (slot, 0),
    };
    let valid = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.');
    if !valid {
        return Err(bad(source, "a slot name is letters, digits, `_` or `.`"));
    }
    Ok(Segment::Slot {
        name: name.to_owned(),
        decimals,
    })
}

impl Serialize for Template {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.source())
    }
}

impl<'de> Deserialize<'de> for Template {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let source = String::deserialize(deserializer)?;
        Self::parse(&source).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn fill(source: &str, b: &Bindings) -> Result<String, UiError> {
        let mut out = String::new();
        Template::parse(source)?.render(b, "n", &mut out)?;
        Ok(out)
    }

    #[test]
    fn literal_text_passes_through() -> Result<(), UiError> {
        assert_eq!(fill("FIRE", &Bindings::new())?, "FIRE");
        Ok(())
    }

    #[test]
    fn slots_fill_with_decimals() -> Result<(), UiError> {
        let mut b = Bindings::new();
        b.set_num("ammo", 29.0);
        b.set_num("t", 1.236);
        b.set_text("who", "BOT");
        assert_eq!(fill("{ammo} / 90", &b)?, "29 / 90");
        assert_eq!(fill("{t:2}s {who}", &b)?, "1.24s BOT");
        Ok(())
    }

    #[test]
    fn doubled_braces_print_one() -> Result<(), UiError> {
        assert_eq!(fill("{{x}}", &Bindings::new())?, "{x}");
        Ok(())
    }

    #[test]
    fn malformed_templates_are_errors() {
        for bad in ["{", "}", "{}", "{a b}", "{a:x}", "{a:9}", "{a:-1}"] {
            assert!(Template::parse(bad).is_err(), "{bad:?} should not parse");
        }
    }

    #[test]
    fn an_unbound_slot_names_the_variable() {
        let err = fill("{ammo}", &Bindings::new()).unwrap_err();
        assert!(matches!(err, UiError::UnboundVariable { name, .. } if name == "ammo"));
    }

    #[test]
    fn a_flag_in_a_text_slot_is_a_type_error() {
        let mut b = Bindings::new();
        b.set_flag("held", true);
        assert!(matches!(
            fill("{held}", &b),
            Err(UiError::BindingType { .. })
        ));
    }

    #[test]
    fn variables_lists_every_slot() -> Result<(), UiError> {
        let t = Template::parse("{a} and {b:1}")?;
        assert_eq!(t.variables().collect::<Vec<_>>(), ["a", "b"]);
        Ok(())
    }
}
