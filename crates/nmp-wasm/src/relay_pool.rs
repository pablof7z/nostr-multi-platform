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
/// 1. Routes each outbound to every driver whose URL matches the kernel's
///    resolved target. A "both,indexer" entry spawns two drivers (one
///    Content, one Indexer) sharing a single URL — both must receive the
///    frame so each lane's `RelayHealth` diagnostics observe the same
///    OK/EOSE/NOTICE replies. A miss drops the frame (the relay is not in
///    our bootstrap — fabricating a socket would violate the host's
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
        // Fan outbound back to the right driver(s). Empty outbound still
        // means the kernel was driven (an inbound text frame ingested, even
        // if it produced no reply) — so we still push the snapshot below.
        //
        // V-01 Stage 3c — a single bootstrap URL can map to multiple
        // drivers when the role string is `"both,indexer"` (one Content
        // lane + one Indexer lane share the same WebSocket target). The
        // outbound is fanned to ALL matching drivers, not just the first,
        // so each lane sees the frame and the kernel's per-lane
        // `RelayHealth` counters stay accurate.
        {
            let drivers = drivers.borrow();
            for message in outbound {
                for driver in drivers.iter().filter(|d| d.url() == message.relay_url()) {
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

/// Map a [`RelayBootstrapEntry::role`] string to the diagnostic lanes the
/// driver pool should open for that URL.
///
/// V-01 Stage 3c — parses the same `RelayRole`-bearing role grammar the
/// native bootstrap uses (see `nmp-core/src/relay.rs` and
/// `nmp-chirp-config`):
///
/// | role string                | lanes spawned                  |
/// |----------------------------|--------------------------------|
/// | `"content"`                | `[Content]`                    |
/// | `"indexer"`                | `[Indexer]`                    |
/// | `"both"` / `"both,indexer"`| `[Content, Indexer]`           |
/// | anything else (incl. `""`) | `[Content]` (safe fallback)    |
///
/// Case-insensitive; surrounding whitespace is trimmed. Returns a
/// `&'static` slice so the caller does not allocate per-entry — the four
/// outcomes cover the entire `RelayRole::all()` bootstrap surface
/// (`Wallet` spawns on demand, not at startup).
///
/// Substrate-grade (D0): the helper carries no app/protocol nouns and
/// rejects nothing — unrecognized strings fall back to `Content` so a
/// future role token does not silently drop a relay from the pool.
fn roles_for_entry(role_str: &str) -> &'static [RelayRole] {
    const CONTENT_ONLY: &[RelayRole] = &[RelayRole::Content];
    const INDEXER_ONLY: &[RelayRole] = &[RelayRole::Indexer];
    const BOTH_LANES: &[RelayRole] = &[RelayRole::Content, RelayRole::Indexer];

    match role_str.trim().to_ascii_lowercase().as_str() {
        "indexer" => INDEXER_ONLY,
        "both" | "both,indexer" => BOTH_LANES,
        // "content" and every unrecognized value — safe fallback so a
        // typo or future-protocol role token never drops the relay.
        _ => CONTENT_ONLY,
    }
}

/// Instantiate one [`BrowserRelayDriver`] per (URL, role) pair derived from
/// the bootstrap entries, wiring each driver's outbound through `sink`.
/// Returns the populated driver list ready to move into the runtime's
/// relay slot.
///
/// V-01 Stage 3c — role assignment now honors the role string instead of
/// hardcoding [`RelayRole::Content`] for every entry. A `"both,indexer"`
/// URL spawns two drivers (one Content, one Indexer) sharing the same
/// `relay_url`, so kernel-side `RelayHealth` rows for the Indexer lane
/// observe their own connect/EOSE/NOTICE counters instead of being
/// mis-bucketed under Content. Wire-path correctness was never at stake —
/// the kernel routes outbound by URL (T105) — only the per-lane
/// diagnostics surface.
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
    // Reserve room for the worst case (every entry expands to two lanes)
    // so the per-entry inner loop never reallocates mid-spawn.
    let mut drivers = Vec::with_capacity(bootstrap.len() * 2);
    for entry in bootstrap {
        for &role in roles_for_entry(&entry.role) {
            let driver = BrowserRelayDriver::new(
                entry.url.clone(),
                role,
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
