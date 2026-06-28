//! C-ABI on-demand NIP-11 probe.
//!
//! This is the manual "add relay" preview path. It is intentionally C-ABI
//! glue: native Rust runtime code owns the app handle/lifecycle, while this
//! module owns the exported symbol, C string parsing, callback lifetime
//! contract, and worker-thread callback handoff.

/// Callback invoked with the result of [`nmp_app_probe_relay_info`].
///
/// `ctx` is the opaque pointer the caller passed to the probe. `doc_json` is a
/// nul-terminated UTF-8 JSON string on success, or `NULL` when the relay could
/// not be reached / served no document. The pointer is valid only for the
/// duration of the callback; callers must copy it before returning.
pub type RelayInfoProbeCallback =
    extern "C" fn(ctx: *mut std::ffi::c_void, doc_json: *const std::ffi::c_char);

/// Probe an arbitrary relay's NIP-11 information document off the caller thread.
///
/// This does not require the relay to be in the pool. The call returns
/// immediately; `callback` fires later on the worker thread.
///
/// # Safety
///
/// `url` must be a valid nul-terminated UTF-8 C string for the duration of this
/// call. `callback` must be a valid function pointer; `ctx` is passed back
/// verbatim and must remain valid until the callback fires. A NULL `url` or
/// `callback` is a no-op.
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

    // SAFETY: `url` is non-null and the caller promises a valid C string.
    let url = match unsafe { std::ffi::CStr::from_ptr(url) }.to_str() {
        Ok(value) => value.to_owned(),
        Err(_) => return,
    };

    let ctx_addr = ctx as usize;
    std::thread::spawn(move || {
        let result = nmp_nip11::probe_relay_info(&url);
        let ctx = ctx_addr as *mut std::ffi::c_void;
        match result.ok().and_then(|doc| doc.to_json()) {
            Some(json) => match std::ffi::CString::new(json) {
                Ok(c_json) => callback(ctx, c_json.as_ptr()),
                Err(_) => callback(ctx, std::ptr::null()),
            },
            None => callback(ctx, std::ptr::null()),
        }
    });
}
