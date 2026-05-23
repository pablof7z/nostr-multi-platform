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

use crate::kernel::Kernel;
use crate::relay::{CanonicalRelayUrl, OutboundMessage, RelayRole};
use crate::relay_worker::{spawn_relay_worker, RelayCommand, RelayEvent};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::sync::mpsc::Sender;

use super::RelayControl;

/// True when at least one URL on **every** lane has reported `Connected`.
/// Used as the startup-send gate so the first burst of REQs has somewhere to
/// land. Per-lane (`RelayRole`) granularity matches the diagnostic surface;
/// M11 will sharpen this to per-URL once the FFI projection lands.
pub(super) fn all_relays_connected(connected_relays: &HashSet<RelayRole>) -> bool {
    RelayRole::all()
        .into_iter()
        .all(|role| connected_relays.contains(&role))
}

/// Lane-bootstrap seeds: spawn one worker per configured URL returned by
/// `kernel.bootstrap_urls_for_role(role)`. Called from `Start` so the cold-start
/// kind:10002 discovery fetch has a socket to leave on before any NIP-65
/// list is cached. Per-author/recipient sockets spawn on demand in
/// `send_outbound` as the kernel emits `OutboundMessages` targeting their
/// resolved relay URLs.
pub(super) fn spawn_missing_relays(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) {
    for role in RelayRole::all() {
        for url in kernel.bootstrap_urls_for_role(role) {
            ensure_relay_worker(
                relay_controls,
                relay_tx,
                kernel,
                next_relay_generation,
                role,
                url,
            );
        }
    }
}

/// Spawn (if missing) a worker for `(role, relay_url)` and stamp the kernel's
/// per-role health row as `connecting`. Returns true iff a new worker was
/// spawned (the URL was previously unseen). On-demand path: any
/// `OutboundMessage` carrying a URL the pool has never seen gets a fresh
/// socket here before `send_outbound` enqueues the frame.
///
/// T-relay-url-normalize: `relay_url` is passed through
/// [`CanonicalRelayUrl::parse_or_raw`] before the pool-key lookup so that
/// URL-equivalent forms (differing only in case, trailing-slash-on-empty-path,
/// or leading whitespace) all resolve to the same pool entry. If the URL
/// cannot be canonicalized (e.g. a bootstrap seed that is already
/// lowercase+clean), the raw string is wrapped unchanged — existing bootstrap
/// behaviour is preserved. The newtype key makes this canonicalization the
/// only way to obtain a pool key, so a raw `&str` can no longer index the map.
pub(super) fn ensure_relay_worker(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    role: RelayRole,
    relay_url: String,
) -> bool {
    // Canonicalize the URL so all callers (add, send_outbound, bootstrap)
    // agree on the pool key. Fall back to wrapping the raw string for URLs
    // that don't parse as ws/wss (e.g. bootstrap seeds that are already
    // canonical).
    let key = CanonicalRelayUrl::parse_or_raw(&relay_url);
    if relay_controls.contains_key(&key) {
        return false;
    }
    let generation = *next_relay_generation;
    *next_relay_generation = generation.saturating_add(1);
    kernel.relay_connecting(role);
    // `spawn_relay_worker` and `RelayControl.relay_url` both take `String`
    // (the transport-worker API stays string-typed); hand them the canonical
    // inner string while the pool key keeps the `CanonicalRelayUrl` newtype.
    let key_str = key.clone().into_string();
    relay_controls.insert(
        key,
        RelayControl {
            generation,
            role,
            relay_url: key_str.clone(),
            tx: spawn_relay_worker(role, key_str, generation, relay_tx.clone()),
        },
    );
    true
}

pub(super) fn maybe_send_startup(
    running: bool,
    startup_sent: &mut bool,
    connected_relays: &HashSet<RelayRole>,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
) -> bool {
    if !running || *startup_sent || !all_relays_connected(connected_relays) {
        return false;
    }

    let startup_requests = kernel.startup_requests();
    send_all_outbound(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        startup_requests,
    );
    let view_requests = kernel.pending_view_requests();
    send_all_outbound(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        view_requests,
    );
    *startup_sent = true;
    true
}

