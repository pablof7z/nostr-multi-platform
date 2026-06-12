//! C-ABI on-demand NIP-11 probe (ADR-0051 path 3 — the "add relay" preview
//! flow). Extracted from `builder.rs` so the builder stays under the file-size
//! gate's hard cap; this is the only on-demand (not-yet-in-pool) entry point.
//!
//! The always-on, in-pool fetch path is wired separately in `tiers.rs` via
//! `nmp-nip11`'s `RelayConnectedHook`; this module is purely the manual probe
//! a shell calls to preview a relay the user is *considering* adding.

/// Callback invoked with the result of [`nmp_app_probe_relay_info`].
///
/// `ctx` is the opaque pointer the caller passed to the probe. `doc_json` is a
/// nul-terminated UTF-8 JSON string (the serialised
/// [`nmp_nip11::RelayInfoDoc`]) on success, or `NULL` when the relay could not
/// be reached / served no document. The pointer is valid ONLY for the duration
/// of the callback — copy it out before returning.
pub type RelayInfoProbeCallback =
    extern "C" fn(ctx: *mut std::ffi::c_void, doc_json: *const std::ffi::c_char);

/// Probe an arbitrary relay's NIP-11 information document off the caller thread
/// (ADR-0051 path 3 — the "add relay" preview flow). This does NOT require the
/// relay to be in the pool.
///
/// Spawns a worker that performs the blocking HTTP `GET` (`Accept:
/// application/nostr+json`), parses the document, and invokes `callback` with
/// the JSON (or `NULL` on any failure). The call returns immediately; the
/// callback fires later on the worker thread.
///
/// # Safety
///
/// `url` must be a valid nul-terminated UTF-8 C string for the duration of this
/// call. `callback` must be a valid function pointer; `ctx` is passed back
/// verbatim and must remain valid until the callback fires (the caller owns its
/// lifetime). A NULL `url` or `callback` is a silent no-op.
#[no_mangle]
pub unsafe extern "C" fn nmp_app_probe_relay_info(
    url: *const std::ffi::c_char,
    ctx: *mut std::ffi::c_void,
    callback: Option<RelayInfoProbeCallback>,
) {
    let Some(callback) = callback else {
        return;
    };
    if url.is_null() {
        return;
    }
    // SAFETY: `url` is non-null and the caller guarantees it is a valid
    // nul-terminated C string for the duration of this call.
    let url = match unsafe { std::ffi::CStr::from_ptr(url) }.to_str() {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    // The opaque `ctx` pointer is carried to the worker as an address (a raw
    // pointer is not `Send`). The caller owns `ctx`'s lifetime per the safety
    // contract.
    let ctx_addr = ctx as usize;
    std::thread::spawn(move || {
        let result = nmp_nip11::probe_relay_info(&url);
        let ctx = ctx_addr as *mut std::ffi::c_void;
        match result.ok().and_then(|doc| doc.to_json()) {
            Some(json) => match std::ffi::CString::new(json) {
                Ok(c) => callback(ctx, c.as_ptr()),
                Err(_) => callback(ctx, std::ptr::null()),
            },
            None => callback(ctx, std::ptr::null()),
        }
    });
}
