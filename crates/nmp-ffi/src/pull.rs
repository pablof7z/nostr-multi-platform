//! ADR-0058 §3 (step 3b) — synchronous, read-only FFI pull-page surface.
//!
//! [`nmp_mirror_pull_page`] is the C-ABI door a host (a nostrdb mirror, or a
//! feed's "give me more" cursor) uses to drain the kernel's raw ingest log. It
//! is a **read-only** call — it never dispatches an actor command, never mutates
//! the registry, never invokes a callback — so it does not cross the
//! fire-and-forget actor seam. ADR-0039 stays intact: this reads the raw event
//! log, NOT a kernel-derived projection (no `nmp_app_get_snapshot`, no
//! projection-pull accessor).
//!
//! ## Implementation note (M14-C7)
//!
//! All encoding logic and the pull algorithm live in
//! [`nmp_native_runtime::app_mirror`]. This file delegates to
//! [`NmpApp::mirror_pull_page_raw_bytes`] and wraps the returned `Vec<u8>` in
//! an [`NmpMirrorBytes`] owned by the host. The re-exports below keep the
//! `pull_tests` suite (`super::encode_gap`, `super::variant::GAP`, etc.)
//! working without modification.
//!
//! ## Result wire format (FFI-local, little-endian)
//!
//! The page/gap/error result is binary (it carries raw event JSON, which may
//! contain any byte), returned as owned [`NmpMirrorBytes`] the host frees with
//! [`nmp_mirror_free_bytes`]:
//!
//! ```text
//! result      := u8 variant
//!   variant 0 = Page  : u64 next_after_seq | u64 latest_seq | u8 has_more
//!                       | u32 entry_count | entry_count × entry
//!   variant 1 = Gap   : u64 requested_after_seq | u64 first_available_seq
//!   variant 2 = Error : u32 error_code  (see `error` consts below)
//! entry       := u64 seq | u8 op_tag (0=Inserted,1=Replaced,2=Deleted)
//!               | [Replaced] lp(replaced_id)
//!               | [Deleted]  lp(target_id) | u8 reason (0=Nip09,1=Nip40Expiry,2=AdminPurge)
//!               | lp(event_id) | u8 has_raw | [has_raw] lp(raw_json)
//!               | lp(source_relay)  (len 0 ⇒ absent) | u64 received_at_ms
//! lp(x)       := u32 byte_len | bytes        (length-prefixed UTF-8)
//! ```

use crate::NmpApp;

// Re-export encoding helpers, constants, and modules from the single
// implementation in nmp-native-runtime.  The pull_tests suite uses these
// via `super::` paths and does not need to change.
pub use nmp_native_runtime::app_mirror::{
    encode_gap, encode_page, error, variant, MAX_PULL_PAGE_ENTRIES, MAX_PULL_PAGE_RAW_BYTES,
};

/// Owned heap buffer handed across the C-ABI for a mirror pull-page result.
///
/// The page/gap result is binary FlatBuffers-adjacent data that may contain NUL
/// bytes, so it cannot be a C string. The host MUST return this to Rust via
/// [`nmp_mirror_free_bytes`] exactly once; the buffer belongs to the Rust
/// allocator.
#[repr(C)]
pub struct NmpMirrorBytes {
    /// Pointer to `len` bytes (null only for the empty buffer).
    pub ptr: *mut u8,
    /// Number of valid bytes.
    pub len: usize,
    /// Allocation capacity (needed to reconstruct the `Vec` on free).
    pub cap: usize,
}

impl NmpMirrorBytes {
    pub(super) fn from_vec(mut v: Vec<u8>) -> Self {
        let ptr = v.as_mut_ptr();
        let len = v.len();
        let cap = v.capacity();
        std::mem::forget(v);
        Self { ptr, len, cap }
    }

    pub(super) fn error(code: u32) -> Self {
        Self::from_vec(nmp_native_runtime::app_mirror::error_bytes(code))
    }
}

/// Synchronously drain one page of the kernel ingest log for a registered cursor.
///
/// Returns serialized [`NmpMirrorBytes`] (Page / Gap / Error — see the module
/// wire format). `max_entries` is clamped to `[1, MAX_PULL_PAGE_ENTRIES]` and
/// further bounded by the cursor's registered `limits.max_entries`; cumulative
/// raw bytes are bounded by `min(max_total_raw_bytes, MAX_PULL_PAGE_RAW_BYTES)`
/// (with at least one entry always delivered so the cursor makes progress).
///
/// Null app / unknown cursor / unavailable store all return a serialized
/// `Error` — never a panic or null deref (D6).
#[no_mangle]
pub extern "C" fn nmp_mirror_pull_page(
    app: *const NmpApp,
    cursor_id: u64,
    max_entries: u32,
    max_total_raw_bytes: u32,
) -> NmpMirrorBytes {
    // A panic must NEVER unwind across the C-ABI (UB). Catch it and return a
    // serialized `Error::PANIC` instead.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pull_page_impl(app, cursor_id, max_entries, max_total_raw_bytes)
    }))
    .unwrap_or_else(|_| NmpMirrorBytes::error(error::PANIC))
}

fn pull_page_impl(
    app: *const NmpApp,
    cursor_id: u64,
    max_entries: u32,
    max_total_raw_bytes: u32,
) -> NmpMirrorBytes {
    if app.is_null() {
        return NmpMirrorBytes::error(error::NULL_APP);
    }
    // SAFETY: `app` is non-null and, by C-ABI contract, a valid `NmpApp`
    // produced by `nmp_app_new`. This is a read-only borrow.
    let app = unsafe { &*app };
    let bytes =
        app.mirror_pull_page_raw_bytes(cursor_id, max_entries, max_total_raw_bytes as usize);
    NmpMirrorBytes::from_vec(bytes)
}

/// Release a buffer returned by [`nmp_mirror_pull_page`].
///
/// MUST be called exactly once for every returned [`NmpMirrorBytes`]. A null
/// pointer (the empty buffer) is a no-op (D6).
#[no_mangle]
pub extern "C" fn nmp_mirror_free_bytes(bytes: NmpMirrorBytes) {
    if bytes.ptr.is_null() {
        return;
    }
    // SAFETY: `bytes` was produced by `NmpMirrorBytes::from_vec` (ptr/len/cap
    // of a leaked `Vec<u8>`) and is freed exactly once per the ownership
    // contract.
    unsafe {
        drop(Vec::from_raw_parts(bytes.ptr, bytes.len, bytes.cap));
    }
}

#[cfg(test)]
#[path = "pull_tests.rs"]
mod pull_tests;
