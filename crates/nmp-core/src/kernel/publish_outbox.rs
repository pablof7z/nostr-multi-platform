//! User-facing publish outbox projection and commands.
//!
//! The publish engine owns retry policy and durable per-relay state. This
//! module only projects that state into a compact UI shape and exposes
//! user-triggered retry/cancel commands back through the engine.

use crate::publish::{
    PerRelayState, PublishAction, PublishEngineError, PublishStoreError, RelaySelectionReason,
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

    pub(crate) fn cancel_publish(&mut self, handle: &str) {
        let now_ms = now_epoch_ms();
        let handle = handle.to_string();
        if self.publish_engine.per_relay(&handle).is_empty() && self.remove_publish_entry(&handle) {
            self.set_last_error_toast(None);
            return;
        }
        let action = PublishAction::Cancel {
            handle: handle.clone(),
        };
        // Cancel reports `handle` as the correlation_id directly (it is what
        // the host received from dispatch), so no override is needed here.
        let engine_rev_before = self.publish_engine.snapshot().rev;
        if let Err(err) = self.publish_engine.start_publish(action, now_ms, None) {
            if matches!(&err, PublishEngineError::Store(PublishStoreError::NotFound))
                && self.remove_publish_entry(&handle)
            {
                self.set_last_error_toast(None);
                self.bump_publish_if_engine_view_changed(engine_rev_before);
                return;
            }
            self.publish_engine
                .record_engine_error(&err, &handle, "", now_ms);
            let (toast, _, _) = describe_engine_error(&err);
            self.set_last_error_toast(Some(toast));
            self.bump_publish_if_engine_view_changed(engine_rev_before);
            return;
        }
        self.set_publish_entry_terminal(&handle, "cancelled", Vec::new());
        self.bump_publish_if_engine_view_changed(engine_rev_before);
        self.changed_since_emit = true;
    }
}

/// Format a single structured selection reason into the human-readable string
/// the shell renders verbatim. This is the **only** place in the codebase
/// where `RelaySelectionReason` becomes English — the resolver, the engine,
/// the view, and persistence all carry the typed enum. Apps that need a
/// different wording must change this function (and nothing else).
pub(super) fn format_relay_reason(reason: &RelaySelectionReason) -> String {
    match reason {
        RelaySelectionReason::AuthorWriteRelay => "NIP-65 write relay".to_string(),
        RelaySelectionReason::LocalConfigRelay => "App relay (local config)".to_string(),
        RelaySelectionReason::DiscoveryIndexer { kind } => {
            format!("Discovery indexer (kind {kind})")
        }
        RelaySelectionReason::RecipientInbox { pubkey } => {
            // D6 — backend projections carry raw identifiers across the wire
            // boundary; the shell/display layer abbreviates (`short_npub`,
            // bech32 encoding, etc.) according to its own UX rules. The raw
            // hex pubkey is emitted verbatim here.
            format!("Inbox relay for {pubkey}")
        }
        RelaySelectionReason::Explicit => "Explicit relay".to_string(),
    }
}

/// Format the per-relay reason list. Joins distinct reasons with `"; "` —
/// the wire-shape contract `PublishOutboxRelay.relay_reason` callers parse.
/// Empty input → empty string (the projection's `skip_serializing_if` then
/// drops the field).
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
        PerRelayState::Pending => ("pending", 0, "Waiting for relay connection".to_string()),
        PerRelayState::InFlight { attempt, .. } => {
            ("sending", *attempt, "Waiting for relay OK".to_string())
        }
        PerRelayState::Ok { .. } => ("ok", 0, "Relay accepted the event".to_string()),
        PerRelayState::RelayError {
            message, attempt, ..
        } => ("retrying", *attempt, message.clone()),
        PerRelayState::TimedOut { attempt, .. } => {
            ("retrying", *attempt, "No response from relay".to_string())
        }
        PerRelayState::FailedAfterRetries { reason, .. } => ("failed", 0, reason.clone()),
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

