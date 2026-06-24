//! V-01 Stage 3 / 3b — relay-pool helpers for the wasm32 runtime.
//!
//! Owns the construction of the per-relay [`BrowserRelayDriver`] set, the
//! kernel-handler callback bag that bridges the driver back into
//! [`nmp_core::KernelReducer`], the outbound fan-out, the event/deadline wake,
//! and the snapshot push that fires after every kernel-mutating inbound relay
//! frame.
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

/// Fan an outbound batch to the driver whose URL matches each message,
/// spawning a driver on demand for any kernel-targeted URL not yet in the
/// pool. Used by every kernel-handler closure (connected/text/binary), the
/// event/deadline runtime wake, and the write publish path.
///
/// One driver per URL (relay pool is now URL-keyed — see
/// [`crate::relay_plan`]), so each message goes to the single matching driver;
/// there is no per-lane duplicate to fan to.
///
/// # Spawn-on-miss — the kernel owns socket lifecycle
///
/// When the kernel emits an `OutboundMessage` for a URL the pool has not
/// opened yet, this **spawns** a driver for it rather than dropping the frame
/// — mirroring the native actor's `send_outbound`, which calls
/// `ensure_relay_worker` for any new target URL. This is what lets the kernel
/// own the "which relays to dial" decision on web exactly as it does on
/// native: the kernel discovers relays (NIP-65 mailboxes, event-tag hints)
/// and targets them; the transport merely obeys.
///
/// The transport trusts the URL without re-checking admission: the router
/// already applies `RelayAdmissionPolicy` on the untrusted lanes (NIP-65
/// mailbox, hints, provenance) and filters per-account blocked relays BEFORE
/// an `OutboundMessage` exists, so every URL reaching here is already
/// admissible. (Native's `send_outbound` likewise carries no admission check.)
/// The spawned driver reports inbound frames under the message's role.
///
/// A spawn needs the `handlers` slot populated — the runtime fills it in
/// `spawn_relay_drivers` before any callback can fire. If it is empty (pool
/// not started) or the URL fails to dial (unparseable), the frame is dropped,
/// preserving the prior no-fabrication semantics for those edge cases.
///
/// # Reentrancy / borrow safety
///
/// `BrowserRelayDriver::new` dials synchronously, but its JS `onopen` /
/// `onmessage` closures cannot fire until control returns to the event loop,
/// so no nested `fan_out_outbound` runs while a `borrow`/`borrow_mut` here is
/// held. Each `drivers` borrow is scoped to a single statement.
pub(crate) fn fan_out_outbound(
    drivers: &Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    handlers: &Rc<RefCell<Option<BrowserKernelHandlers>>>,
    outbound: &[OutboundMessage],
) {
    for message in outbound {
        let url = message.relay_url();
        let known = drivers.borrow().iter().any(|driver| driver.url() == url);
        if !known {
            // Spawn-on-miss: the kernel targeted a URL not yet in the pool.
            let Some(handlers) = handlers.borrow().as_ref().cloned() else {
                // Pool not started (no handler bag) — preserve drop semantics.
                continue;
            };
            match BrowserRelayDriver::new(url.to_string(), message.role(), handlers) {
                Ok(driver) => drivers.borrow_mut().push(driver),
                // Unparseable/invalid URL — nothing safe to dial; drop it.
                Err(_err) => continue,
            }
        }
        for driver in drivers.borrow().iter().filter(|driver| driver.url() == url) {
            let _ = driver.send_text(message.text());
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn request_runtime_deadline(
    deadline: &Rc<RefCell<crate::tick::RuntimeDeadline>>,
    policy: crate::tick::WakePolicy,
    reducer: &Rc<RefCell<KernelReducer>>,
    drivers: &Rc<RefCell<Vec<Rc<BrowserRelayDriver>>>>,
    handlers_slot: &Rc<RefCell<Option<BrowserKernelHandlers>>>,
    snapshot_callback: &Rc<RefCell<Option<js_sys::Function>>>,
    meta: &Rc<RefCell<RuntimeMeta>>,
    post_event_drain: &Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    crate::tick::request_runtime_deadline(
        Rc::clone(deadline),
        policy,
        Rc::clone(reducer),
        Rc::clone(drivers),
        Rc::clone(handlers_slot),
        Rc::clone(snapshot_callback),
        Rc::clone(meta),
        Rc::clone(post_event_drain),
    );
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
    handlers_slot: Rc<RefCell<Option<BrowserKernelHandlers>>>,
    deadline: Rc<RefCell<crate::tick::RuntimeDeadline>>,
    post_event_drain: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) -> BrowserKernelHandlers {
    // Each closure clones the `Rc` handles it needs. The driver invokes them
    // with `&str` URLs (we copy into owned `String` only where the kernel API
    // requires it, which today is zero places — every kernel entrypoint takes
    // `&str` directly). `handlers_slot` is the self-referential bag the runtime
    // populates with these very handlers after `build_handlers` returns; the
    // closures pass it to `fan_out_outbound` so a kernel frame that targets a
    // not-yet-open URL can spawn a driver on demand (the slot is non-empty by
    // the time any callback can fire — see the ordering invariant on
    // `spawn_drivers`).
    let on_connected = {
        let drivers = Rc::clone(&drivers);
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        let handlers_slot = Rc::clone(&handlers_slot);
        let deadline = Rc::clone(&deadline);
        let post_event_drain = Rc::clone(&post_event_drain);
        Rc::new(move |role: RelayRole, url: &str, is_reconnect: bool| {
            let outbound = reducer
                .borrow_mut()
                .handle_relay_connected(role, url, is_reconnect);
            let policy = if outbound.is_empty() {
                crate::tick::WakePolicy::Single
            } else {
                crate::tick::WakePolicy::Tracked
            };
            fan_out_outbound(&drivers, &handlers_slot, &outbound);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            request_runtime_deadline(
                &deadline,
                policy,
                &reducer,
                &drivers,
                &handlers_slot,
                &snapshot_callback,
                &meta,
                &post_event_drain,
            );
        }) as Rc<dyn Fn(RelayRole, &str, bool)>
    };

    let on_text = {
        let drivers = Rc::clone(&drivers);
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        let handlers_slot = Rc::clone(&handlers_slot);
        let deadline = Rc::clone(&deadline);
        let post_event_drain = Rc::clone(&post_event_drain);
        Rc::new(move |role: RelayRole, url: &str, text: String| {
            let outbound =
                reducer
                    .borrow_mut()
                    .handle_relay_frame(role, url, RelayFrame::Text(text));
            let policy = if outbound.is_empty() {
                crate::tick::WakePolicy::Single
            } else {
                crate::tick::WakePolicy::Tracked
            };
            fan_out_outbound(&drivers, &handlers_slot, &outbound);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            request_runtime_deadline(
                &deadline,
                policy,
                &reducer,
                &drivers,
                &handlers_slot,
                &snapshot_callback,
                &meta,
                &post_event_drain,
            );
        }) as Rc<dyn Fn(RelayRole, &str, String)>
    };

    let on_binary = {
        let drivers = Rc::clone(&drivers);
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        let handlers_slot = Rc::clone(&handlers_slot);
        let deadline = Rc::clone(&deadline);
        let post_event_drain = Rc::clone(&post_event_drain);
        Rc::new(move |role: RelayRole, url: &str, bytes: Vec<u8>| {
            let outbound =
                reducer
                    .borrow_mut()
                    .handle_relay_frame(role, url, RelayFrame::Binary(bytes));
            let policy = if outbound.is_empty() {
                crate::tick::WakePolicy::Single
            } else {
                crate::tick::WakePolicy::Tracked
            };
            fan_out_outbound(&drivers, &handlers_slot, &outbound);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            request_runtime_deadline(
                &deadline,
                policy,
                &reducer,
                &drivers,
                &handlers_slot,
                &snapshot_callback,
                &meta,
                &post_event_drain,
            );
        }) as Rc<dyn Fn(RelayRole, &str, Vec<u8>)>
    };

    let on_close = {
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        let drivers = Rc::clone(&drivers);
        let handlers_slot = Rc::clone(&handlers_slot);
        let deadline = Rc::clone(&deadline);
        let post_event_drain = Rc::clone(&post_event_drain);
        Rc::new(move |role: RelayRole, url: &str, reason: Option<String>| {
            // `RelayFrame::Close` always returns an empty outbound — we drop
            // it. Snapshot push captures `relay.last_close_reason`.
            let _ = reducer
                .borrow_mut()
                .handle_relay_frame(role, url, RelayFrame::Close(reason));
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            request_runtime_deadline(
                &deadline,
                crate::tick::WakePolicy::Single,
                &reducer,
                &drivers,
                &handlers_slot,
                &snapshot_callback,
                &meta,
                &post_event_drain,
            );
        }) as Rc<dyn Fn(RelayRole, &str, Option<String>)>
    };

    let on_closed = {
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        let drivers = Rc::clone(&drivers);
        let handlers_slot = Rc::clone(&handlers_slot);
        let deadline = Rc::clone(&deadline);
        let post_event_drain = Rc::clone(&post_event_drain);
        Rc::new(move |role: RelayRole, url: &str| {
            reducer.borrow_mut().handle_relay_closed(role, url);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            request_runtime_deadline(
                &deadline,
                crate::tick::WakePolicy::Single,
                &reducer,
                &drivers,
                &handlers_slot,
                &snapshot_callback,
                &meta,
                &post_event_drain,
            );
        }) as Rc<dyn Fn(RelayRole, &str)>
    };

    let on_failed = {
        let reducer = Rc::clone(&reducer);
        let snapshot_callback = Rc::clone(&snapshot_callback);
        let meta = Rc::clone(&meta);
        let drivers = Rc::clone(&drivers);
        let handlers_slot = Rc::clone(&handlers_slot);
        let deadline = Rc::clone(&deadline);
        let post_event_drain = Rc::clone(&post_event_drain);
        Rc::new(move |role: RelayRole, url: &str, error: String| {
            reducer.borrow_mut().handle_relay_failed(role, url, error);
            push_snapshot_if_callback(&snapshot_callback, &reducer, &meta);
            request_runtime_deadline(
                &deadline,
                crate::tick::WakePolicy::Single,
                &reducer,
                &drivers,
                &handlers_slot,
                &snapshot_callback,
                &meta,
                &post_event_drain,
            );
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

/// Instantiate one [`BrowserRelayDriver`] per **distinct URL** derived from
/// the bootstrap entries, wiring each driver's kernel handlers through the
/// shared callback bag. Returns the populated driver list ready to move
/// into the runtime's relay slot.
///
/// One socket per URL — native parity. A `"both,indexer"` URL yields a
/// **single** driver, recorded under the native first-role-wins primary
/// (`Content`), not two. The native pool keys sockets by URL alone
/// (`nmp_network::pool::ensure_open` ignores the role once a URL is open), so
/// per-`(URL, role)` drivers were a wasm-only divergence that opened duplicate
/// WebSockets to the same host. Inbound role is diagnostics-only (the kernel
/// ingests events identically regardless of role and routes outbound purely by
/// URL), and the host's full declared role set still reaches the UI via the
/// kernel's `configured_relays` projection — so collapsing is
/// behaviour-preserving. The dedup/role-collapse decision lives in the
/// always-compiled, native-tested [`crate::relay_plan::plan_drivers`].
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
    let plans = crate::relay_plan::plan_drivers(bootstrap);
    let mut drivers = Vec::with_capacity(plans.len());
    for plan in plans {
        let driver = BrowserRelayDriver::new(plan.url.clone(), plan.primary_role, handlers.clone())
            .map_err(|err| {
                WasmRuntimeError::InvalidConfig(format!(
                    "failed to open WebSocket to {}: {err:?}",
                    plan.url
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
