//! Error reporting for code with no caller to return to, such as the Android entry point.
//! On Android a message goes to logcat (`__android_log_write` in liblog, looked up at run time
//! like the other Android calls in `android::frame_rate`); elsewhere it goes to stderr. A message is never
//! dropped silently: if logcat cannot be reached, the message still goes to stderr.
//!
//! Invariants: never panics; NUL bytes in a message are written as `?`, so the C string is valid.

/// Writes `message` where the platform shows errors: logcat on Android, stderr elsewhere.
pub fn log_error(message: &str) {
    #[cfg(target_os = "android")]
    android::write_error(message);
    #[cfg(not(target_os = "android"))]
    eprintln!("{message}");
}

#[cfg(target_os = "android")]
#[allow(unsafe_code)] // the logcat FFI call; see the SAFETY comments inside
mod android {
    use std::ffi::{c_char, c_int, c_void};

    /// `ANDROID_LOG_ERROR` from `android/log.h`.
    const PRIORITY_ERROR: c_int = 6;
    /// `RTLD_NOW` from `dlfcn.h`.
    const RTLD_NOW: c_int = 2;

    unsafe extern "C" {
        fn dlopen(path: *const c_char, flags: c_int) -> *mut c_void;
        fn dlsym(handle: *mut c_void, name: *const c_char) -> *mut c_void;
    }

    /// `int __android_log_write(int prio, const char *tag, const char *text)`.
    type LogWrite = unsafe extern "C" fn(c_int, *const c_char, *const c_char) -> c_int;

    /// Sends `message` to logcat, or to stderr when liblog cannot be found.
    pub fn write_error(message: &str) {
        let mut text: Vec<u8> = message
            .bytes()
            .map(|b| if b == 0 { b'?' } else { b })
            .collect();
        text.push(0);
        // SAFETY: the name is a NUL-terminated literal that lives for the call; a null result is handled below.
        let lib = unsafe { dlopen(c"liblog.so".as_ptr(), RTLD_NOW) };
        if lib.is_null() {
            eprintln!("{message}");
            return;
        }
        // SAFETY: the name is a NUL-terminated literal; a null result is handled below.
        let symbol = unsafe { dlsym(lib, c"__android_log_write".as_ptr()) };
        if symbol.is_null() {
            eprintln!("{message}");
            return;
        }
        // SAFETY: `symbol` is non-null and is `__android_log_write`, whose signature is `LogWrite`.
        // The tag and the text are NUL-terminated and outlive the call. The returned byte count is
        // not needed: there is nowhere else to report a logging failure.
        let write: LogWrite = unsafe { std::mem::transmute(symbol) };
        unsafe { write(PRIORITY_ERROR, c"engine".as_ptr(), text.as_ptr().cast()) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logging_does_not_panic_on_host() {
        log_error("sandbox: test message");
        log_error("message with a NUL \0 byte and non-ASCII text: \u{e9}");
        log_error("");
    }
}
