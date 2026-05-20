//! User-facing publish outbox projection and commands.
//!
//! The publish engine owns retry policy and durable per-relay state. This
//! module only projects that state into a compact UI shape and exposes
//! user-triggered retry/cancel commands back through the engine.

use crate::publish::{PerRelayState, PublishAction};
use crate::relay::{OutboundMessage, RelayRole};

use super::publish_engine_wire::{describe_engine_error, now_epoch_ms};
use super::*;

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
                let relays = row
                    .per_relay
                    .iter()
                    .map(|(url, state)| publish_outbox_relay(url, state))
                    .collect::<Vec<_>>();
                let status = publish_outbox_status(&row.per_relay);
                PublishOutboxItem {
                    handle: row.handle,
                    event_id: row.event_id,
                    kind: row.kind,
                    title: publish_event_title(row.kind),
                    preview: publish_event_preview(row.kind, &row.content),
                    created_at_display: format_timestamp(row.created_at),
                    status,
                    target_relays: relays.len(),
                    relays,
                }
            })
            .collect()
    }

    pub(crate) fn retry_publish_now(&mut self, handle: &str) -> Vec<OutboundMessage> {
        let now_ms = now_epoch_ms();
        let handle = handle.to_string();
        if let Err(err) = self.publish_engine.retry_now(&handle, now_ms) {
            self.publish_engine
                .record_engine_error(&err, &handle, "", now_ms);
            let (toast, _) = describe_engine_error(&err);
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
        let action = PublishAction::Cancel {
            handle: handle.clone(),
        };
        if let Err(err) = self.publish_engine.start_publish(action, now_ms) {
            self.publish_engine
                .record_engine_error(&err, &handle, "", now_ms);
            let (toast, _) = describe_engine_error(&err);
            self.set_last_error_toast(Some(toast));
            return;
        }
        self.set_publish_entry_terminal(&handle, "cancelled", Vec::new());
        self.changed_since_emit = true;
    }
}

fn publish_outbox_relay(relay_url: &str, state: &PerRelayState) -> PublishOutboxRelay {
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

fn publish_event_title(kind: u32) -> String {
    match kind {
        0 => "Profile",
        1 => "Note",
        3 => "Contacts",
        7 => "Reaction",
        10002 => "Relay list",
        _ => "Event",
    }
    .to_string()
}

fn publish_event_preview(kind: u32, content: &str) -> String {
    match kind {
        0 => "Profile metadata update".to_string(),
        3 => "Contact list update".to_string(),
        7 if content.trim().is_empty() => "Reaction event".to_string(),
        10002 => "Relay list metadata".to_string(),
        4 | 44 | 1059 => "Encrypted event content hidden".to_string(),
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
