//! V-01 Stage 3 / 3b — relay-pool helpers for the wasm32 runtime.
//!
//! Owns the construction of the per-relay [`BrowserRelayDriver`] set, the
//! kernel-handler callback bag that bridges the driver back into
//! [`nmp_core::KernelReducer`], the outbound fan-out, and the snapshot push
//! that fires after every kernel-mutating inbound relay frame.
//!
//! # Step 8 phase C — relocation seam
//!
//! Before phase C the driver lived in this crate alongside `relay_pool` and
//! held its kernel handle directly (`Rc<RefCell<KernelReducer>>`). Phase C
//! moved [`BrowserRelayDriver`] into [`nmp_network::browser_driver`] while
//! keeping the layering invariant intact (`nmp-network` cannot depend on
//! `nmp-core`). The driver now takes a [`BrowserKernelHandlers`] struct of
//! `Rc<dyn Fn>` callbacks; this module is the single construction site.
//! Each callback wraps the same kernel-ingest method the old driver called
//! directly, plus the outbound fan-out + snapshot push the old sink ran.
//!
//! Split out of `runtime.rs` so neither file exceeds the LOC ceiling and so
//! the wasm32-only logic does not pollute the protocol-conformance paths
//! that run on native CI.

use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{KernelReducer, OutboundMessage, RelayFrame, RelayRole};
use nmp_network::browser_driver::{BrowserKernelHandlers, BrowserRelayDriver};

use crate::protocol::RelayBootstrapEntry;
use crate::runtime::WasmRuntimeError;
use crate::snapshot::{push_snapshot_if_callback, RuntimeMeta};

/// Fan an outbound batch back to the driver(s) whose URL matches each
/// message. Used by every kernel-handler closure (connected/text/binary)
/// after the kernel returns a non-empty outbound queue.
///
/// A single bootstrap URL can map to multiple drivers when the role string
/// is `"both,indexer"` (one Content lane + one Indexer lane share the same
/// WebSocket target). The outbound is fanned to ALL matching drivers, not
/// just the first, so each lane sees the frame and the kernel's per-lane
/// `RelayHealth` counters stay accurate. A miss drops the frame (the relay
/// is not in our bootstrap — fabricating a socket would violate the host's
/// relay-policy declaration).
fn fan_outbound(
    drivers: &Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    outbound: Vec<OutboundMessage>,
) {
    let drivers = drivers.borrow();
    for message in outbound {
        for driver in drivers.iter().filter(|d| d.url() == message.relay_url()) {
            let _ = driver.send_text(message.text());
        }
    }
}

