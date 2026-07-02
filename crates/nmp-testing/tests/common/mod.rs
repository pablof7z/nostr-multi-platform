//! Shared helpers across `tests/*.rs` integration tests in `nmp-testing`.
//!
//! - `mock_bunker_relay` — NIP-46 mock-bunker relay (bunker:// direction).
//! - `mock_nostrconnect_signer` — NIP-46 mock signer-app (nostrconnect://
//!   direction; Phase 2).
//! - `broker_adapter` — test-only translation from app-neutral broker events
//!   into actor commands.
//! - `wire_log` — stderr FD-pipe capture for `NMP_CLAIM_LOG` structured JSON
//!   lines (W9 relay-search-radius acceptance tests).
//! - `stub_relay` — TCP stub relay that drops connections after a configurable
//!   delay (A5 mid-claim unreachable test).
//! - `recording_relay` — local WebSocket relay that records `REQ`/`CLOSE` and
//!   serves signed events to runtime E2E tests.
//! - `ref_commands` — helpers that decode test `nostr:` URI fixtures before
//!   sending the raw-key `RefsCommand::Resolve` / `Release` seam.
//!
//! cargo treats `tests/common/mod.rs` as a non-test source file even when
//! sibling files are integration tests.

#![allow(dead_code)]

use std::ffi::c_void;
use std::sync::Arc;

pub mod broker_adapter;
pub mod mock_bunker_relay;
pub mod mock_nostrconnect_signer;
pub mod recording_relay;
pub mod ref_commands;
pub mod stub_relay;
pub mod wire_log;

pub fn new_app_ptr() -> *mut nmp_native_runtime::NmpApp {
    Box::into_raw(Box::new(nmp_native_runtime::new_app()))
}

pub fn free_app_ptr(app: *mut nmp_native_runtime::NmpApp) {
    if app.is_null() {
        return;
    }
    unsafe {
        (&*app).stop_runtime();
        drop(Box::from_raw(app));
    }
}

pub fn start_app(app: *mut nmp_native_runtime::NmpApp, visible_limit: usize, emit_hz: u32) {
    if app.is_null() {
        return;
    }
    unsafe { (&*app).start_runtime(visible_limit, emit_hz) };
}

pub fn stop_app(app: *mut nmp_native_runtime::NmpApp) {
    if app.is_null() {
        return;
    }
    unsafe { (&*app).stop_runtime() };
}

pub fn set_c_update_listener(
    app: *mut nmp_native_runtime::NmpApp,
    ctx: *mut c_void,
    callback: Option<extern "C" fn(*mut c_void, *const u8, usize)>,
) {
    if app.is_null() {
        return;
    }
    let listener = callback.map(|callback| {
        let ctx_addr = ctx as usize;
        Arc::new(move |payload: &[u8]| {
            callback(ctx_addr as *mut c_void, payload.as_ptr(), payload.len());
        }) as nmp_native_runtime::UpdateListener
    });
    unsafe { (&*app).set_update_listener(listener) };
}
