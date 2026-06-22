//! User-facing publish outbox projection and commands.
//!
//! The publish engine owns retry policy and durable per-relay state. This
//! module only projects that state into a compact UI shape and exposes
//! user-triggered retry/cancel commands back through the engine.

use crate::publish::{
    PerRelayState, PublishEngineError, PublishStoreError, RelaySelectionReason,
};
use crate::relay::{OutboundMessage, RelayRole};

use super::publish_engine_wire::{describe_engine_error, now_epoch_ms};
use super::{Kernel, OutboxSummarySnapshot, PublishOutboxItem, PublishOutboxRelay};

impl Kernel {
    pub(super) fn publish_outbox_items(&self) -> Vec<PublishOutboxItem> {
        let mut rows = self.publish_engine.snapshot().in_flight.clone();
        rows.sort_by(|left, right| {
            right
                .created_at
                .cmp(&left.created_at)
                .then_with(|| left.event_id.cmp(&right.event_id))
        });
        rows.into_iter()
            .map(|row| {
                // Build a quick canonical-URL → reasons lookup so the per_relay
                // iteration stays O(n + m) instead of O(n*m). Keys match the
                // canonicalization already applied by the engine, so a direct
                // `.get()` against `url.as_str()` is sufficient.
                let relay_reasons_map: std::collections::HashMap<&str, &Vec<RelaySelectionReason>> =
                    row.relay_reasons
                        .iter()
                        .map(|(k, v)| (k.as_str(), v))
                        .collect();
                let relays = row
                    .per_relay
                    .iter()
                    .map(|(url, state)| {
                        let reason = relay_reasons_map
                            .get(url.as_str())
                            .map(|reasons| format_relay_reasons(reasons))
                            .unwrap_or_default();
                        publish_outbox_relay(url, state, &reason)
                    })
                    .collect::<Vec<_>>();
                let status = publish_outbox_status(&row.per_relay);
                // RMP bible commandment #4 — retry policy lives in Rust. The
                // shell renders `can_retry` directly instead of branching on
                // `status != "sending"` to decide whether to enable a button.
                let can_retry = status != "sending";
                // ADR-0032 / V-115: emit raw Unix-seconds `created_at` so
                // shells can format the timestamp in their own locale + TZ.
                // `format_timestamp` (chrono::Local, OS wall clock) and
                // `publish_outbox_target_summary` ("N relays · <time>") are
                // removed; shells compose the relay-count + time label
                // themselves from `target_relays` + `created_at`.
                // ADR-0032 / aim.md §2 #4: `title`, `preview`, `system_image`,
                // `status_label` pre-formatted strings removed — shells own all
                // presentation formatting. Raw `content` is emitted so shells
                // can render a preview appropriate to their UX.
                PublishOutboxItem {
                    handle: row.handle,
                    event_id: row.event_id,
                    kind: row.kind,
                    content: row.content.clone(),
                    created_at: row.created_at,
                    status,
                    can_retry,
                    target_relays: relays.len(),
                    relays,
                }
            })
            .collect()
    }

    /// Raw per-status counters for the publish-outbox summary header.
    /// ADR-0032 / aim.md §2 #4: the previously-emitted pre-formatted English
    /// `title` / `subtitle` strings are removed; shells derive display strings
    /// from these raw counts using their own locale/formatting rules.
    pub(super) fn outbox_summary_snapshot(&self) -> OutboxSummarySnapshot {
        let rows = &self.publish_engine.snapshot().in_flight;
        let mut sending: u32 = 0;
        let mut retrying: u32 = 0;
        let mut queued: u32 = 0;
        let mut failed: u32 = 0;
        for row in rows {
            match publish_outbox_status(&row.per_relay).as_str() {
                "sending" => sending = sending.saturating_add(1),
                "retrying" => retrying = retrying.saturating_add(1),
                "failed" => failed = failed.saturating_add(1),
                // `pending` (waiting for a relay socket) and the catch-all
                // `queued` are both surfaced under the same UI bucket: the
                // user can't act on either.
                _ => queued = queued.saturating_add(1),
            }
        }
        let total = sending
            .saturating_add(retrying)
            .saturating_add(queued)
            .saturating_add(failed);
        OutboxSummarySnapshot {
            total,
            sending,
            retrying,
            queued,
            failed,
        }
    }

