//! Inbound relay-event handler (`handle_relay_event`) and its helper
//! `resolve_handle`.
//!
//! Extracted from `dispatch.rs` to keep `mod.rs` under the LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.

use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Instant;

use nmp_network::pool::{BackoffClass, Pool, PoolEvent, RelayFrame as PoolFrame};

use crate::kernel::{BackoffHint, Kernel, RelayFrame};
use crate::relay::CanonicalRelayUrl;
use nmp_network::role::RelayRole;

use super::super::relay_mgmt::{maybe_send_startup, send_all_outbound};
use super::super::relay_runtime::RelayRuntime;
use super::super::tick::emit_now;

/// Convert a [`nmp_network::pool::RelayFrame`] (the wire frame variant the
/// pool's translator emits) into the kernel's wire-transport-agnostic
/// [`RelayFrame`] consumed by `Kernel::handle_message`.
///
/// Step 8 phase F: replaces the prior `tungstenite::Message → RelayFrame`
/// adapter — the pool already owns that conversion in its translator thread,
/// so this adapter is now a pure variant-rename (1:1 mapping). The
/// [`PoolFrame::Auth`] variant (phase E pre-classification) is round-tripped
/// to `RelayFrame::Text` by reconstructing the canonical
/// `["AUTH", <challenge>]` text frame; the kernel's existing
/// `auth_handlers.rs` ingest path then sees an unchanged surface.
/// `nmp-network`'s `nmp-nip42-types` parser already validated the shape on
/// the way in, so the round-trip is structural.
fn pool_frame_to_relay_frame(frame: PoolFrame) -> RelayFrame {
    match frame {
        PoolFrame::Text(text) => RelayFrame::Text(text),
        PoolFrame::Auth(challenge) => {
            // Reconstruct the canonical NIP-42 wire shape so the kernel
            // ingest's existing `["AUTH", ...]` parse path handles it
            // unchanged (the wire-layer pre-classification is opportunistic;
            // the kernel still owns the AUTH state machine).
            let payload = serde_json::json!(["AUTH", challenge]).to_string();
            RelayFrame::Text(payload)
        }
        PoolFrame::Binary(bytes) => RelayFrame::Binary(bytes),
        PoolFrame::Ping => RelayFrame::Ping,
        PoolFrame::Pong => RelayFrame::Pong,
        PoolFrame::Close(reason) => RelayFrame::Close(reason),
    }
}

