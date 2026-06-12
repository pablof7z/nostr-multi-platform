//! Canonical string-free symbol for the NMP FFI surface.
//!
//! Every `*mut c_char` returned by any NMP FFI function (`nmp_app_*`,
//! `nmp_broker_*`, etc.) is heap-allocated via `CString::into_raw`.  This
//! module exports the **single** symbol that owns the matching free path:
//!
//! ```c
//! void nmp_free_string(char *ptr);
//! ```
//!
//! Hosts MUST call `nmp_free_string` for **every** C string that an NMP FFI
//! function returns, regardless of which function produced it.  Mixing
//! `nmp_free_string` with the host's own `free(3)` is unsafe because the Rust
//! allocator may differ from the system allocator; cross-freeing is similarly
//! undefined behaviour.
//!
//! Passing `NULL` is a no-op (D6).

use std::ffi::{c_char, CString};

/// Release a heap-allocated C string returned by any NMP FFI function.
///
/// This is the **only** correct way to free a `*mut c_char` handed back by
/// the NMP FFI surface.  Every such pointer is produced by `CString::into_raw`
/// inside the Rust side; the memory therefore belongs to the Rust allocator and
/// MUST be returned to it through this symbol — not via the host's `free(3)`.
///
/// Ownership contract:
/// * The pointer MUST have been returned by an NMP FFI function.
/// * The pointer MUST be freed exactly once.
/// * After the call the pointer is dangling — do not read, write, or pass it
///   anywhere.
///
/// Passing `NULL` is always safe: this function returns immediately without
/// dereferencing the pointer (D6).
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_free_string(ptr: *mut c_char) {
    if ptr.is_null() {
        return;
    }
    // SAFETY: `ptr` is guaranteed by contract to have come from a
    // `CString::into_raw` call inside this crate and to be freed exactly once.
    unsafe {
        drop(CString::from_raw(ptr));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CString;

    /// `nmp_free_string(NULL)` must be a no-op (D6 null-safety).
    #[test]
    fn null_is_no_op() {
        nmp_free_string(std::ptr::null_mut());
    }

    /// A round-trip: allocate via `CString::into_raw`, free via
    /// `nmp_free_string`.  Under Miri / AddressSanitizer this catches a
    /// double-free or use-after-free if the implementation is wrong.
    #[test]
    fn round_trip_cstring() {
        let ptr = CString::new("hello nmp").unwrap().into_raw();
        nmp_free_string(ptr);
    }
}
