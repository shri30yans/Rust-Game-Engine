//! Draws a UI document with the renderer's immediate-mode [`Ui`], and reloads it when its file
//! changes in a debug build.
//!
//! Invariants: a document that fails to reload keeps the last good one on screen and the error is
//! returned to the caller (show it with the [`crate::ErrorBanner`]); a failed layout replays the
//! retained last-good draw list; button regions come from the same layout pass as the picture.

use engine_core::TouchRegion;
use engine_ui::doc::TextAlign;
use engine_ui::{
    Bindings, Document, DrawList, MetricsKey, Paint, Primitive, Skew, TextMeasure, Transition,
    UiLayout,
};
use render::{Align, FontId, Ui};

use crate::error::AppError;

struct UiMeasure<'a> {
    ui: &'a mut Ui,
    font: FontId,
}

impl TextMeasure for UiMeasure<'_> {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(u64::from(self.font.0))
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        self.ui.measure(self.font, px, text)
    }
}

/// Text width for checks that do not have a renderer font.
struct NoFont;

impl TextMeasure for NoFont {
    fn metrics_key(&self) -> MetricsKey {
        MetricsKey(u64::MAX)
    }

    fn width(&mut self, px: f32, text: &str) -> f32 {
        px * 0.5 * text.chars().count() as f32
    }
}

fn rgba(color: Paint) -> [f32; 4] {
    [
        f32::from(color.rgb.r) / 255.0,
        f32::from(color.rgb.g) / 255.0,
        f32::from(color.rgb.b) / 255.0,
        color.alpha,
    ]
}

/// A UI document ready to draw and atomically reload from the mounted pack.
#[derive(Debug)]
pub struct UiScreen {
    layout: UiLayout,
    /// Touch regions from the last good draw list, retained without steady-frame allocation.
    regions: Vec<TouchRegion>,
}

