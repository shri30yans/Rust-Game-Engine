//! Compiles an HTML + CSS subset into the engine's UI document (`*.ui.ron`).
//!
//! The page is a design surface: edit it in any editor or browser, run `ui-cook`, and the engine
//! hot-reloads the result. The output is the same [`engine_ui::Document`] a hand-written RON file
//! gives, and it is validated before it is written.

mod css;
mod error;
mod html;

pub use error::CookError;

/// Compiles `html` to document text.
///
/// # Errors
///
/// Returns a [`CookError`] for markup or style outside the subset, or an invalid document.
pub fn cook(html: &str) -> Result<String, CookError> {
    let (root, screen) = html::cook_root(html)?;
    let doc = engine_ui::Document::with_screen(root, screen)?;
    Ok(doc.to_ron()?)
}
