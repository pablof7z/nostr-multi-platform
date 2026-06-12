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
use super::{truncate, Kernel, OutboxSummarySnapshot, PublishOutboxItem, PublishOutboxRelay};

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
                let status_label = publish_outbox_status_label(&status);
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
                PublishOutboxItem {
                    handle: row.handle,
                    event_id: row.event_id,
                    kind: row.kind,
                    title: publish_event_title(row.kind),
                    preview: publish_event_preview(row.kind, &row.content),
                    created_at: row.created_at,
                    status,
                    status_label,
                    system_image: publish_event_system_image(row.kind),
                    can_retry,
                    target_relays: relays.len(),
                    relays,
                }
            })
            .collect()
    }

    /// Pre-formatted summary of the publish-outbox state for shells that
    /// render an "N pending" header. The kernel owns the counters AND the
    /// English strings; shells render `title` / `subtitle` verbatim (no
    /// `.filter`/`.count` chains, no ternary status strings — RMP bible
    /// §6 anti-pattern #1).
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
            title: outbox_summary_title(total),
            subtitle: outbox_summary_subtitle(total, sending, retrying, queued, failed),
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
            return Vec::new();
        }
        self.apply_engine_completions();
        let drained = self.publish_dispatcher.drain();
        if !drained.is_empty() {
            self.changed_since_emit = true;
        }
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
        if let Err(err) = self.publish_engine.start_publish(action, now_ms, None) {
            if matches!(&err, PublishEngineError::Store(PublishStoreError::NotFound))
                && self.remove_publish_entry(&handle)
            {
                self.set_last_error_toast(None);
                return;
            }
            self.publish_engine
                .record_engine_error(&err, &handle, "", now_ms);
            let (toast, _, _) = describe_engine_error(&err);
            self.set_last_error_toast(Some(toast));
            return;
        }
        self.set_publish_entry_terminal(&handle, "cancelled", Vec::new());
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
        status_label: publish_outbox_relay_status_label(status),
        attempt,
        attempt_label: publish_outbox_attempt_label(attempt),
        message,
        relay_reason: relay_reason.to_string(),
    }
}

/// English label for a relay-level status key. Mirrors the closed key set in
/// `publish_outbox_relay`; the shell renders this verbatim (no Swift-side
/// `.capitalized` or switch deciding text).
fn publish_outbox_relay_status_label(status: &str) -> String {
    match status {
        "sending" => "Sending",
        "retrying" => "Retrying",
        "pending" => "Pending",
        "ok" => "Ok",
        "failed" => "Failed",
        // Defensive fallback — surface the raw key rather than panic at the
        // FFI boundary if the closed set ever grows without a label update.
        other => other,
    }
    .to_string()
}

/// English badge for a relay attempt counter. Empty when the relay has not
/// retried yet so the shell renders unconditionally without an `if attempt >
/// 0` branch (D1: best-effort rendering — placeholders are server-side).
fn publish_outbox_attempt_label(attempt: u32) -> String {
    if attempt == 0 {
        String::new()
    } else {
        format!("try {attempt}")
    }
}

/// Row-level status label for `PublishOutboxItem.status_label`. Mirrors the
/// closed set produced by `publish_outbox_status`; the shell binds this string
/// directly into the status badge (no Swift-side switch on `status`).
fn publish_outbox_status_label(status: &str) -> String {
    match status {
        "sending" => "Sending",
        "retrying" => "Retrying",
        "pending" => "Pending",
        "failed" => "Failed",
        "queued" => "Queued",
        other => other,
    }
    .to_string()
}

/// Pre-formatted outbox-summary title. Empty outbox → `"Nothing waiting"`;
/// otherwise an "N pending publish(es)" headline with server-side pluralization.
fn outbox_summary_title(total: u32) -> String {
    if total == 0 {
        return "Nothing waiting".to_string();
    }
    let suffix = if total == 1 { "" } else { "es" };
    format!("{total} pending publish{suffix}")
}

