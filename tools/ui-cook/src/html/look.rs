//! Reads an element's layout and visual style from its declarations.
//! Invariant: every property is either mapped here or rejected by the caller as unsupported.

use engine_ui::doc::{
    Align, Border, Color, Direction, Display, Justify, LayoutDef, Position, StateStyles, StyleDef,
    TextAlign, TextFit, TextOverflow,
};

use crate::css::Value;
use crate::error::CookError;

use super::props::Props;
use super::state::state_delta;

/// The layout, style and text properties of one element.
pub(super) struct Look {
    pub(super) layout: LayoutDef,
    pub(super) style: StyleDef,
    pub(super) font_size: Option<f32>,
    pub(super) color: Option<Color>,
    pub(super) text_align: TextAlign,
    pub(super) fit: TextFit,
    pub(super) text_overflow: TextOverflow,
}

pub(super) fn look(p: &mut Props) -> Result<Look, CookError> {
    let mut layout = LayoutDef {
        display: match p.keyword("display")?.as_deref() {
            None | Some("block") => Display::Block,
            Some("flex") => Display::Flex,
            Some(_) => return Err(p.bad("display", "block or flex")),
        },
        ..LayoutDef::default()
    };
    if let Some(w) = p.len("width")? {
        layout.width = w;
    }
    if let Some(h) = p.len("height")? {
        layout.height = h;
    }
    let has_flex_direction = p.props.contains_key("flex-direction");
    layout.direction = match p.keyword("flex-direction")?.as_deref() {
        None | Some("row") => Direction::Row,
        Some("column") => Direction::Column,
        Some(_) => return Err(p.bad("flex-direction", "row or column")),
    };
    if layout.display == Display::Block && has_flex_direction {
        return Err(p.bad("flex-direction", "only valid on a flex container"));
    }
    layout.align = match p.keyword("align-items")?.as_deref() {
        None | Some("stretch") => Align::Stretch,
        Some("flex-start") => Align::Start,
        Some("center") => Align::Center,
        Some("flex-end") => Align::End,
        Some(_) => return Err(p.bad("align-items", "flex-start, center, flex-end or stretch")),
    };
    layout.justify = match p.keyword("justify-content")?.as_deref() {
        None | Some("flex-start") => Justify::Start,
        Some("center") => Justify::Center,
        Some("flex-end") => Justify::End,
        Some("space-between") => Justify::SpaceBetween,
        Some("space-around") => Justify::SpaceAround,
        Some(_) => return Err(p.bad("justify-content", "a flex-start..space-around keyword")),
    };
    layout.gap = p.px("gap")?.unwrap_or(0.0);
    layout.padding = p.px("padding")?.unwrap_or(0.0);
    layout.margin = p.px("margin")?.unwrap_or(0.0);
    layout.grow = p.number("flex-grow")?.unwrap_or(0.0);
    if let Some(line_height) = p.number("line-height")?
        && line_height != 1.0
    {
        return Err(p.bad("line-height", "1 in the UI subset"));
    }
    if let Some(value) = p.keyword("box-sizing")?
        && value != "border-box"
    {
        return Err(p.bad("box-sizing", "border-box"));
    }
    if let Some(value) = p.keyword("appearance")?
        && value != "none"
    {
        return Err(p.bad("appearance", "none"));
    }
    if let Some(value) = p.keyword("font")?
        && value != "inherit"
    {
        return Err(p.bad("font", "inherit"));
    }

    let edges = [
        p.len("left")?,
        p.len("top")?,
        p.len("right")?,
        p.len("bottom")?,
    ];
    layout.position = match p.keyword("position")?.as_deref() {
        None | Some("relative") if edges.iter().all(Option::is_none) => Position::Flow,
        None | Some("relative") => {
            return Err(p.bad("left/top/right/bottom", "position: absolute"));
        }
        Some("absolute") => Position::Absolute {
            left: edges[0],
            top: edges[1],
            right: edges[2],
            bottom: edges[3],
        },
        Some(_) => return Err(p.bad("position", "relative or absolute")),
    };

    let text_color = match p.one("color")? {
        None => None,
        Some(Value::Ident(value)) if value == "inherit" => None,
        Some(Value::Color(hex)) => Some(Color::parse(&hex)?),
        Some(_) => return Err(p.bad("color", "a colour or inherit")),
    };
    let mut style = StyleDef {
        fill: p.color("background-color")?,
        background: p.background()?,
        radius: p.px("border-radius")?.unwrap_or(0.0),
        opacity: p.number("opacity")?.unwrap_or(1.0),
        skew: p.skew()?,
        ..StyleDef::default()
    };
    style.states = StateStyles {
        hover: state_delta(p, "state-hover-")?,
        focus: state_delta(p, "state-focus-")?,
        pressed: state_delta(p, "state-pressed-")?,
        disabled: state_delta(p, "state-disabled-")?,
    };
    if let Some(values) = p.take("border") {
        style.border = match values.as_slice() {
            [Value::Number(0.0)] => None,
            [Value::Px(w), Value::Ident(solid), Value::Color(hex)] if solid == "solid" => Border {
                width: *w,
                color: Color::parse(hex)?,
            }
            .into(),
            _ => return Err(p.bad("border", "`<n>px solid #colour`")),
        };
    }
    let text_align = match p.keyword("text-align")?.as_deref() {
        None | Some("left") => TextAlign::Start,
        Some("center") => TextAlign::Center,
        Some("right") => TextAlign::End,
        Some("inherit") => TextAlign::Start,
        Some(_) => return Err(p.bad("text-align", "left, center or right")),
    };
    let fit = p.fit()?;
    let text_overflow = p.text_overflow()?;
    Ok(Look {
        layout,
        style,
        font_size: p.px("font-size")?,
        color: text_color,
        text_align,
        fit,
        text_overflow,
    })
}
