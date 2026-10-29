//! C ABI surface — everything declared in include/linkkit.h.
//! Conventions (house standard):
//! - `*mut c_char` results are caller-owned; free with
//!   `lk_free_string`.
//! - Failures return null / an LK_ERR_* code + a thread-local
//!   last-error message.
//! - Nothing unwinds across the FFI boundary (catch_unwind
//!   everywhere; release profile also sets panic = "abort").
//! - Spec validation is pure and runs on every platform; the
//!   operations themselves are Windows-only and return
//!   LK_ERR_NOT_SUPPORTED elsewhere.

#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::cell::RefCell;
use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int};
use std::panic::catch_unwind;

use crate::{ERR_INTERNAL, ERR_INVALID_ARG, LK_OK};

thread_local! {
    static LAST_ERROR: RefCell<(i32, CString)> =
        RefCell::new((0, CString::new("").unwrap()));
}

pub fn set_error(code: i32, msg: &str) {
    let clean: String = msg.chars().filter(|&c| c != '\0').collect();
    LAST_ERROR.with(|e| {
        *e.borrow_mut() = (
            code,
            CString::new(clean).unwrap_or_else(|_| CString::new("error").unwrap()),
        );
    });
}

fn clear_error() {
    set_error(LK_OK, "");
}

fn out_json(result: crate::LkResult<String>) -> *mut c_char {
    match result {
        Ok(text) => match CString::new(text) {
            Ok(s) => {
                clear_error();
                s.into_raw()
            }
            Err(_) => {
                set_error(ERR_INTERNAL, "result string contained a NUL byte");
                std::ptr::null_mut()
            }
        },
        Err((code, msg)) => {
            set_error(code, &msg);
            std::ptr::null_mut()
        }
    }
}

fn out_code(result: crate::LkResult<()>) -> c_int {
    match result {
        Ok(()) => {
            clear_error();
            LK_OK as c_int
        }
        Err((code, msg)) => {
            set_error(code, &msg);
            code as c_int
        }
    }
}

fn cstr<'a>(ptr: *const c_char, what: &str) -> crate::LkResult<&'a str> {
    if ptr.is_null() {
        return Err((ERR_INVALID_ARG, format!("{what} must not be null")));
    }
    unsafe { CStr::from_ptr(ptr) }
        .to_str()
        .map_err(|_| (ERR_INVALID_ARG, format!("{what} must be valid UTF-8")))
}

fn guard_json<F: FnOnce() -> *mut c_char>(f: F) -> *mut c_char {
    match catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(p) => p,
        Err(_) => {
            set_error(ERR_INTERNAL, "internal panic caught at the FFI boundary");
            std::ptr::null_mut()
        }
    }
}

fn guard_code<F: FnOnce() -> c_int>(f: F) -> c_int {
    match catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(c) => c,
        Err(_) => {
            set_error(ERR_INTERNAL, "internal panic caught at the FFI boundary");
            ERR_INTERNAL as c_int
        }
    }
}

#[no_mangle]
pub extern "C" fn lk_version() -> *const c_char {
    static VERSION_C: &[u8] = b"0.1.0\0";
    VERSION_C.as_ptr() as *const c_char
}

#[no_mangle]
pub extern "C" fn lk_last_error() -> *const c_char {
    LAST_ERROR.with(|e| e.borrow().1.as_ptr())
}

#[no_mangle]
pub extern "C" fn lk_last_error_code() -> c_int {
    LAST_ERROR.with(|e| e.borrow().0) as c_int
}

/// # Safety (caller contract): `p` is null or a string this
/// library returned and has not been freed yet. Documented in
/// the header.
#[no_mangle]
pub extern "C" fn lk_free_string(p: *mut c_char) {
    if !p.is_null() {
        unsafe {
            drop(CString::from_raw(p));
        }
    }
}

#[no_mangle]
pub extern "C" fn lk_create(spec_json: *const c_char) -> c_int {
    guard_code(|| {
        let inner = cstr(spec_json, "spec_json").and_then(crate::create_link);
        out_code(inner)
    })
}

#[no_mangle]
pub extern "C" fn lk_read(spec_json: *const c_char) -> *mut c_char {
    guard_json(|| {
        let inner = cstr(spec_json, "spec_json").and_then(crate::read_link);
        out_json(inner)
    })
}