/// Resolve a [`nmp_network::pool::RelayHandle`] back to the `(URL, role)`
/// pair the actor tracks in `relay_controls`. Returns `None` for a stale
/// handle — the slot may have been reopened (different generation) or the
/// caller may have already shut down the worker for this URL. Stale events
/// are dropped silently; the pool's translator already filters out events
/// whose slot generation no longer matches, so this is belt-and-braces.
fn resolve_handle(
    h: nmp_network::pool::RelayHandle,
    rt: &RelayRuntime,
) -> Option<(&CanonicalRelayUrl, RelayRole)> {
    let url = rt.slot_to_url.get(&h.slot())?;
    let control = rt.relay_controls.get(url)?;
    if control.handle.generation() != h.generation() {
        return None;
    }
    Some((url, control.role))
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn handle_relay_event(
    event: PoolEvent,
    kernel: &mut Kernel,
    relay_text_interceptors: &[Arc<dyn crate::substrate::RelayTextInterceptor>],
    relay_connected_hooks: &[Arc<dyn crate::substrate::RelayConnectedHook>],
    command_tx_self: &crate::actor::CommandSender,
    relay_runtime: &mut RelayRuntime,
    pool: &Pool,
    update_tx: &Sender<crate::update_envelope::UpdateFrameBytes>,
    last_emit: &mut Instant,
    startup_sent: &mut bool,
    running: bool,
) {
    match event {
        // ── Opened ───────────────────────────────────────────────────────
        // Pool→kernel handshake for "socket dial completed". Carries the
        // URL (the only `PoolEvent` variant that does) plus the handle's
        // generation — we look up the role from `relay_controls` keyed by
        // the canonical URL the pool reports (already canonical, since
        // `ensure_relay_worker` only ever hands canonical strings in).
        PoolEvent::Opened { h, url, .. } => {
            let canonical = CanonicalRelayUrl::parse_or_raw(&url);
            let Some(control) = relay_runtime.relay_controls.get(&canonical) else {
                // No control row — stale event (worker spawned, then
                // RemoveRelay shut down the slot before `Opened` arrived).
                return;
            };
            if control.handle.generation() != h.generation() {
                return;
            }
            let role = control.role;
            // #1938: role readiness is derived from `connected_urls` — no
            // separate `connected_relays.insert(role)`. Marking this URL
            // connected below is the single readiness write.
            kernel.relay_connected_url(role, &url);
            // T116/G1 — reconnect-replay. The first `Opened` for a URL is
            // the initial dial; the startup path (`maybe_send_startup` /
            // `kernel.startup_requests()`) emits REQs there. Every
            // subsequent `Opened` after a `Failed`/`Closed` is a true
            // reconnect — the kernel's `wire_subs` for that URL were
            // evicted by `relay_closed` (T133), and the relay's
            // per-connection sub-id table is fresh, so we must re-emit
            // active sub-shapes. `kernel.replay_on_reconnect` consults
            // `SubscriptionLifecycle::handle_reconnect` (a pure read of
            // `current_plan`) and applies the T129 watermark per-shape so
            // `since` is bumped past already-stored events.
            //
            // D7 preserved: actor reports the OS-level transition; the
            // kernel decides what to replay and rewrites `since`.
            let is_reconnect = !relay_runtime.mark_url_connected(&canonical);
            for hook in relay_connected_hooks {
                let sender = command_tx_self.clone();
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    hook.on_relay_connected(canonical.as_str(), is_reconnect, sender);
                }));
            }
            if is_reconnect && running {
                let replay = kernel.replay_on_reconnect(role, &url);
                if !replay.is_empty() {
                    send_all_outbound(relay_runtime, pool, kernel, replay);
                }
            }
            if running {
                let publish_replay = kernel.mark_publish_relay_available(&url);
                if !publish_replay.is_empty() {
                    send_all_outbound(relay_runtime, pool, kernel, publish_replay);
                }
            }
            maybe_send_startup(
                running,
                startup_sent,
                relay_runtime,
                pool,
                kernel,
                Instant::now(),
            );
            emit_now(kernel, running, update_tx, last_emit);
        }
        // ── Failed ───────────────────────────────────────────────────────
        // Pool→kernel "socket dial / mid-session failed". The pool decides
        // whether this is permanent (HTTP 401/403 → no reconnect) or
        // transient (transport reset → it will retry with backoff). The
        // kernel observable is the per-URL `retrying` mark either way; the
        // permanent-vs-transient distinction surfaces via the next
        // `Opened` (transient) or absence thereof (permanent).
        PoolEvent::Failed { h, error, .. } => {
            let Some((url_ref, role)) = resolve_handle(h, relay_runtime) else {
                return;
            };
            let url_key = url_ref.clone();
            let url = url_key.as_str().to_string();
            // #1938: mark only THIS URL disconnected. Sibling URLs on the same
            // role stay in `connected_urls`, so the role stays derived-ready —
            // no `connected_relays.remove(&role)` dropping the whole lane.
            relay_runtime.mark_url_disconnected(&url_key);
            *startup_sent = false;
            // T105: scope the `retrying` mark to the specific socket that
            // failed — sibling sockets sharing this role lane are still live.
            kernel.relay_failed(role, &url, error.message);
            kernel.mark_publish_relay_unavailable(&url);
            emit_now(kernel, running, update_tx, last_emit);
        }
        // ── Closed ───────────────────────────────────────────────────────
        // Pool→kernel "socket torn down, no retry". Mirrors the legacy
        // `RelayEvent::Closed` arm one-to-one.
        PoolEvent::Closed { h, .. } => {
            let Some((url_ref, role)) = resolve_handle(h, relay_runtime) else {
                return;
            };
            let url_key = url_ref.clone();
            let url = url_key.as_str().to_string();
            // #1938: mark only THIS URL disconnected — sibling URLs on the same
            // role keep the role derived-ready.
            relay_runtime.mark_url_disconnected(&url_key);
            *startup_sent = false;
            // T105: scope T133 wire-sub eviction to the closed socket's URL,
            // not the whole role lane (sibling sockets keep their subs).
            kernel.relay_closed(role, &url);
            kernel.mark_publish_relay_unavailable(&url);
            emit_now(kernel, running, update_tx, last_emit);
        }
        // ── Frame ────────────────────────────────────────────────────────
        // Pool→kernel inbound wire frame. The pool's translator already
        // converted `tungstenite::Message → RelayFrame` (and pre-classified
        // NIP-42 AUTH frames into `RelayFrame::Auth` in phase E); we
        // round-trip the `Auth` variant back to a `Text` frame so the
        // kernel's existing ingest path handles AUTH unchanged.
        PoolEvent::Frame { h, frame, .. } if running => {
            let (url_str, role) = match resolve_handle(h, relay_runtime) {
                Some((url, role)) => (url.as_str().to_string(), role),
                None => return,
            };
            // V-38: peek at the text payload BEFORE kernel ingest so an
            // installed substrate-generic relay-text interceptor (today
            // `nmp-nip47`'s NWC runtime) can decode kind:23195 responses
            // the kernel itself drops as unknown kinds. The interceptor
            // filters by relay URL internally; uninteresting frames are a
            // single-lock no-op. D0: substrate-generic — no NIP-47 / NWC
            // nouns in nmp-core.
            let raw_text = match &frame {
                PoolFrame::Text(s) => Some(s.clone()),
                // Phase F: phase-E `RelayFrame::Auth` doesn't carry a
                // payload an interceptor would interpret; nothing to peek.
                _ => None,
            };
            let kernel_frame = pool_frame_to_relay_frame(frame);
            let now = Instant::now();
            let mut outbound = kernel.handle_message_at(role, &url_str, kernel_frame, now);
            outbound.extend(kernel.pending_view_requests_at(now));
            // V-58: drain any backoff hints the kernel enqueued during
            // `handle_message` (e.g. from a rate-limited CLOSED) and forward
            // each one to the pool worker. The hint is URL-keyed; we look up
            // the handle via `relay_controls` the same way every other per-URL
            // dispatch does. Stale or missing handles are silently ignored.
            for (hint_url, hint) in kernel.take_backoff_hints() {
                let canonical = CanonicalRelayUrl::parse_or_raw(&hint_url);
                if let Some(control) = relay_runtime.relay_controls.get(&canonical) {
                    let class = match hint {
                        BackoffHint::RateLimited => BackoffClass::RateLimited,
                    };
                    pool.set_backoff_hint(control.handle, class);
                }
            }
            if let Some(text) = raw_text {
                for interceptor in relay_text_interceptors {
                    let extra = interceptor.on_relay_text(kernel, &url_str, &text);
                    outbound.extend(extra);
                }
            }
            send_all_outbound(relay_runtime, pool, kernel, outbound);
        }
        PoolEvent::Frame { .. } => {}
        // ── Health ───────────────────────────────────────────────────────
        // Diagnostic snapshot; the kernel doesn't act on it (per-URL health
        // is M11). Reserved for future per-URL health-row writes.
        PoolEvent::Health { .. } => {}
    }
}