/// Pre-formatted outbox-summary subtitle. Decomposes per-status counts into a
/// single English sentence (mirrors the old Swift ternary tree at lines 87–97
/// of `NotificationsView.swift`).
fn outbox_summary_subtitle(
    total: u32,
    sending: u32,
    retrying: u32,
    queued: u32,
    failed: u32,
) -> String {
    if total == 0 {
        return "Your local outbox is clear.".to_string();
    }
    if retrying > 0 {
        return format!("{retrying} waiting to retry, {sending} currently sending.");
    }
    if sending > 0 {
        return format!("{sending} currently sending.");
    }
    if failed > 0 {
        return format!("{failed} failed.");
    }
    // `queued` covers both `pending` (waiting for a relay socket) and any
    // genuinely-queued rows — same UI bucket per `outbox_summary_snapshot`.
    let _ = queued;
    "Waiting for relay connections.".to_string()
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

pub(super) fn publish_event_title(kind: u32) -> String {
    use crate::kinds::{
        KIND_CONTACT_LIST, KIND_PROFILE_METADATA, KIND_REACTION, KIND_RELAY_LIST,
        KIND_SHORT_TEXT_NOTE,
    };
    match kind {
        k if k == KIND_PROFILE_METADATA => "Profile",
        k if k == KIND_SHORT_TEXT_NOTE => "Note",
        k if k == KIND_CONTACT_LIST => "Contacts",
        k if k == KIND_REACTION => "Reaction",
        k if k == KIND_RELAY_LIST => "Relay list",
        _ => "Event",
    }
    .to_string()
}

/// SF Symbol name for the outbox row icon, pre-classified from the Nostr event
/// kind. Mirrors the closed match in `publish_event_title` so the shell renders
/// `system_image` verbatim — no Swift-side `switch item.kind` branching on a
/// protocol concept (aim.md §4.4 / §6 anti-pattern). Default `"doc.text"`
/// keeps the shell rendering unconditionally for any future kind.
fn publish_event_system_image(kind: u32) -> String {
    use crate::kinds::{
        KIND_CONTACT_LIST, KIND_PROFILE_METADATA, KIND_REACTION, KIND_RELAY_LIST,
        KIND_SHORT_TEXT_NOTE,
    };
    match kind {
        k if k == KIND_PROFILE_METADATA => "person.crop.circle",
        k if k == KIND_SHORT_TEXT_NOTE => "text.bubble",
        k if k == KIND_CONTACT_LIST => "person.2",
        k if k == KIND_REACTION => "heart",
        k if k == KIND_RELAY_LIST => "antenna.radiowaves.left.and.right",
        _ => "doc.text",
    }
    .to_string()
}

fn publish_event_preview(kind: u32, content: &str) -> String {
    use crate::kinds::{
        KIND_CONTACT_LIST, KIND_GIFT_WRAP, KIND_PROFILE_METADATA, KIND_REACTION, KIND_RELAY_LIST,
    };
    // Legacy NIP-04 DM kind (kind:4) and the historical NIP-44-versioned DM
    // kind (kind:44) are still emitted by other clients; we treat their
    // content as encrypted opaque bytes alongside the canonical NIP-59
    // gift-wrap envelope. The integers are local literals because
    // `nmp_core::kinds` only mints constants for kinds the workspace
    // actively WRITES — kind:4 / kind:44 are read-side legacy only.
    const KIND_LEGACY_DM: u32 = 4;
    const KIND_LEGACY_VERSIONED_DM: u32 = 44;
    match kind {
        k if k == KIND_PROFILE_METADATA => "Profile metadata update".to_string(),
        k if k == KIND_CONTACT_LIST => "Contact list update".to_string(),
        k if k == KIND_REACTION && content.trim().is_empty() => "Reaction event".to_string(),
        k if k == KIND_RELAY_LIST => "Relay list metadata".to_string(),
        k if k == KIND_LEGACY_DM || k == KIND_LEGACY_VERSIONED_DM || k == KIND_GIFT_WRAP => {
            "Encrypted event content hidden".to_string()
        }
        _ => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                "Event with no text content".to_string()
            } else {
                truncate(trimmed, 180)
            }
        }
    }
}