#[no_mangle]
pub extern "C" fn lk_set_app_id(spec_json: *const c_char) -> c_int {
    guard_code(|| {
        let inner = cstr(spec_json, "spec_json").and_then(crate::set_app_id);
        out_code(inner)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    fn cs(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    #[cfg(windows)]
    fn take(p: *mut c_char) -> String {
        assert!(!p.is_null(), "expected a result string, got null");
        let s = unsafe { CStr::from_ptr(p) }.to_str().unwrap().to_string();
        lk_free_string(p);
        s
    }

    #[cfg(not(windows))]
    const CREATE_SPEC: &str = concat!(
        "{\"path\":\"C:\\\\Tools\\\\MyApp.lnk\",",
        "\"target\":\"C:\\\\Tools\\\\myapp.exe\"}"
    );

    #[test]
    fn ffi_null_and_bad_spec() {
        assert_eq!(lk_create(std::ptr::null()), ERR_INVALID_ARG as c_int);
        assert_eq!(lk_last_error_code(), ERR_INVALID_ARG as c_int);
        assert!(lk_read(std::ptr::null()).is_null());
        assert_eq!(lk_last_error_code(), ERR_INVALID_ARG as c_int);
        assert_eq!(lk_set_app_id(std::ptr::null()), ERR_INVALID_ARG as c_int);
        assert_eq!(lk_last_error_code(), ERR_INVALID_ARG as c_int);
        // Bad specs are argument errors on every platform.
        assert_eq!(
            lk_create(cs("{\"path\":\"x\"}").as_ptr()),
            ERR_INVALID_ARG as c_int
        );
        assert!(lk_read(cs("not json").as_ptr()).is_null());
        assert_eq!(lk_last_error_code(), ERR_INVALID_ARG as c_int);
        assert_eq!(
            lk_set_app_id(cs("{\"path\":\"C:\\\\a.lnk\"}").as_ptr()),
            ERR_INVALID_ARG as c_int
        );
        lk_free_string(std::ptr::null_mut()); // must be a no-op
    }

    #[cfg(not(windows))]
    #[test]
    fn ffi_platform_contract_off_windows() {
        // Valid specs validate fine, then hit the documented
        // NOT_SUPPORTED — never a fake success.
        assert_eq!(
            lk_create(cs(CREATE_SPEC).as_ptr()),
            crate::ERR_NOT_SUPPORTED as c_int
        );
        assert_eq!(lk_last_error_code(), crate::ERR_NOT_SUPPORTED as c_int);
        assert!(lk_read(cs("{\"path\":\"C:\\\\Tools\\\\MyApp.lnk\"}").as_ptr()).is_null());
        assert_eq!(lk_last_error_code(), crate::ERR_NOT_SUPPORTED as c_int);
        assert_eq!(
            lk_set_app_id(
                cs("{\"path\":\"C:\\\\Tools\\\\MyApp.lnk\",\"app_id\":\"My.App\"}").as_ptr()
            ),
            crate::ERR_NOT_SUPPORTED as c_int
        );
    }

    /// The Windows integration test: create a real .lnk in a
    /// temp dir through the FFI, read it back, stamp a new
    /// AppUserModelID, read again. This is the one test that
    /// exercises the COM layer for real; it only runs on
    /// Windows (CI), and the runtime behaviour it proves is
    /// still only Partially Verified until a field test opens
    /// the shortcut (see QUALITY.md).
    #[cfg(windows)]
    #[test]
    fn ffi_create_read_stamp_round_trip_on_windows() {
        let dir = std::env::temp_dir().join(format!("linkkit-it-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let link_path = dir.join("MyApp.lnk");
        let link_path = link_path.to_str().unwrap().replace('\\', "\\\\");
        let target = "C:\\\\Tools\\\\myapp.exe";
        // Create without an app_id first: the read-back must
        // report app_id as null, not an empty string.
        let spec = format!(
            "{{\"path\":\"{link_path}\",\"target\":\"{target}\",\
             \"args\":\"--flag value\",\"working_dir\":\"C:\\\\Tools\",\
             \"icon\":\"C:\\\\Tools\\\\myapp.exe\",\"icon_index\":2,\
             \"description\":\"My App\"}}"
        );
        assert_eq!(
            lk_create(cs(&spec).as_ptr()),
            LK_OK as c_int,
            "create: {}",
            unsafe { CStr::from_ptr(lk_last_error()).to_str().unwrap() }
        );
        assert!(std::path::Path::new(&link_path.replace("\\\\", "\\")).exists());

        let read_spec = format!("{{\"path\":\"{link_path}\"}}");
        let out = take(lk_read(cs(&read_spec).as_ptr()));
        let v = crate::json::parse(&out).unwrap();
        assert_eq!(
            v.get("target").unwrap().as_str(),
            Some("C:\\Tools\\myapp.exe")
        );
        assert_eq!(v.get("args").unwrap().as_str(), Some("--flag value"));
        assert_eq!(v.get("working_dir").unwrap().as_str(), Some("C:\\Tools"));
        assert_eq!(
            v.get("icon").unwrap().as_str(),
            Some("C:\\Tools\\myapp.exe")
        );
        assert_eq!(v.get("icon_index").unwrap().as_i64(), Some(2));
        assert_eq!(v.get("description").unwrap().as_str(), Some("My App"));
        assert_eq!(v.get("app_id").unwrap(), &crate::json::Value::Null);

        // Stamp an AppUserModelID onto the existing file and
        // read it back.
        let stamp = format!("{{\"path\":\"{link_path}\",\"app_id\":\"LinkKit.IntegrationTest\"}}");
        assert_eq!(lk_set_app_id(cs(&stamp).as_ptr()), LK_OK as c_int);
        let out = take(lk_read(cs(&read_spec).as_ptr()));
        let v = crate::json::parse(&out).unwrap();
        assert_eq!(
            v.get("app_id").unwrap().as_str(),
            Some("LinkKit.IntegrationTest")
        );
        // The stamp must not have disturbed the other fields.
        assert_eq!(
            v.get("target").unwrap().as_str(),
            Some("C:\\Tools\\myapp.exe")
        );
        assert_eq!(v.get("description").unwrap().as_str(), Some("My App"));

        // Reading a missing file is NOT_FOUND, not a platform
        // mystery.
        let missing = format!(
            "{{\"path\":\"{}\"}}",
            link_path.replace("MyApp.lnk", "Nope.lnk")
        );
        assert!(lk_read(cs(&missing).as_ptr()).is_null());
        assert_eq!(lk_last_error_code(), crate::ERR_NOT_FOUND as c_int);

        let _ = std::fs::remove_dir_all(&dir);
    }
}