impl UiScreen {
    /// Builds a screen from document text (an embedded default, or a test).
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Ui`] when the text is not a valid document.
    pub fn from_source(source: &str) -> Result<Self, AppError> {
        let doc = Document::from_ron(source)?;
        let layout = UiLayout::new(doc)?;
        Ok(Self {
            layout,
            regions: Vec::with_capacity(platform::MAX_TOUCH_REGIONS),
        })
    }

    /// Checks the current text bounds for a document and its bindings without a GPU.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Ui`] when bindings or layout are invalid, or a text run escapes its
    /// measured box or parent.
    pub fn check_text_overflow(
        &mut self,
        bindings: &Bindings,
        screen: (f32, f32),
    ) -> Result<(), AppError> {
        let draw_list = self.layout.frame(bindings, screen, &mut NoFont)?;
        engine_ui::check_text_overflow(draw_list).map_err(AppError::Ui)
    }

    /// Reloads from a changed pack entry and retains the prior layout on failure.
    ///
    /// # Errors
    ///
    /// Returns a parse or validation error; the previous document stays in use.
    pub fn reload_source(&mut self, source: &str) -> Result<(), AppError> {
        let doc = Document::from_ron(source)?;
        self.layout.reload(doc)?;
        Ok(())
    }

    /// Lays out the document for `screen` pixels and draws it. Returns the clipped touch areas
    /// produced by that exact real-font layout pass.
    ///
    /// # Errors
    ///
    /// Returns [`AppError::Ui`] when the document names a variable the bindings lack or have with
    /// the wrong type, or the layout engine rejects the tree. On error it replays the last
    /// successful draw list before returning the error.
    pub fn draw(
        &mut self,
        ui: &mut Ui,
        font: FontId,
        bindings: &Bindings,
        screen: (f32, f32),
    ) -> Result<&[TouchRegion], AppError> {
        let error = match self
            .layout
            .frame(bindings, screen, &mut UiMeasure { ui, font })
        {
            Ok(list) => {
                Self::draw_list(
                    ui,
                    font,
                    list,
                    Transition {
                        opacity: 1.0,
                        offset_x: 0.0,
                        scale: 1.0,
                    },
                );
                self.regions.clear();
                self.regions.extend_from_slice(list.touch_regions());
                return Ok(&self.regions);
            }
            Err(error) => error,
        };
        if let Some(last_good) = self.layout.draw_list_ref() {
            Self::draw_list(
                ui,
                font,
                last_good,
                Transition {
                    opacity: 1.0,
                    offset_x: 0.0,
                    scale: 1.0,
                },
            );
        }
        Err(error.into())
    }

    /// Draws a retained menu list with the transition transform supplied by `Menus`.
    pub fn draw_list(ui: &mut Ui, font: FontId, list: &DrawList, transition: Transition) {
        let mut skew: Option<Skew> = None;
        for prim in list.primitives() {
            // The renderer shears each shape about its own bottom edge; shifting by the gap to
            // the node's pivot makes a whole subtree lean about one common line.
            let lean = skew.map_or(0.0, |s| s.lean);
            ui.shear = lean;
            let shift = |bottom: f32| skew.map_or(0.0, |s| s.lean * (s.pivot_y - bottom));
            match prim {
                Primitive::Skew(next) => skew = *next,
                Primitive::Fill {
                    rect,
                    radius,
                    color,
                } => {
                    ui.rect(
                        rect.x + shift(rect.y + rect.h) + transition.offset_x,
                        rect.y,
                        rect.w * transition.scale,
                        rect.h * transition.scale,
                        *radius,
                        rgba(Paint {
                            alpha: color.alpha * transition.opacity,
                            ..*color
                        }),
                    );
                }
                Primitive::Outline {
                    rect,
                    radius,
                    width,
                    color,
                } => {
                    ui.stroke(
                        rect.x + shift(rect.y + rect.h) + transition.offset_x,
                        rect.y,
                        rect.w * transition.scale,
                        rect.h * transition.scale,
                        *radius,
                        *width,
                        rgba(Paint {
                            alpha: color.alpha * transition.opacity,
                            ..*color
                        }),
                    );
                }
                Primitive::Ring {
                    cx,
                    cy,
                    radius,
                    width,
                    color,
                } => {
                    ui.ring(
                        *cx + transition.offset_x,
                        *cy,
                        *radius * transition.scale,
                        *width,
                        1.0,
                        rgba(Paint {
                            alpha: color.alpha * transition.opacity,
                            ..*color
                        }),
                    );
                }
                Primitive::Disc {
                    cx,
                    cy,
                    radius,
                    color,
                } => {
                    ui.circle(
                        *cx + transition.offset_x,
                        *cy,
                        *radius * transition.scale,
                        rgba(Paint {
                            alpha: color.alpha * transition.opacity,
                            ..*color
                        }),
                    );
                }
                Primitive::LinearGradient {
                    rect,
                    radius,
                    angle_deg,
                    stops,
                    stop_count,
                } => {
                    let count = usize::from(*stop_count);
                    let packed: Vec<(f32, [f32; 4])> = stops
                        .iter()
                        .take(count)
                        .map(|stop| {
                            (
                                stop.pos,
                                [
                                    f32::from(stop.color.rgb.r) / 255.0,
                                    f32::from(stop.color.rgb.g) / 255.0,
                                    f32::from(stop.color.rgb.b) / 255.0,
                                    stop.color.alpha() * transition.opacity,
                                ],
                            )
                        })
                        .collect();
                    ui.set_linear_gradient_uniform(*angle_deg, &packed);
                    ui.linear_gradient(
                        rect.x + shift(rect.y + rect.h) + transition.offset_x,
                        rect.y,
                        rect.w * transition.scale,
                        rect.h * transition.scale,
                        *radius,
                        rgba(Paint {
                            alpha: transition.opacity,
                            rgb: engine_ui::Srgb8 {
                                r: 255,
                                g: 255,
                                b: 255,
                            },
                        }),
                    );
                }
                Primitive::Text {
                    rect,
                    px,
                    color,
                    align,
                    range,
                } => {
                    let Some(text) = list.text_of(range) else {
                        continue;
                    };
                    let rect = &engine_ui::Rect {
                        x: rect.x + shift(rect.y + *px) + transition.offset_x,
                        y: rect.y,
                        w: rect.w * transition.scale,
                        h: rect.h * transition.scale,
                    };
                    let (x, align) = match align {
                        TextAlign::Start => (rect.x, Align::Left),
                        TextAlign::Center => (rect.x + rect.w * 0.5, Align::Center),
                        TextAlign::End => (rect.x + rect.w, Align::Right),
                    };
                    ui.text(
                        font,
                        x,
                        rect.y,
                        *px * transition.scale,
                        rgba(Paint {
                            alpha: color.alpha * transition.opacity,
                            ..*color
                        }),
                        align,
                        text,
                    );
                }
            }
        }
        ui.shear = 0.0;
    }
}
