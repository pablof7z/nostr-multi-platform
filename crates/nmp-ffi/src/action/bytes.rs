//! ADR-0064 / S4 (#1752) — the native **byte** doorway.
//!
//! The typed-FlatBuffers twin of the JSON `nmp_app_dispatch_action` doorway
//! (`action.rs`). It carries an open
//! [`nmp_core::dispatch_envelope::DispatchEnvelope`] (correlation_id + generated
//! `action_namespace` + schema_version + opaque per-crate payload) instead of
//! `(namespace, action_json)`, decodes it (S2), routes the opaque payload by
//! namespace into the registry's typed `start_bytes` / `execute_bytes` (S3),
//! and returns the SAME `{"correlation_id":…}` / `{"error":…}` shape.
//!
//! Lands ADDITIVELY beside the JSON doorway (ADR-0064 staged migration step 1):
//! the JSON path stays alive; it is retired later at Cut B / S9, not here.
//!
//! Split from `action.rs` purely as a size-management seam (AGENTS.md / V-12 —
//! keep each hand-authored file under the 500-LOC ceiling); the shared post-mint
//! outcome handling (`finish_dispatch`) and the small JSON helpers stay in
//! `action.rs` and are reached here through `super::`.

use std::ffi::{CString, c_char};

use super::super::{NmpApp, app_ref};
use super::{error_json, finish_dispatch, rejection_json};
use nmp_core::dispatch_envelope::{
    DispatchDecodeError, MAX_DISPATCH_ENVELOPE_BYTES, decode_dispatch_envelope,
};
use nmp_core::substrate::ActionContext;

/// ADR-0064 / S4 (#1752) — dispatch a typed action through the **byte**
/// doorway.
///
/// This is the typed-FlatBuffers twin of [`super::nmp_app_dispatch_action`]:
/// instead of `(namespace, action_json)`, the caller passes the bytes of an
/// open [`nmp_core::dispatch_envelope::DispatchEnvelope`] (correlation_id +
/// generated `action_namespace` + schema_version + opaque per-crate payload).
/// The generated host builders (`client.publishNote(...)`, Swift/Kotlin
/// equivalents) stamp the namespace + payload into that envelope so the host
/// never hand-assembles FlatBuffers or spells a namespace string.
///
/// Lands ADDITIVELY beside the JSON doorway (ADR-0064 staged migration step 1):
/// the JSON `nmp_app_dispatch_action` stays alive; it is retired later at Cut B
/// / S9, not here.
///
/// Returns a freshly heap-allocated, NUL-terminated JSON C string the caller
/// MUST release via [`crate::free::nmp_free_string`]:
///
/// * `{"correlation_id":"<id>"}` — the action was accepted and enqueued with
///   the actor for execution. The `correlation_id` is the HOST-SUPPLIED
///   envelope id, echoed back verbatim: on the byte lane it is the operation
///   identity end-to-end (ADR-0064 §4), so the host's spinner keyed on the id
///   it stamped into the envelope matches the terminal verdict.
/// * `{"error":"<message>"}` — the action was rejected. Fail-closed (D6): a
///   null `app`, a null `ptr`, an oversize / malformed / wrong-identifier /
///   wrong-schema-version / namespace-less / correlation-id-less envelope, an
///   unknown namespace, or a not-typed-capable module all come back here. An
///   oversize `len` is rejected at the ABI BEFORE a slice is even formed. Never
///   NULL for a non-null `app`, never a panic across the ABI.
///
/// # Safety
/// `app` must be a valid non-null pointer from [`crate::nmp_app_new`], or null
/// (a null `app` yields error JSON, never a crash). `ptr`/`len` must describe a
/// valid readable byte range (the finished envelope bytes), or `ptr` may be
/// null with `len` `0` (treated as an empty buffer and rejected). The bytes are
/// read but never retained or freed by this call.
#[allow(clippy::not_unsafe_ptr_arg_deref)]
#[no_mangle]
pub extern "C" fn nmp_app_dispatch_action_bytes(
    app: *mut NmpApp,
    ptr: *const u8,
    len: usize,
) -> *mut c_char {
    // Fail-closed BEFORE forming a slice: a hostile `len` must never construct a
    // `&[u8]` of that span. The S2 decoder bounds the same `MAX_…` ceiling, but
    // gating it here at the raw ptr/len means an oversize length can never drive
    // even a slice creation across the ABI. The reject reuses the S2
    // `DispatchDecodeError::Oversize` Display so the `{"error":…}` data shape is
    // byte-identical to the decoder's own oversize reject.
    if len > MAX_DISPATCH_ENVELOPE_BYTES {
        let result = error_json(
            &DispatchDecodeError::Oversize {
                len,
                max: MAX_DISPATCH_ENVELOPE_BYTES,
            }
            .to_string(),
        );
        return CString::new(result)
            .unwrap_or_else(|_| c"{}".to_owned())
            .into_raw();
    }
    // A null `ptr` (or zero `len`) is an empty buffer — the S2 decoder rejects
    // it fail-closed (BadFileIdentifier), so we never dereference a null.
    let bytes: &[u8] = if ptr.is_null() || len == 0 {
        &[]
    } else {
        // SAFETY: the caller's safety contract guarantees `ptr`/`len` describe
        // a valid readable byte range for the duration of this call. `len` is
        // bounded by `MAX_DISPATCH_ENVELOPE_BYTES` (gated above).
        unsafe { std::slice::from_raw_parts(ptr, len) }
    };
    let result = dispatch_action_bytes(app_ref(app), bytes);
    // JSON never contains an interior NUL; the `c"{}"` literal fallback is
    // NUL-checked at compile time, so there is no runtime panic path (D6).
    CString::new(result)
        .unwrap_or_else(|_| c"{}".to_owned())
        .into_raw()
}

