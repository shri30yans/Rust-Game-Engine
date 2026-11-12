//! A small CSS reader on top of `cssparser`'s tokenizer: rules of simple selectors (`tag`,
//! `.class`, `#id`, comma lists) holding `name: value value;` declarations.
//!
//! Invariants: anything outside that shape (combinators, pseudo-classes, at-rules, `!important`,
//! functions) is an error, so the cascade stays a plain specificity sort.

use cssparser::{Delimiter, ParseError, ParseErrorKind, Parser, Token};

use crate::error::CookError;

/// A value in a declaration.
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A length in px.
    Px(f32),
    /// A length in vmin.
    Vmin(f32),
    /// A percentage as a fraction (`50%` is `0.5`).
    Percent(f32),
    /// A bare number.
    Number(f32),
    /// A keyword, lower-cased.
    Ident(String),
    /// A hash colour, with the leading `#`.
    Color(String),
    /// `skewX(<n>deg)`, the angle in degrees with CSS's sign.
    SkewX(f32),
    /// `scale(<n>)`, supported only in a widget state rule.
    Scale(f32),
    /// `linear-gradient(<n>deg, <#colour> <n>%, ...)`: the CSS angle, then each stop's
    /// colour (with its leading `#`) and position as a fraction, in order.
    Gradient {
        /// Degrees, CSS convention: 0 points to the top, 90 to the right.
        angle_deg: f32,
        /// Colour and position of each stop.
        stops: Vec<(String, f32)>,
    },
}

/// One `name: values` pair.
#[derive(Clone, Debug, PartialEq)]
pub struct Declaration {
    /// The property, lower-cased.
    pub name: String,
    /// The values, in order.
    pub values: Vec<Value>,
}

/// A simple selector.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Selector {
    /// Every element (the CSS universal selector).
    Universal,
    /// An element name.
    Tag(String),
    /// A class.
    Class(String),
    /// An id.
    Id(String),
    /// A supported focus or pointer state applied to a simple widget selector.
    State {
        base: Box<Selector>,
        state: WidgetState,
    },
}

/// A CSS interaction state supported for menu widgets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WidgetState {
    Hover,
    FocusVisible,
    Active,
    Disabled,
}

impl Selector {
    /// Cascade weight: id beats class beats tag.
    #[must_use]
    pub const fn specificity(&self) -> u8 {
        match self {
            Self::Universal => 0,
            Self::Tag(_) => 1,
            Self::Class(_) => 10,
            Self::Id(_) => 100,
            Self::State { base, .. } => base.specificity() + 10,
        }
    }
}

/// A rule: selectors sharing a declaration block.
#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    /// The selectors, any of which matches.
    pub selectors: Vec<Selector>,
    /// The declarations.
    pub declarations: Vec<Declaration>,
}

fn css_err(e: ParseError<CookError>) -> CookError {
    match e.kind {
        ParseErrorKind::Custom(c) => c,
        ParseErrorKind::Basic(b) => CookError::Css {
            reason: format!("{b:?}"),
        },
    }
}

/// Reads a stylesheet.
///
/// # Errors
///
/// Returns [`CookError::Css`] for malformed CSS and [`CookError::UnsupportedSelector`] for a
/// selector outside the subset.
pub fn parse_stylesheet(text: &str) -> Result<Vec<Rule>, CookError> {
    let mut p = Parser::new(text);
    let mut rules = Vec::new();
    loop {
        p.skip_whitespace();
        if p.is_exhausted() {
            return Ok(rules);
        }
        let selectors = p
            .parse_until_before(Delimiter::CurlyBracketBlock, parse_selectors)
            .map_err(css_err)?;
        p.expect_curly_bracket_block()
            .map_err(|e| css_err(e.into()))?;
        let declarations = p.parse_nested_block(parse_body).map_err(css_err)?;
        rules.push(Rule {
            selectors,
            declarations,
        });
    }
}

/// Reads the text of a `style` attribute.
///
/// # Errors
///
/// Returns [`CookError::Css`] for malformed declarations.
pub fn parse_declarations(text: &str) -> Result<Vec<Declaration>, CookError> {
    let mut p = Parser::new(text);
    parse_body(&mut p).map_err(css_err)
}

fn unsupported(what: String) -> ParseError<CookError> {
    ParseError::custom(CookError::UnsupportedSelector(what))
}

