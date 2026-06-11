//! Relay lifecycle helpers — spawning, closing, routing outbound messages.
//!
//! # T105 — URL-keyed transport pool
//!
//! `relay_controls` is keyed by **resolved relay URL**, not by `RelayRole`.
//! `send_outbound` dispatches each `OutboundMessage` by its `relay_url`, and
//! a worker is spawned **on demand** the first time a new URL appears (cold
//! discovery seed at startup, then per resolved write/read relay as the
//! kernel resolves NIP-65 mailboxes). `connected_relays` is still per-`RelayRole`
//! to drive the diagnostic surface (one row per lane) until M11 makes
//! per-URL health a first-class part of the FFI projection.
//!
//! # Compiler-enforced canonical pool keys
//!
//! The pool is `HashMap<CanonicalRelayUrl, RelayControl>`. The key type makes
//! the canonicalization invariant *unrepresentable to violate*: a raw `&str`
//! cannot index the pool, so every lookup/insert site must first run
//! [`CanonicalRelayUrl::parse_or_raw`]. This extends the compiler enforcement
//! introduced for the kernel's `wire_subs` / `persistent_subs` maps (PR #7)
//! into the actor transport layer — replacing the prior pattern of callers
//! remembering to call `canonical_relay_url()` before a `HashMap<String, _>`
//! lookup.
//!
//! # Step 8 phase F — `nmp_network::pool::Pool` cut-over
//!
//! The actor used to drive `nmp_network::relay_worker::spawn_relay_worker`
//! directly, holding a per-URL `Sender<RelayCommand>` in `RelayControl.tx`
//! and consuming a single shared `Receiver<RelayEvent>` in the main loop.
//! Phase F migrates every callsite onto the push-model
//! [`nmp_network::pool::Pool`] API: workers are owned by the pool;
//! `ensure_open` returns a generational [`nmp_network::pool::RelayHandle`]
//! the actor stores in [`RelayControl`]; outbound frames go through
//! `pool.send(handle, WireFrame::Text(..))`; teardown is `pool.close(handle)`.
//! The generational handle gives us structural stale-handle rejection
//! (the pool drops events whose generation no longer matches the slot),
//! and the per-URL state machine, keepalive FSM, and jittered backoff are
//! preserved bit-for-bit (Pool wraps the same worker lifecycle).

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use nmp_network::pool::{Pool, WireFrame};
use serde_json::json;
use std::collections::{HashMap, HashSet};

use super::{RelayConnectionKind, RelayControl};

/// True when at least one URL on **every** lane has reported `Connected`.
///
/// Historical send-gate semantics. No longer used in production: V-87 #602
/// decoupled startup interests from it, and Fix A (the universal latent-bug fix)
/// replaced the claim/open send-gate with [`claim_send_gate`] — the `all` gate
/// parked every claim forever when one bootstrap lane (e.g. the Indexer) never
/// opened its socket. Retained `#[cfg(test)]` as the contrast reference in
/// `send_gate_universal_tests` (proving `all` is false while `claim_send_gate`
/// is true when one lane is offline).
#[cfg(test)]
pub(super) fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .all(|role| connected_relays.contains(&role))
}

/// THE claim/open send-gate — the single production decision point for whether
/// a `claim_event` / `claim_profile` / `open_interest` /
/// `open_firehose` / sign-in-driven retarget sends its REQ now or parks until a
/// relay connects. Its value is computed once per dispatch at `actor/mod.rs` and
/// fed to every consumer as `relays_ready` / `can_send`.
/// V-68 / V-112 (ADR-0042): `open_author` / `open_thread` deleted.
///
/// # Fix A — universal latent-bug fix (`all` → `any`)
///
/// Returns `true` as soon as **any** bootstrap lane (`Content` or `Indexer`) has
/// reported `Connected`. Previously this gate required **every** lane
/// ([`all_relays_connected`]); if one bootstrap lane never opened its socket
/// (the Android emulator's `purplepag.es` Indexer lane), the gate was
/// permanently `false` and every claim/open parked forever with no REQ — even
/// for an nevent carrying a working relay hint.
///
/// Relaxing to `any` is correct and behavior-preserving:
/// * Every consumer treats this as a **park-until-connect readiness heuristic**,
///   not a correctness invariant requiring all lanes. Relays that connect later
///   pick up the compiled REQ via the planner's reconnect-replay path (the same
///   decoupling V-87 #602 applied to startup interests).
/// * For hosts that connect all bootstrap lanes (iOS, the TUI smoke), `any` is
///   reached no later than `all`, so the gate flips `true` at the same point or
///   earlier — they pass identically before and after this change. They were
///   never special-cased; they only ever passed because their environment
///   connects both lanes.
pub(super) fn claim_send_gate(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .any(|role| connected_relays.contains(&role))
}