    pub(crate) fn retry_publish_now(&mut self, handle: &str) -> Vec<OutboundMessage> {
        let now_ms = now_epoch_ms();
        let handle = handle.to_string();
        let engine_rev_before = self.publish_engine.snapshot().rev;
        if let Err(err) = self.publish_engine.retry_now(&handle, now_ms) {
            if matches!(&err, PublishEngineError::Store(PublishStoreError::NotFound)) {
                if let Some((signed, target)) = self.retry_payload_for_publish(&handle) {
                    self.remove_publish_entry(&handle);
                    return self.run_publish_engine_at(&signed, &[], target, None, now_ms);
                }
            }
            self.publish_engine
                .record_engine_error(&err, &handle, "", now_ms);
            let (toast, _, _) = describe_engine_error(&err);
            self.set_last_error_toast(Some(toast));
            self.bump_publish_if_engine_view_changed(engine_rev_before);
            return Vec::new();
        }
        self.apply_engine_completions();
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        drained
            .into_iter()
            .map(|(relay_url, text)| OutboundMessage {
                role: RelayRole::Content,
                relay_url,
                text,
            })
            .collect()
    }

    /// Cancel an in-flight publish addressed by `id` — the original dispatch
    /// `correlation_id` (the host's spinner key) OR the raw publish handle
    /// (== event id), the two forms the durable handle↔correlation index
    /// resolves between (S7, #1754).
    ///
    /// This is the ONE cancel doorway. The bespoke `nmp_app_cancel_publish` C
    /// symbol and `PublishAction::Cancel` lane are deleted; the FFI and the wasm
    /// boundary route cancel here by `correlation_id`. The `Cancelled` terminal
    /// is recorded under the ORIGINAL `correlation_id`, never the handle — the
    /// PD-036 fix. An unknown `id` falls back to treating it as both the handle
    /// and the correlation_id (the prior cancel-by-handle behaviour for an
    /// already-settled or never-indexed publish), so a stale host cancel is
    /// still a benign idempotent terminal verdict (D6).
    pub(crate) fn cancel_publish(&mut self, id: &str) {
        let now_ms = now_epoch_ms();
        // Reverse-resolve `id` → (handle, original correlation_id). Unknown id:
        // fall back to id-as-both so an evicted/never-indexed publish still
        // clears the host spinner under the id the host handed us. The resolved
        // `correlation_id` is the ORIGINAL dispatch id; the engine records the
        // `Cancelled` terminal under it (NOT the handle) — the single PD-036 fix.
        let (handle, correlation_id) = self
            .publish_handle_correlation
            .resolve(id)
            .unwrap_or_else(|| (id.to_string(), id.to_string()));
        // Settled-row "clear" path: the publish already terminated (nothing in
        // the engine's in-flight set) and the queue still carries a finished
        // row. Cancelling here is a host-side CLEAR of that finished row, not an
        // in-flight cancellation — drop the row and the index entry, no new
        // terminal (the publish already settled with its real verdict).
        if self.publish_engine.per_relay(&handle).is_empty() && self.remove_publish_entry(&handle) {
            self.set_last_error_toast(None);
            self.publish_handle_correlation.forget(&handle);
            return;
        }
        let engine_rev_before = self.publish_engine.snapshot().rev;
        // `cancel_by_handle` is the SINGLE cancel terminal source and is
        // INFALLIBLE (D6): it removes any in-flight engine row AND ALWAYS records
        // the `Cancelled` terminal under `correlation_id` BEFORE the best-effort
        // durable delete — so a store-delete failure can never orphan the host
        // spinner. The terminal drains through `take_pending_terminals` →
        // `drain_action_results`, which mirrors it into `action_stages` /
        // `action_lifecycle` as the distinct `cancelled` stage. No parallel
        // `record_action_stage` here — one path only.
        self.publish_engine
            .cancel_by_handle(&handle, &correlation_id, now_ms);
        self.set_publish_entry_terminal(&handle, "cancelled", Vec::new());
        self.publish_handle_correlation.forget(&handle);
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        self.changed_since_emit = true;
    }
}

