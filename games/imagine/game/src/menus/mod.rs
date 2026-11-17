//! IMAGINE menu documents, runtime and typed action mapping.
//!
//! Documents enter the stack only after validation. Platform input maps to game actions through
//! the engine-owned menu runtime. Public paths remain available through `imagine::menus`.

mod actions;
mod bindings;
mod documents;
mod draw;
mod input;
mod runtime;

#[cfg(test)]
mod tests;

pub use actions::{MenuAction, SettingControl};
pub use bindings::{list, record};
pub use runtime::GameMenus;