/// Pure (FFI-free) core of [`nmp_app_dispatch_action_bytes`]: decode the open
/// [`nmp_core::dispatch_envelope::DispatchEnvelope`] (S2), route the opaque
/// per-crate payload by `action_namespace` into the registry's typed
/// `start_bytes` / `execute_bytes` doorway (S3), and return the same
/// `{"correlation_id":…}` / `{"error":…}` JSON shape as the JSON twin. Split
/// out so the unit tests can exercise the dispatch logic without raw pointers.
///
/// Fail-closed (D6): a null app, an oversize / malformed / wrong-identifier /
/// wrong-version / namespace-less envelope, or an unknown namespace all come
/// back as a populated `{"error":…}` — never a panic across the ABI. The S2
/// decoder and the S3 registry both `catch_unwind` internally; this function
/// adds no new unwind path.
pub(in crate::action) fn dispatch_action_bytes(app: Option<&NmpApp>, bytes: &[u8]) -> String {
    let Some(app) = app else {
        return error_json("null app");
    };
    // S2 — decode the open envelope and run its fail-closed gates. The
    // transport never peeks the opaque payload (S3 owns the typed decode).
    let decoded = match decode_dispatch_envelope(bytes) {
        Ok(decoded) => decoded,
        Err(err) => return error_json(&err.to_string()),
    };
    // Non-authoritative validation metadata: on the byte lane `start_bytes`
    // mints an id only after validation, but this doorway discards it and uses
    // the host-supplied `decoded.correlation_id` as the operation identity.
    // This stamp never feeds reducer state, event `created_at`, diagnostics, or
    // snapshot metadata; it is retained only because `ActionRegistry::start_*`
    // still returns an id for the legacy JSON twin.
    let dispatch_now_ms = {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    };
    let mut ctx = ActionContext::with_event_store_slot(app.event_store_handle());
    // S3 — route the opaque payload by namespace into the typed registry
    // doorway. `start_bytes` runs the per-crate typed decode + the fail-closed
    // `schema_version` gate BEFORE `start()`; an unknown namespace, a
    // not-typed-capable module, or a decode/version trip all surface as
    // `ActionRejection::Invalid` (the module never ran). Here `start_bytes` is
    // the VALIDATION gate; its `Ok` means "validation passed". We DISCARD the
    // id it mints — on the byte lane the operation identity is the
    // HOST-SUPPLIED `decoded.correlation_id`, threaded end-to-end per ADR-0064
    // §4 (the host's spinner is keyed on the id it stamped into the envelope, so
    // substituting a kernel-minted id here would strand it — the same
    // identity-substitution class the #1748 event-id fix closed). S2 already
    // proved the envelope carries a non-empty `correlation_id` (it rejects
    // `MissingCorrelationId`), so this id is always present and routable.
    let correlation_id = decoded.correlation_id;
    match app.action_registry.start_bytes(
        &mut ctx,
        dispatch_now_ms,
        &decoded.action_namespace,
        &decoded.payload,
    ) {
        Ok(_validated) => {
            let outcome = app.action_registry.execute_bytes(
                &ctx,
                &decoded.action_namespace,
                &decoded.payload,
                &correlation_id,
                &|cmd| app.send_cmd(cmd),
            );
            finish_dispatch(app, &correlation_id, outcome)
        }
        Err(rejection) => rejection_json(rejection),
    }
}

#[cfg(test)]
#[path = "tests_bytes.rs"]
mod tests_bytes;