/// Emit a stable machine token for one relay selection reason.
///
/// The returned string is a raw token the shell parses and localises —
/// **not** English prose. This is the **only** place in the codebase where
/// `RelaySelectionReason` becomes a wire token; the resolver, engine, view,
/// and persistence all carry the typed enum. Shells format these tokens into
/// the appropriate display language.
///
/// Token grammar:
/// - Simple: `"nip65_write"`, `"local_config"`, `"explicit"`
/// - Parameterised: `"discovery_indexer:{kind}"`, `"recipient_inbox:{pubkey}"`
pub(super) fn format_relay_reason(reason: &RelaySelectionReason) -> String {
    match reason {
        RelaySelectionReason::AuthorWriteRelay => "nip65_write".to_string(),
        RelaySelectionReason::LocalConfigRelay => "local_config".to_string(),
        RelaySelectionReason::DiscoveryIndexer { kind } => {
            format!("discovery_indexer:{kind}")
        }
        RelaySelectionReason::RecipientInbox { pubkey } => {
            // D6 — backend projections carry raw identifiers across the wire
            // boundary; the shell/display layer abbreviates (`short_npub`,
            // bech32 encoding, etc.) according to its own UX rules. The raw
            // hex pubkey is emitted verbatim here.
            format!("recipient_inbox:{pubkey}")
        }
        RelaySelectionReason::Explicit => "explicit".to_string(),
    }
}

/// Emit the per-relay reason token list. Joins distinct reason tokens with
/// `"; "` — the wire-shape contract `PublishOutboxRelay.relay_reason` callers
/// parse. Empty input → empty string (the projection's `skip_serializing_if`
/// then drops the field).
pub(super) fn format_relay_reasons(reasons: &[RelaySelectionReason]) -> String {
    reasons
        .iter()
        .map(format_relay_reason)
        .collect::<Vec<_>>()
        .join("; ")
}

fn publish_outbox_relay(
    relay_url: &str,
    state: &PerRelayState,
    relay_reason: &str,
) -> PublishOutboxRelay {
    let (status, attempt, message) = match state {
        // Raw machine tokens — shells localise these to display strings.
        PerRelayState::Pending => ("pending", 0, "waiting_for_connection".to_string()),
        PerRelayState::InFlight { attempt, .. } => {
            ("sending", *attempt, "waiting_for_ok".to_string())
        }
        PerRelayState::Ok { .. } => ("ok", 0, "accepted".to_string()),
        PerRelayState::RelayError {
            message, attempt, ..
        } => ("retrying", *attempt, message.clone()), // raw relay protocol error — pass through
        PerRelayState::TimedOut { attempt, .. } => {
            ("retrying", *attempt, "timed_out".to_string())
        }
        PerRelayState::FailedAfterRetries { reason, .. } => ("failed", 0, reason.clone()), // raw relay protocol error — pass through
    };
    PublishOutboxRelay {
        relay_url: relay_url.to_string(),
        status: status.to_string(),
        attempt,
        message,
        relay_reason: relay_reason.to_string(),
    }
}

fn publish_outbox_status(per_relay: &[(String, PerRelayState)]) -> String {
    if per_relay.iter().any(|(_, state)| {
        matches!(
            state,
            PerRelayState::RelayError { .. } | PerRelayState::TimedOut { .. }
        )
    }) {
        return "retrying".to_string();
    }
    if per_relay
        .iter()
        .any(|(_, state)| matches!(state, PerRelayState::InFlight { .. }))
    {
        return "sending".to_string();
    }
    // At least one relay already accepted: the event is published. Remaining
    // Pending entries are secondary fanout relays still waiting for a
    // connection — surface as "queued" so the user isn't misled into thinking
    // the publish failed.
    if per_relay
        .iter()
        .any(|(_, state)| matches!(state, PerRelayState::Ok { .. }))
    {
        return "queued".to_string();
    }
    if per_relay
        .iter()
        .any(|(_, state)| matches!(state, PerRelayState::Pending))
    {
        return "pending".to_string();
    }
    if per_relay
        .iter()
        .any(|(_, state)| matches!(state, PerRelayState::FailedAfterRetries { .. }))
    {
        return "failed".to_string();
    }
    "queued".to_string()
}

