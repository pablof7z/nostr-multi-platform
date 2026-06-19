//! Positive no_raw_tap fixture — #1552-deleted native push C-ABI sink.
//!
//! Contains the C-ABI register symbol from the deleted native push sink
//! (`nmp_app_register_event_sink`). The named-token check must catch this.
//!
//! NOTE: `ExternalEventSinkPolicy` is the retained in-process relay-forwarding
//! policy and is NOT the deleted native push sink. This fixture exercises the
//! *different*, deleted C-ABI-facing native push sink that required
//! retain-until-ack and created_at resync.

// A reintroduced native push sink register symbol — banned by no_raw_tap.
// External mirrors must use nmp_app_pull_page + GlobalLog cursor instead
// (ADR-0058, docs/architecture/external-consumers.md).
pub unsafe extern "C" fn nmp_app_register_event_sink(
    _app: *mut std::ffi::c_void,
    _callback: extern "C" fn(*mut std::ffi::c_void, *const u8, usize) -> bool,
    _ctx: *mut std::ffi::c_void,
) {
}