pub(super) fn send_all_outbound(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
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
            relay_tx,
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
pub(super) fn route_dispatch_outbound(
    running: bool,
    queued_publish_outbound: &mut Vec<OutboundMessage>,
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    outbound: Vec<OutboundMessage>,
) {
    if running {
        let queued = take_non_duplicate_queued(queued_publish_outbound, &outbound);
        send_all_outbound(relay_controls, relay_tx, kernel, next_relay_generation, queued);
        send_all_outbound(
            relay_controls,
            relay_tx,
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

/// Route one `OutboundMessage` to the worker for its `relay_url`. Spawns a
/// new worker on first sight (per-URL on-demand). The previous role-based
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
    relay_tx: &Sender<RelayEvent>,
    kernel: &mut Kernel,
    next_relay_generation: &mut u64,
    message: OutboundMessage,
) {
    // Resolve to the canonical pool key first so both the spawn and the
    // subsequent lookup agree on the same HashMap entry. `ensure_relay_worker`
    // takes a `String` (the transport-worker API stays string-typed); the
    // `CanonicalRelayUrl` key is what indexes the pool here.
    let canonical_key = CanonicalRelayUrl::parse_or_raw(&message.relay_url);

    // Spawn on demand for any URL the pool has not seen before. The
    // diagnostic lane is `message.role`; the actual socket dials `canonical_key`.
    let _spawned = ensure_relay_worker(
        relay_controls,
        relay_tx,
        kernel,
        next_relay_generation,
        message.role,
        canonical_key.clone().into_string(),
    );

    let Some(control) = relay_controls.get(&canonical_key) else {
        // ensure_relay_worker only fails to insert under a logic bug — defer
        // so the frame isn't dropped silently.
        kernel.defer_outbound(message);
        return;
    };

    kernel.record_tx(message.role, message.text.len());
    if control.tx.send(RelayCommand::Send(message.text)).is_err() {
        // T105: the dead channel is this specific socket — scope the
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
/// Mirrors `ensure_relay_worker` in the remove direction. Sends
/// `RelayCommand::Shutdown` to the worker, which causes the worker thread to
/// close the socket and emit `RelayEvent::Closed` back to the actor loop.
/// The `relay_controls` entry is dropped immediately so the URL is no longer
/// in the pool — future `ensure_relay_worker` calls for the same URL will
/// spawn a fresh worker (T126 invariant preserved).
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
    url: &str,
) -> bool {
    let key = CanonicalRelayUrl::parse_or_raw(url);
    let Some(control) = relay_controls.remove(&key) else {
        return false;
    };
    // Best-effort send: if the worker channel is already closed the worker
    // has already exited — treat as success (the entry is gone from the pool).
    let _ = control.tx.send(RelayCommand::Shutdown);
    true
}

pub(super) fn close_relays(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
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
            let _ = control.tx.send(RelayCommand::Send(close));
        }
    }
    for (_url, control) in relay_controls.drain() {
        let _ = control.tx.send(RelayCommand::Shutdown);
    }
    // Mirror the lane-level "closed" status into the kernel diagnostics.
    bootstrap_lane_close(connected_relays, kernel);
}

/// Mark each lane as closed once all its sockets are gone (post-drain).
fn bootstrap_lane_close(
    connected_relays: &mut HashSet<RelayRole>,
    kernel: &mut Kernel,
) {
    for role in RelayRole::all() {
        connected_relays.remove(&role);
        // Global teardown: every socket of every role is being drained, so
        // evict the whole lane (the per-URL `relay_closed` would force the
        // caller to enumerate sockets it is discarding anyway — T105).
        kernel.relay_closed_all(role);
    }
    // Cold-start bootstrap seeds will be respawned from relay_edit_rows on the next Start cycle.
}

#[cfg(test)]
#[path = "relay_mgmt/tests.rs"]
mod tests;