/// Build the [`BrowserKernelHandlers`] each [`BrowserRelayDriver`] will own.
///
/// One closure per kernel-ingest touchpoint:
///
/// 1. `on_connected` -> [`KernelReducer::handle_relay_connected`] then
///    fan-out + snapshot push.
/// 2. `on_text` -> wrap into [`RelayFrame::Text`], call
///    [`KernelReducer::handle_relay_frame`], fan-out + snapshot push.
/// 3. `on_binary` -> wrap into [`RelayFrame::Binary`], same path.
/// 4. `on_close` -> wrap into [`RelayFrame::Close`], call
///    [`KernelReducer::handle_relay_frame`] (the returned outbound is
///    always empty so we drop it; snapshot push captures
///    `relay.last_close_reason` for the next render).
/// 5. `on_closed` -> [`KernelReducer::handle_relay_closed`], snapshot push.
/// 6. `on_failed` -> [`KernelReducer::handle_relay_failed`], snapshot push.
///
/// Every closure pushes a fresh snapshot to the JS host through the
/// registered callback (if any). The push fires unconditionally after every
/// inbound — the relay-frame ingest path does not return a `KernelUpdate`,
/// so we cannot gate on "produced an update" without re-snapshotting and
/// diffing, which is more expensive than just pushing. The host's reducer
/// is idempotent on identical envelopes.
///
/// Substrate-grade (D0): the closures touch only protocol-neutral
/// [`OutboundMessage`]s and the kernel's frame-ingest entrypoints; no app
/// nouns leak through.
#[must_use]
pub(crate) fn build_handlers(
    drivers: Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    snapshot_callback: Rc<RefCell<Option<js_sys::Function>>>,
    reducer: Rc<RefCell<KernelReducer>>,
    meta: Rc<RefCell<RuntimeMeta>>,
) -> BrowserKernelHandlers {
    // Each closure clones the four `Rc` handles it needs. The driver invokes
    // them with `&str` URLs (we copy into owned `String` only where the
    // kernel API requires it, which today is zero places — every kernel
    // entrypoint takes `&str` directly).
    let on_connected = {
        let drivers = Rc::clone(&drivers);
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        Rc::new(move |role: RelayRole, url: &str, is_reconnect: bool| {
            let outbound = reducer
                .borrow_mut()
                .handle_relay_connected(role, url, is_reconnect);
            fan_outbound(&drivers, outbound);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }) as Rc<dyn Fn(RelayRole, &str, bool)>
    };

    let on_text = {
        let drivers = Rc::clone(&drivers);
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        Rc::new(move |role: RelayRole, url: &str, text: String| {
            let outbound = reducer
                .borrow_mut()
                .handle_relay_frame(role, url, RelayFrame::Text(text));
            fan_outbound(&drivers, outbound);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }) as Rc<dyn Fn(RelayRole, &str, String)>
    };

    let on_binary = {
        let drivers = Rc::clone(&drivers);
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        Rc::new(move |role: RelayRole, url: &str, bytes: Vec<u8>| {
            let outbound = reducer
                .borrow_mut()
                .handle_relay_frame(role, url, RelayFrame::Binary(bytes));
            fan_outbound(&drivers, outbound);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }) as Rc<dyn Fn(RelayRole, &str, Vec<u8>)>
    };

    let on_close = {
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        Rc::new(move |role: RelayRole, url: &str, reason: Option<String>| {
            // `RelayFrame::Close` always returns an empty outbound — we drop
            // it. Snapshot push captures `relay.last_close_reason`.
            let _ = reducer
                .borrow_mut()
                .handle_relay_frame(role, url, RelayFrame::Close(reason));
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }) as Rc<dyn Fn(RelayRole, &str, Option<String>)>
    };

    let on_closed = {
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        Rc::new(move |role: RelayRole, url: &str| {
            reducer.borrow_mut().handle_relay_closed(role, url);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }) as Rc<dyn Fn(RelayRole, &str)>
    };

    let on_failed = {
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        Rc::new(move |role: RelayRole, url: &str, error: String| {
            reducer.borrow_mut().handle_relay_failed(role, url, error);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
        }) as Rc<dyn Fn(RelayRole, &str, String)>
    };

    BrowserKernelHandlers {
        on_connected,
        on_text,
        on_binary,
        on_close,
        on_closed,
        on_failed,
    }
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
/// the bootstrap entries, wiring each driver's kernel handlers through the
/// shared callback bag. Returns the populated driver list ready to move
/// into the runtime's relay slot.
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
/// The runtime calls [`build_handlers`] BEFORE `spawn_drivers`, but the
/// handler closures capture `Rc<RefCell<Vec<…>>>` that is still empty at
/// that point. The drivers are then constructed in this loop, and only
/// after the function returns does the runtime assign `*self.relays.borrow_mut() = drivers`.
/// This is safe **because** `WebSocket::new()` returns synchronously and
/// the `onopen` JS closure cannot fire until control returns to the JS
/// event loop — which happens only after this whole function returns and
/// the runtime swaps the driver list in. By the time the first `onopen`
/// fires, the handler can find the driver via URL lookup. Any refactor
/// that moves the `onopen`-firing call site (e.g. a synchronous polyfill
/// in tests) must re-establish this invariant.
pub(crate) fn spawn_drivers(
    bootstrap: &[RelayBootstrapEntry],
    handlers: BrowserKernelHandlers,
) -> Result<Vec<Rc<BrowserRelayDriver>>, WasmRuntimeError> {
    // Reserve room for the worst case (every entry expands to two lanes)
    // so the per-entry inner loop never reallocates mid-spawn.
    let mut drivers = Vec::with_capacity(bootstrap.len() * 2);
    for entry in bootstrap {
        for &role in roles_for_entry(&entry.role) {
            let driver =
                BrowserRelayDriver::new(entry.url.clone(), role, handlers.clone()).map_err(
                    |err| {
                        WasmRuntimeError::InvalidConfig(format!(
                            "failed to open WebSocket to {}: {err:?}",
                            entry.url
                        ))
                    },
                )?;
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