fn parse_selectors<'i>(p: &mut Parser<'i>) -> Result<Vec<Selector>, ParseError<CookError>> {
    let mut list = Vec::new();
    loop {
        p.skip_whitespace();
        let mut selector = match p.next()?.clone() {
            Token::Ident(name) => Selector::Tag(name.to_ascii_lowercase()),
            Token::Delim('*') => Selector::Universal,
            Token::IDHash(name) => Selector::Id(name.to_string()),
            Token::Delim('.') => Selector::Class(p.expect_ident()?.to_string()),
            other => return Err(unsupported(format!("{other:?}"))),
        };
        p.skip_whitespace();
        if !p.is_exhausted() {
            match p.next()?.clone() {
                Token::Colon | Token::Delim(':') => {
                    let state = match p.expect_ident()?.to_ascii_lowercase().as_str() {
                        "hover" => WidgetState::Hover,
                        "focus-visible" => WidgetState::FocusVisible,
                        "active" => WidgetState::Active,
                        "disabled" => WidgetState::Disabled,
                        other => return Err(unsupported(format!("pseudo-class :{other}"))),
                    };
                    selector = Selector::State {
                        base: Box::new(selector),
                        state,
                    };
                    p.skip_whitespace();
                }
                Token::Comma => {
                    list.push(selector);
                    continue;
                }
                other => {
                    return Err(unsupported(format!(
                        "combinator or trailing token {other:?}"
                    )));
                }
            }
        }
        list.push(selector);
        p.skip_whitespace();
        if p.is_exhausted() {
            return Ok(list);
        }
        match p.next()?.clone() {
            Token::Comma => {}
            other => return Err(unsupported(format!("{other:?} after a selector"))),
        }
    }
}

fn parse_body<'i>(p: &mut Parser<'i>) -> Result<Vec<Declaration>, ParseError<CookError>> {
    let mut out = Vec::new();
    loop {
        p.skip_whitespace();
        if p.is_exhausted() {
            return Ok(out);
        }
        let name = p.expect_ident()?.to_ascii_lowercase();
        p.expect_colon()?;
        let values = p.parse_until_before(Delimiter::Semicolon, parse_values)?;
        out.push(Declaration { name, values });
        if !p.is_exhausted() {
            p.expect_semicolon()?;
        }
    }
}

fn parse_values<'i>(p: &mut Parser<'i>) -> Result<Vec<Value>, ParseError<CookError>> {
    let mut out = Vec::new();
    while !p.is_exhausted() {
        let token = p.next()?.clone();
        let value = match token {
            Token::Dimension {
                value, ref unit, ..
            } if unit.eq_ignore_ascii_case("px") => Value::Px(value),
            Token::Dimension {
                value, ref unit, ..
            } if unit.eq_ignore_ascii_case("vmin") => Value::Vmin(value),
            Token::Percentage { unit_value, .. } => Value::Percent(unit_value),
            Token::Number { value, .. } => Value::Number(value),
            Token::Ident(name) => Value::Ident(name.to_ascii_lowercase()),
            Token::Hash(hex) | Token::IDHash(hex) => Value::Color(format!("#{hex}")),
            Token::Function(name) if name.eq_ignore_ascii_case("skewx") => {
                let degrees = p.parse_nested_block(|inner| match inner.next()?.clone() {
                    Token::Dimension {
                        value, ref unit, ..
                    } if unit.eq_ignore_ascii_case("deg") => Ok(value),
                    Token::Number { value: 0.0, .. } => Ok(0.0),
                    other => Err(ParseError::custom(CookError::Css {
                        reason: format!("skewX takes an angle in deg, got {other:?}"),
                    })),
                })?;
                Value::SkewX(degrees)
            }
            Token::Function(name) if name.eq_ignore_ascii_case("linear-gradient") => {
                p.parse_nested_block(parse_linear_gradient)?
            }
            Token::Function(name) if name.eq_ignore_ascii_case("scale") => {
                let scale = p.parse_nested_block(|inner| match inner.next()?.clone() {
                    Token::Number { value, .. } => Ok(value),
                    other => Err(ParseError::custom(CookError::Css {
                        reason: format!("scale takes a number, got {other:?}"),
                    })),
                })?;
                Value::Scale(scale)
            }
            other => {
                return Err(ParseError::custom(CookError::Css {
                    reason: format!("unsupported value {other:?}"),
                }));
            }
        };
        out.push(value);
    }
    Ok(out)
}

