//! V-01 Stage 3 / 3b — relay-pool helpers for the wasm32 runtime.
//!
//! Owns the construction of the per-relay [`BrowserRelayDriver`] set, the
//! sink closure that fans the kernel's outbound back to the right driver,
//! and (Stage 3b) the snapshot-callback push that fires after every
//! kernel-mutating inbound relay frame.
//!
//! Split out of `runtime.rs` so neither file exceeds the LOC ceiling and so
//! the wasm32-only logic does not pollute the protocol-conformance paths
//! that run on native CI.

use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{KernelReducer, OutboundMessage, RelayRole};

use crate::protocol::RelayBootstrapEntry;
use crate::relay_driver::{BrowserRelayDriver, RelaySink};
use crate::runtime::WasmRuntimeError;
use crate::snapshot::{push_snapshot_if_callback, RuntimeMeta};

/// Build the shared outbound sink — the closure each
/// [`BrowserRelayDriver`] hands to the kernel via
/// [`nmp_core::KernelReducer::handle_relay_frame`]. The sink:
///
/// 1. Routes each outbound to the driver whose URL matches the kernel's
///    resolved target. A miss drops the frame (the relay is not in our
///    bootstrap — fabricating a socket would violate the host's
///    relay-policy declaration).
/// 2. **Stage 3b**: pushes a fresh snapshot to the JS host through the
///    registered callback (if any). The push fires unconditionally after
///    every inbound — the relay-frame ingest path does not return a
///    `KernelUpdate`, so we cannot gate on "produced an update" without
///    re-snapshotting and diffing, which is more expensive than just
///    pushing. The host's reducer is idempotent on identical envelopes.
///
/// Substrate-grade (D0): the sink receives only protocol-neutral
/// [`OutboundMessage`]s; the callback push delivers the same JSON envelope
/// `handle()` returns synchronously.
#[must_use]
pub(crate) fn build_sink(
    drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    reducer: Rc<RefCell<KernelReducer>>,
    meta: Rc<RefCell<RuntimeMeta>>,
) -> RelaySink {
    Rc::new(move |outbound: Vec<OutboundMessage>| {
        // Fan outbound back to the right driver. Empty outbound still means
        // the kernel was driven (an inbound text frame ingested, even if it
        // produced no reply) — so we still push the snapshot below.
        {
            let drivers = drivers.borrow();
            for message in outbound {
                if let Some(driver) = drivers.iter().find(|d| d.url() == message.relay_url()) {
                    let _ = driver.send_text(message.text());
                }
            }
        }

        // V-01 Stage 3b — async snapshot push. Every relay-frame injection
        // is a potential kernel-state mutation; the push helper short-
        // circuits if no callback is installed. Borrow order matters: we
        // dropped the `drivers` borrow at the end of the block above, so
        // `push_snapshot_if_callback` can re-enter the kernel through its
        // own `reducer.borrow()` without conflict.
        push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
    })
}

/// Instantiate one [`BrowserRelayDriver`] per bootstrap entry, wiring each
/// one's outbound through `sink`. Returns the populated driver list ready to
/// move into the runtime's relay slot.
///
/// Role assignment: every relay opens as [`RelayRole::Content`] — the
/// diagnostic-lane discriminator the kernel uses for `RelayHealth` rows.
/// Multi-role parsing (split `"both,indexer"` into two drivers) is a
/// post-Stage-3 follow-up tracked in BACKLOG §V-01 Stage 3b — kernel-side
/// routing is by URL (T105) so the wire path is correct; only the
/// `RelayHealth` lane diagnostics for pure-indexer URLs are mis-bucketed.
///
/// # Ordering invariant
///
/// The runtime calls [`build_sink`] BEFORE `spawn_drivers`, but the sink
/// captures `Rc<RefCell<Vec<…>>>` that is still empty at that point. The
/// drivers are then constructed in this loop, and only after the function
/// returns does the runtime assign `*self.relays.borrow_mut() = drivers`.
/// This is safe **because** `WebSocket::new()` returns synchronously and the
/// `onopen` JS closure cannot fire until control returns to the JS event
/// loop — which happens only after this whole function returns and the
/// runtime swaps the driver list in. By the time the first `onopen` fires,
/// the sink can find the driver via URL lookup. Any refactor that moves the
/// `onopen`-firing call site (e.g. a synchronous polyfill in tests) must
/// re-establish this invariant.
pub(crate) fn spawn_drivers(
    bootstrap: &[RelayBootstrapEntry],
    kernel: Rc<RefCell<KernelReducer>>,
    sink: RelaySink,
) -> Result<Vec<Rc<BrowserRelayDriver>>, WasmRuntimeError> {
    let mut drivers = Vec::with_capacity(bootstrap.len());
    for entry in bootstrap {
        let driver = BrowserRelayDriver::new(
            entry.url.clone(),
            RelayRole::Content,
            Rc::clone(&kernel),
            Rc::clone(&sink),
        )
        .map_err(|err| {
            WasmRuntimeError::InvalidConfig(format!(
                "failed to open WebSocket to {}: {err:?}",
                entry.url
            ))
        })?;
        drivers.push(driver);
    }
    Ok(drivers)
}

/// Close every driver in the pool and drop their parked closures. Idempotent.
pub(crate) fn close_drivers(drivers: &Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>) {
    for driver in drivers.borrow().iter() {
        driver.close();
    }
    drivers.borrow_mut().clear();
}
