//! The arena's touch control identifiers. Their labels and geometry live exclusively in the HUD
//! document (`games/imagine/ui/hud.html`); the game reads only installed drawn regions.
//!
//! Invariant: each identifier matches one `data-id` in the cooked HUD document.

use platform::TouchRegionId;

/// The big right fire button, held down to fire; dragging it also aims.
pub const FIRE: TouchRegionId = TouchRegionId(0);
/// The jump button, pressed once per jump.
pub const JUMP: TouchRegionId = TouchRegionId(1);
/// The reload button, pressed once per reload.
pub const RELOAD: TouchRegionId = TouchRegionId(2);
/// The scope button, pressed to toggle aiming down the sights.
pub const SCOPE: TouchRegionId = TouchRegionId(3);
/// The optional left fire button.
pub const FIRE_LEFT: TouchRegionId = TouchRegionId(4);
/// Opens the match menu from the compact gear control.
pub const GEAR: TouchRegionId = TouchRegionId(5);
/// Toggle crouch or slide on touch.
pub const CROUCH: TouchRegionId = TouchRegionId(14);