/// Lane-bootstrap seeds: spawn one worker per configured URL returned by
/// `kernel.bootstrap_urls_for_role(role)`. Called from `Start` so the cold-start
/// kind:10002 discovery fetch has a socket to leave on before any NIP-65
/// list is cached. Per-author/recipient sockets spawn on demand in
/// `send_outbound` as the kernel emits `OutboundMessages` targeting their
/// resolved relay URLs.
pub(super) fn spawn_missing_relays(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) {
    for role in RelayRole::all() {
        for url in kernel.bootstrap_urls_for_role(role) {
            ensure_relay_worker(
                relay_controls,
                slot_to_url,
                pool,
                kernel,
                next_relay_generation,
                role,
                url,
            );
        }
    }
}

/// Spawn (if missing) a worker for `(role, relay_url)` via the
/// [`Pool::ensure_open_with_role`] entry point and stamp the kernel's per-role
/// health row as `connecting`. Returns true iff a new worker was spawned (the
/// URL was previously unseen). On-demand path: any `OutboundMessage` carrying
/// a URL the pool has never seen gets a fresh socket here before
/// `send_outbound` enqueues the frame.
///
/// T-relay-url-normalize: `relay_url` is passed through
/// [`CanonicalRelayUrl::parse_or_raw`] before the pool-key lookup so that
/// URL-equivalent forms (differing only in case, trailing-slash-on-empty-path,
/// or leading whitespace) all resolve to the same pool entry. If the URL
/// cannot be canonicalized (e.g. a bootstrap seed that is already
/// lowercase+clean), the raw string is wrapped unchanged — existing bootstrap
/// behaviour is preserved. The newtype key makes this canonicalization the
/// only way to obtain a pool key, so a raw `&str` can no longer index the map.
///
/// Phase F: `next_relay_generation` is no longer the worker-side generation
/// (the pool owns that now). It survives as the **actor-visible** generation
/// stamped onto `RelayControl` so the staleness check in the main loop
/// against `event.h.generation()` keeps the same observable behaviour during
/// the transition. The pool's translator already drops events with a stale
/// slot-generation, so this counter is effectively a belt-and-braces marker
/// the actor uses for symmetry with the previous design; it is bumped on
/// every fresh `ensure_open` whose URL was not already in the pool.
#[allow(clippy::too_many_arguments)]
pub(super) fn ensure_relay_worker(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    role: RelayRole,
    relay_url: String,
) -> bool {
    ensure_relay_worker_with_kind(
        relay_controls,
        slot_to_url,
        pool,
        kernel,
        next_relay_generation,
        role,
        relay_url,
        RelayConnectionKind::Persistent,
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn ensure_relay_worker_with_kind(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    role: RelayRole,
    relay_url: String,
    connection_kind: RelayConnectionKind,
) -> bool {
    // Canonicalize the URL so all callers (add, send_outbound, bootstrap)
    // agree on the pool key. Fall back to wrapping the raw string for URLs
    // that don't parse as ws/wss (e.g. bootstrap seeds that are already
    // canonical).
    let key = CanonicalRelayUrl::parse_or_raw(&relay_url);
    if let Some(control) = relay_controls.get_mut(&key) {
        if connection_kind == RelayConnectionKind::Persistent {
            control.connection_kind = RelayConnectionKind::Persistent;
            control.idle_since = None;
        }
        return false;
    }
    let generation = *next_relay_generation;
    *next_relay_generation = generation.saturating_add(1);
    let key_str = key.clone().into_string();
    kernel.relay_connecting_url(role, &key_str);
    // Hand the canonical URL to the pool. `Pool::ensure_open_with_role` does
    // its own (lighter) canonicalization but the input is already the canonical
    // form so it round-trips byte-identically.
    let handle = pool.ensure_open_with_role(&key_str, role);
    slot_to_url.insert(handle.slot(), key.clone());
    relay_controls.insert(
        key,
        RelayControl {
            generation,
            role,
            relay_url: key_str,
            handle,
            connection_kind,
            idle_since: None,
        },
    );
    true
}

/// Register startup interests and flush pending view requests once the actor
/// is running — regardless of relay connectivity.
///
/// V-87 #602 — D1 / offline-first §3: startup must not wait for relays.
///
/// **Previous behaviour (violation):** `all_relays_connected` gated the entire
/// function.  One tardy lane (e.g. Indexer) delayed `startup_requests()` (the
/// bootstrap interest registration) indefinitely, meaning the planner never
/// received its compile triggers and sent no REQs even on connected lanes.
///
/// **New behaviour:** the relay-connectivity check is removed.
/// `startup_requests()` registers bootstrap interests via the planner
/// immediately — it returns `Vec::new()` (no direct wire send); the planner
/// compiles those interests into wire REQs on the next `drain_lifecycle_tick`
/// and routes them to whatever relays are open at that point (or holds them
/// until a relay connects, via `reconnect_replay`).  `pending_view_requests()`
/// likewise drains the deferred-outbound queue; `send_all_outbound` spawns pool
/// workers on demand, so frames queue in the worker's send buffer until the
/// socket is live — no connectivity pre-condition needed at the actor level.
///
/// The `startup_sent` flag is still used as a one-shot gate so this function
/// fires exactly once per `Start` / `Reset` cycle regardless of how many relay
/// `Opened` events arrive.
#[allow(clippy::too_many_arguments)]
pub(super) fn maybe_send_startup(
    running: bool,
    startup_sent: &mut bool,
    // `connected_relays` is retained in the signature for call-site
    // compatibility; it is no longer used as a gate here (V-87 #602).
    _connected_relays: &HashSet<RelayRole>,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) -> bool {
    if !running || *startup_sent {
        return false;
    }

    let startup_requests = kernel.startup_requests();
    send_all_outbound(
        relay_controls,
        slot_to_url,
        pool,
        kernel,
        next_relay_generation,
        startup_requests,
    );
    let view_requests = kernel.pending_view_requests();
    send_all_outbound(
        relay_controls,
        slot_to_url,
        pool,
        kernel,
        next_relay_generation,
        view_requests,
    );
    *startup_sent = true;
    true
}

pub(super) fn send_all_outbound(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    outbound: Vec<OutboundMessage>,
) {
    // M5+M2+M8 wiring: every outbound batch passes through the AUTH-pause
    // partition before hitting the wire. REQs targeting an AUTH-paused
    // relay (ChallengeReceived / Authenticating) are diverted into the
    // deferred queue and replayed on the next tick after Authenticated.
    let outbound = kernel.partition_auth_paused(outbound);
    for message in outbound {
        send_outbound(
            relay_controls,
            slot_to_url,
            pool,
            kernel,
            next_relay_generation,
            message,
        );
    }
}

/// Route command-produced outbound frames through the relay pool.
/// Non-publish frames remain running-gated; publish `EVENT` frames are retained
/// in actor memory until the next running cycle, while `PublishEngine` remains
/// the durable source of truth for process restart resume.
#[allow(clippy::too_many_arguments)]
pub(super) fn route_dispatch_outbound(
    running: bool,
    queued_publish_outbound: &mut Vec<OutboundMessage>,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    outbound: Vec<OutboundMessage>,
) {
    if running {
        let queued = take_non_duplicate_queued(queued_publish_outbound, &outbound);
        send_all_outbound(
            relay_controls,
            slot_to_url,
            pool,
            kernel,
            next_relay_generation,
            queued,
        );
        send_all_outbound(
            relay_controls,
            slot_to_url,
            pool,
            kernel,
            next_relay_generation,
            outbound,
        );
    } else {
        queue_publish_outbound(queued_publish_outbound, outbound);
    }
}

fn queue_publish_outbound(
    queued_publish_outbound: &mut Vec<OutboundMessage>,
    outbound: Vec<OutboundMessage>,
) {
    for message in outbound {
        if publish_message_key(&message).is_some() {
            queued_publish_outbound.push(message);
        }
    }
}

fn take_non_duplicate_queued(
    queued_publish_outbound: &mut Vec<OutboundMessage>,
    outbound: &[OutboundMessage],
) -> Vec<OutboundMessage> {
    if queued_publish_outbound.is_empty() {
        return Vec::new();
    }
    let current_keys = outbound
        .iter()
        .filter_map(publish_message_key)
        .collect::<HashSet<_>>();
    let queued = std::mem::take(queued_publish_outbound);
    queued
        .into_iter()
        .filter(|message| {
            publish_message_key(message).is_none_or(|key| !current_keys.contains(&key))
        })
        .collect()
}

fn publish_message_key(message: &OutboundMessage) -> Option<(String, String)> {
    if message.relay_url.trim().is_empty() {
        return None;
    }
    let parsed = serde_json::from_str::<serde_json::Value>(&message.text).ok()?;
    let array = parsed.as_array()?;
    if array.first()?.as_str()? != "EVENT" {
        return None;
    }
    let event_id = array.get(1)?.get("id")?.as_str()?;
    Some((message.relay_url.clone(), event_id.to_string()))
}

/// Route one `OutboundMessage` to the pool worker for its `relay_url`. Spawns
/// a new worker on first sight (per-URL on-demand). The previous role-based
/// fallback (defer when role's socket is missing) is gone — every message
/// resolves a concrete URL now (T105).
///
/// T-relay-url-normalize: both the spawn call and the subsequent pool lookup
/// must use the same canonical key. `ensure_relay_worker` canonicalizes
/// internally and stores the canonical key, so the `relay_controls.get()`
/// must also use the canonical form — otherwise a non-canonical
/// `message.relay_url` (trailing slash / uppercase scheme) would miss the
/// entry and silently defer the frame forever.
pub(super) fn send_outbound(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    message: OutboundMessage,
) {
    // Resolve to the canonical pool key first so both the spawn and the
    // subsequent lookup agree on the same HashMap entry. `ensure_relay_worker`
    // takes a `String`; the `CanonicalRelayUrl` key is what indexes the pool here.
    let canonical_key = CanonicalRelayUrl::parse_or_raw(&message.relay_url);
    let connection_kind = if message.role == RelayRole::Wallet
        || kernel.relay_socket_is_persistent(&canonical_key, message.role)
    {
        RelayConnectionKind::Persistent
    } else {
        RelayConnectionKind::Temporary
    };

    // Spawn on demand for any URL the pool has not seen before. The
    // diagnostic lane is `message.role`; the actual socket dials `canonical_key`.
    let _spawned = ensure_relay_worker_with_kind(
        relay_controls,
        slot_to_url,
        pool,
        kernel,
        next_relay_generation,
        message.role,
        canonical_key.clone().into_string(),
        connection_kind,
    );

    let Some(control) = relay_controls.get_mut(&canonical_key) else {
        // ensure_relay_worker only fails to insert under a logic bug — defer
        // so the frame isn't dropped silently.
        kernel.defer_outbound(message);
        return;
    };
    control.idle_since = None;
    let handle = control.handle;

    kernel.record_tx_to(message.role, canonical_key.as_str(), message.text.len());
    // Phase F: a stale (or sentinel) handle returns false from `Pool::send`;
    // we treat that exactly like the previous "channel disconnected" path —
    // mark the per-URL row as retrying and move on. The pool's own
    // reconnect/backoff will surface a fresh `Opened` event when the socket
    // recovers (or `Failed`/`Closed` when it doesn't).
    if !pool.send(handle, WireFrame::Text(message.text)) {
        // T105: the dead handle is this specific socket — scope the
        // `retrying` mark to its URL, not the whole role lane.
        kernel.relay_failed(
            message.role,
            canonical_key.as_str(),
            "relay worker stopped".to_string(),
        );
    }
}

/// Shut down the worker for `url` (if one exists) and remove it from the pool.
///
/// Mirrors `ensure_relay_worker` in the remove direction. Calls
/// [`Pool::close`] on the stored handle, which sends a shutdown command to
/// the worker; the worker thread closes the socket and the pool's translator
/// emits a [`nmp_network::pool::PoolEvent::Closed`] for the actor loop. The
/// `relay_controls` entry is dropped immediately so the URL is no longer in
/// the pool — future `ensure_relay_worker` calls for the same URL will spawn
/// a fresh worker (the pool reopens the slot with a bumped generation; the
/// T126 one-socket-per-URL invariant is preserved).
///
/// T-relay-url-normalize: `url` is canonicalized before the pool-key lookup so
/// that removing `"wss://R.Ex/"` correctly finds the entry stored under the
/// canonical key `"wss://r.ex"`. If the URL cannot be canonicalized, the raw
/// string is tried as-is (idempotent, no panic).
///
/// Returns `true` if a worker was found and shut down, `false` if the URL was
/// not in the pool (idempotent, no panic).
pub(super) fn shutdown_relay_worker(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    url: &str,
) -> bool {
    let key = CanonicalRelayUrl::parse_or_raw(url);
    let Some(control) = relay_controls.remove(&key) else {
        return false;
    };
    slot_to_url.remove(&control.handle.slot());
    // Best-effort close: stale handle / already-closed slot returns false
    // from `Pool::close`, which is the correct behaviour for an idempotent
    // shutdown (the entry is gone from the pool either way).
    let _ = pool.close(control.handle);
    true
}

pub(super) fn close_relays(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    pool: &Pool,
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    // Close every active wire-sub on every per-URL socket. The kernel's
    // `active_subscriptions(role)` enumerates WireSubs by lane — we route
    // each CLOSE to the socket the sub was opened on (URL recorded in
    // WireSub by req_for_relay).
    let active = kernel.snapshot_active_wire_subs();
    for (sub_id, relay_url) in active {
        // T-relay-url-normalize: wire-sub URLs may carry non-canonical forms
        // (trailing slash, uppercase scheme) — canonicalize before pool lookup
        // so the CLOSE frame reaches the correct worker.
        let key = CanonicalRelayUrl::parse_or_raw(&relay_url);
        if let Some(control) = relay_controls.get(&key) {
            let close = json!(["CLOSE", sub_id]).to_string();
            let _ = pool.send(control.handle, WireFrame::Text(close));
        }
    }
    for (_url, control) in relay_controls.drain() {
        slot_to_url.remove(&control.handle.slot());
        let _ = pool.close(control.handle);
    }
    // Mirror the lane-level "closed" status into the kernel diagnostics.
    bootstrap_lane_close(connected_relays, kernel);
}

/// Mark each lane as closed once all its sockets are gone (post-drain).
fn bootstrap_lane_close(connected_relays: &mut HashSet<RelayRole>, kernel: &mut Kernel) {
    for role in RelayRole::all() {
        connected_relays.remove(&role);
        // Global teardown: every socket of every role is being drained, so
        // evict the whole lane (the per-URL `relay_closed` would force the
        // caller to enumerate sockets it is discarding anyway — T105).
        kernel.relay_closed_all(role);
    }
    // Cold-start bootstrap seeds will be respawned from configured_relays on the next Start cycle.
}

#[cfg(test)]
#[path = "relay_mgmt/tests.rs"]
mod tests;
