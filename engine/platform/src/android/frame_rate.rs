//! Declares the game's intended present rate on the native window (Android 11+).
//!
//! Invariants: the call runs only after the swapchain surface is configured; minSdk 26
//! requires run-time symbol lookup for API 30+ entry points.

use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

use super::super::presentation::RefreshError;

/// Intended full-rate present for high-refresh panels (Hz).
pub const GAME_PRESENT_HZ: f32 = 120.0;
/// [`ANATIVEWINDOW_FRAME_RATE_COMPATIBILITY_DEFAULT`]: game content without fixed pull-down.
const FRAME_RATE_COMPATIBILITY_DEFAULT: i8 = 0;
/// [`ANATIVEWINDOW_CHANGE_FRAME_RATE_ALWAYS`]: do not require a seamless display mode switch.
const CHANGE_FRAME_RATE_ALWAYS: i8 = 1;

/// Reads the status that `ANativeWindow_setFrameRate` returns: zero is success.
#[cfg(any(target_os = "android", test))]
pub(super) fn frame_rate_status(status: i32) -> Result<(), RefreshError> {
    if status == 0 {
        Ok(())
    } else {
        Err(RefreshError::Rejected(status))
    }
}

/// Tells the compositor this window intends to present at [`GAME_PRESENT_HZ`].
#[allow(unsafe_code)] // NDK call; see SAFETY in `declare_present_rate`.
pub fn declare_present_rate(window: &Window) -> Result<(), RefreshError> {
    use std::ffi::{c_char, c_int, c_void};
    unsafe extern "C" {
        fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }
    let h = window.window_handle().map_err(RefreshError::NoHandle)?;
    let RawWindowHandle::AndroidNdk(a) = h.as_raw() else {
        return Err(RefreshError::NotAndroid);
    };
    let native = a.a_native_window.as_ptr().cast();
    // SAFETY: each symbol uses the NDK signatures in `android/native_window.h`. A null library or
    // symbol returns before the window pointer is used. `h` keeps the window alive for the call.
    unsafe {
        let lib = dlopen(c"libandroid.so".as_ptr(), 2);
        if lib.is_null() {
            return Err(RefreshError::LibraryMissing);
        }
        let with_strategy = dlsym(
            lib,
            c"ANativeWindow_setFrameRateWithChangeStrategy".as_ptr(),
        );
        if !with_strategy.is_null() {
            let f: extern "C" fn(*mut c_void, f32, i8, i8) -> i32 =
                std::mem::transmute(with_strategy);
            return frame_rate_status(f(
                native,
                GAME_PRESENT_HZ,
                FRAME_RATE_COMPATIBILITY_DEFAULT,
                CHANGE_FRAME_RATE_ALWAYS,
            ));
        }
        let basic = dlsym(lib, c"ANativeWindow_setFrameRate".as_ptr());
        if basic.is_null() {
            return Err(RefreshError::SymbolMissing);
        }
        let f: extern "C" fn(*mut c_void, f32, i8) -> i32 = std::mem::transmute(basic);
        frame_rate_status(f(native, GAME_PRESENT_HZ, FRAME_RATE_COMPATIBILITY_DEFAULT))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_zero_status_is_granted_and_any_other_status_is_kept_as_the_refusal() {
        assert!(frame_rate_status(0).is_ok());
        assert!(matches!(
            frame_rate_status(-22),
            Err(RefreshError::Rejected(-22))
        ));
    }
}
