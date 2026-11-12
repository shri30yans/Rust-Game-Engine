//! When the swapchain is configured and ready to present, the platform declares how fast the game
//! intends to draw. Measured frame times stay separate: they report what the compositor
//! actually delivered.
//!
//! Invariant: one call per presentation surface session, immediately after `surface.configure`,
//! before the first frame of that session.

#[cfg(target_os = "android")]
use crate::log_error;
use winit::window::Window;

/// Why the display did not accept the declared present rate.
#[derive(Debug, thiserror::Error)]
pub enum RefreshError {
    /// Not an Android native window (desktop and headless).
    #[error("the window is not an Android window")]
    NotAndroid,
    /// The window has no native handle to pass to the system.
    #[error("the window has no native handle")]
    NoHandle(#[source] winit::raw_window_handle::HandleError),
    /// `libandroid.so` could not be opened.
    #[error("libandroid.so could not be opened")]
    LibraryMissing,
    /// `ANativeWindow_setFrameRate` is missing (Android before 11).
    #[error("ANativeWindow_setFrameRate is missing from libandroid.so (Android before 11)")]
    SymbolMissing,
    /// The system refused the request with this status code.
    #[error("the system refused the frame rate request with status {0}")]
    Rejected(i32),
}

/// Result of binding a configured swapchain to the OS presentation policy for this session.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PresentationBinding {
    /// Hz the engine asked the compositor to match, when a declaration was attempted.
    pub intended_present_hz: Option<f32>,
    /// Whether the OS accepted the declaration for this surface session.
    pub declared: bool,
}

impl PresentationBinding {
    /// Desktop and headless: no declaration; measured telemetry still reports actual cadence.
    #[must_use]
    pub const fn unmanaged() -> Self {
        Self {
            intended_present_hz: None,
            declared: false,
        }
    }
}

/// Declares present intent for this surface session. Called once after swapchain configure.
#[must_use]
pub fn bind_after_swapchain_configure(window: &Window) -> PresentationBinding {
    #[cfg(target_os = "android")]
    {
        use crate::android::frame_rate::{self, GAME_PRESENT_HZ};
        let intended = Some(GAME_PRESENT_HZ);
        let result = frame_rate::declare_present_rate(window);
        let binding = PresentationBinding {
            intended_present_hz: intended,
            declared: result.is_ok(),
        };
        log_binding(&binding, result.err());
        binding
    }
    #[cfg(not(target_os = "android"))]
    {
        let _ = window;
        PresentationBinding::unmanaged()
    }
}

/// Records whether the compositor accepted the declared rate (logcat on Android).
#[cfg(target_os = "android")]
fn log_binding(binding: &PresentationBinding, error: Option<RefreshError>) {
    let Some(hz) = binding.intended_present_hz else {
        return;
    };
    if binding.declared {
        log_error(&format!(
            "presentation: compositor accepted {hz:.0} Hz declare for this surface"
        ));
    } else if let Some(error) = error {
        log_error(&format!(
            "presentation: compositor did not accept {hz:.0} Hz declare: {error}"
        ));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unmanaged_binding_carries_no_intent() {
        let binding = PresentationBinding::unmanaged();
        assert_eq!(binding.intended_present_hz, None);
        assert!(!binding.declared);
    }
}