/// A gradient syntax error; the reason names the `background` property it belongs to.
fn gradient_error(reason: String) -> ParseError<CookError> {
    ParseError::custom(CookError::Css {
        reason: format!("background: {reason}"),
    })
}

/// Reads the next token of a gradient, or a `background` error naming what was expected.
fn next_gradient_token<'i>(
    inner: &mut Parser<'i>,
    expected: &str,
) -> Result<Token<'i>, ParseError<CookError>> {
    inner
        .next()
        .cloned()
        .map_err(|_| gradient_error(format!("linear-gradient is missing {expected}")))
}

/// Reads the inside of `linear-gradient(...)`: an angle in deg, then comma-separated
/// `#colour <n>%` stops. Stop count and order are checked when the cooker builds the brush.
fn parse_linear_gradient(inner: &mut Parser<'_>) -> Result<Value, ParseError<CookError>> {
    let angle_deg = match next_gradient_token(inner, "an angle in deg")? {
        Token::Dimension {
            value, ref unit, ..
        } if unit.eq_ignore_ascii_case("deg") => value,
        other => {
            return Err(gradient_error(format!(
                "linear-gradient starts with an angle in deg, got {other:?}"
            )));
        }
    };
    let mut stops = Vec::new();
    while !inner.is_exhausted() {
        inner.expect_comma()?;
        let colour = match next_gradient_token(inner, "a #colour")? {
            Token::Hash(hex) | Token::IDHash(hex) => format!("#{hex}"),
            other => {
                return Err(gradient_error(format!(
                    "a gradient stop starts with a #colour, got {other:?}"
                )));
            }
        };
        let position = match next_gradient_token(inner, "a percentage position")? {
            Token::Percentage { unit_value, .. } => unit_value,
            other => {
                return Err(gradient_error(format!(
                    "a gradient stop needs a percentage position, got {other:?}"
                )));
            }
        };
        stops.push((colour, position));
    }
    Ok(Value::Gradient { angle_deg, stops })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn rules_and_declarations_parse() -> Result<(), CookError> {
        let rules = parse_stylesheet(
            "#hud, .row { width: 50%; height: 40px; background-color: #ff000080; }\n p{font-size:20px}",
        )?;
        assert_eq!(rules.len(), 2);
        assert_eq!(
            rules[0].selectors,
            vec![Selector::Id("hud".into()), Selector::Class("row".into())]
        );
        assert_eq!(rules[0].declarations[0].values, vec![Value::Percent(0.5)]);
        assert_eq!(rules[0].declarations[1].values, vec![Value::Px(40.0)]);
        assert_eq!(
            rules[0].declarations[2].values,
            vec![Value::Color("#ff000080".into())]
        );
        assert_eq!(rules[1].selectors, vec![Selector::Tag("p".into())]);
        Ok(())
    }

    #[test]
    fn a_style_attribute_parses_with_or_without_the_last_semicolon() -> Result<(), CookError> {
        assert_eq!(parse_declarations("gap: 8px; padding: 4px")?.len(), 2);
        assert_eq!(parse_declarations("gap: 8px;")?.len(), 1);
        Ok(())
    }

    #[test]
    fn unsupported_selectors_are_named() {
        for css in [
            "div p { gap: 1px }",
            "a:visited { gap: 1px }",
            "@media x { p { gap: 1px } }",
        ] {
            assert!(
                matches!(
                    parse_stylesheet(css),
                    Err(CookError::UnsupportedSelector(_))
                ),
                "{css}"
            );
        }
    }

    #[test]
    fn a_linear_gradient_parses_into_its_angle_and_stops() -> Result<(), CookError> {
        let rules = parse_stylesheet(
            "p { background: linear-gradient(90deg, #000000 0%, #ffffff 100%); }",
        )?;
        assert_eq!(
            rules[0].declarations[0].values,
            vec![Value::Gradient {
                angle_deg: 90.0,
                stops: vec![("#000000".into(), 0.0), ("#ffffff".into(), 1.0)],
            }]
        );
        Ok(())
    }

    #[test]
    fn malformed_css_is_an_error() {
        assert!(matches!(
            parse_stylesheet("p { gap 1px }"),
            Err(CookError::Css { .. })
        ));
        assert!(matches!(
            parse_stylesheet("p { gap: calc(1px) }"),
            Err(CookError::Css { .. })
        ));
    }
}
