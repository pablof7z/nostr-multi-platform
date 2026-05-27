//! Thread view open/close/hydration request builders.
//!
//! # M2 migration plan (compiler.md §3.5)
//! Per `docs/design/subscription-compilation/compiler.md` §3.5, these request
//! builders are scheduled for replacement by `SubscriptionCompiler`-driven
//! interest registration once the wire-emitter, `InterestRegistry`, and
//! trigger-based recompilation infrastructure land (M2 full migration):
//!
//! - `open_thread` → register Thread view-module spec; return interests with
//!   `event_ids` + #e-tag shapes
//! - `close_thread` → drop interests by `InterestId`; `recompile(Trigger::ViewClose)`
//! - `prepare_thread_requests` → moves to `nmp_nip01::ThreadView` /
//!   `nmp_nip01::Nip10ModularTimelineView` (the latter wrapping
//!   `nmp_threading::Grouper`); the hydration cascade becomes a view module
//!   emitting new interests as event ids surface.
//! - `enqueue_thread_id` → internal state of the chosen NIP-01 view module
//! - `enqueue_thread_reply_target` → internal state of the chosen NIP-01 view module
//! - `maybe_open_thread_hydration` → `reduce()` on the chosen NIP-01 view module
//!
//! The `close_subscriptions_with_prefixes` helper disappears: the wire-emitter
//! closes by `WireSubId` (compiler diff output), not string-prefix matching.

use super::super::{
    is_hex_id, json, referenced_event_ids, short_hex, Kernel, OutboundMessage, RelayRole,
    ViewInterest,
};
use crate::stable_hash::stable_hash64;

/// Deterministic 8-char tag over `relay_url` for thread hydration sub-ids.
///
/// Same URL → same suffix across runs, so wire-sub identity in the diagnostic
/// surface is stable and `close_subscriptions_with_prefixes` still matches
/// the `thread-ids-` / `thread-replies-` prefixes used in `close_thread`.
fn relay_short(relay_url: &str) -> String {
    format!(
        "{:08x}",
        stable_hash64(("thread-relay-short", relay_url)) & 0xFFFF_FFFF
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_short_is_restart_stable() {
        assert_eq!(relay_short("wss://relay.example"), "5ea311a8");
        assert_eq!(
            relay_short("wss://relay.example"),
            relay_short("wss://relay.example")
        );
        assert_ne!(
            relay_short("wss://relay.example"),
            relay_short("wss://other.example")
        );
    }
}

impl Kernel {
    pub(crate) fn open_thread(&mut self, event_id: String, can_send: bool) -> Vec<OutboundMessage> {
        match self.thread_view.selected_thread.as_mut() {
            Some(interest) if interest.key == event_id => {
                interest.refcount = interest.refcount.saturating_add(1);
            }
            _ => {
                self.thread_view.selected_thread = Some(ViewInterest {
                    key: event_id.clone(),
                    refcount: 1,
                });
                self.thread_view.pending_ids.clear();
                self.thread_view.requested_ids.clear();
                self.thread_view.pending_reply_targets.clear();
                self.thread_view.requested_reply_targets.clear();
            }
        }
        self.thread_view.request_pending = true;
        self.changed_since_emit = true;
        self.log(format!("open thread view {}", short_hex(&event_id)));

        if can_send {
            self.prepare_thread_requests()
        } else {
            self.log("thread request queued until relay connects");
            Vec::new()
        }
    }

    pub(crate) fn close_thread(&mut self, event_id: &str) -> Vec<OutboundMessage> {
        let Some(interest) = self.thread_view.selected_thread.as_mut() else {
            return Vec::new();
        };
        if interest.key != event_id {
            return Vec::new();
        }
        interest.refcount = interest.refcount.saturating_sub(1);
        if interest.refcount > 0 {
            self.changed_since_emit = true;
            return Vec::new();
        }

        self.thread_view.selected_thread = None;
        self.thread_view.request_pending = false;
        self.thread_view.pending_ids.clear();
        self.thread_view.pending_reply_targets.clear();
        self.thread_view.ids_inflight = false;
        self.thread_view.replies_inflight = false;
        self.changed_since_emit = true;
        self.log(format!("close thread view {}", short_hex(event_id)));
        self.close_subscriptions_with_prefixes(&["thread-ids-", "thread-replies-", "thread-more-"])
    }

    pub(crate) fn prepare_thread_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(focused_id) = self
            .thread_view
            .selected_thread
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            self.thread_view.request_pending = false;
            return Vec::new();
        };

        self.thread_view.request_pending = false;
        let root_id = self
            .thread_root_id(&focused_id)
            .unwrap_or_else(|| focused_id.clone());
        self.enqueue_thread_id(focused_id.clone());
        self.enqueue_thread_id(root_id.clone());
        self.enqueue_thread_reply_target(root_id);
        self.enqueue_thread_reply_target(focused_id.clone());
        if let Some(focused) = self.events.get(&focused_id) {
            for id in referenced_event_ids(focused) {
                self.enqueue_thread_id(id.clone());
                self.enqueue_thread_reply_target(id);
            }
        }
        self.maybe_open_thread_hydration()
    }

    pub(crate) fn enqueue_thread_id(&mut self, id: String) {
        if is_hex_id(&id) && !self.thread_view.requested_ids.contains(&id) {
            self.thread_view.pending_ids.insert(id);
        }
    }

    pub(crate) fn enqueue_thread_reply_target(&mut self, id: String) {
        if is_hex_id(&id)
            && self.thread_view.requested_reply_targets.len() < 96
            && !self.thread_view.requested_reply_targets.contains(&id)
        {
            self.thread_view.pending_reply_targets.insert(id);
        }
    }

    pub(crate) fn maybe_open_thread_hydration(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        if !self.thread_view.pending_ids.is_empty() && !self.thread_view.ids_inflight {
            let ids = self
                .thread_view
                .pending_ids
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for id in &ids {
                self.thread_view.pending_ids.remove(id);
                self.thread_view.requested_ids.insert(id.clone());
            }
            self.thread_view.seq = self.thread_view.seq.saturating_add(1);
            self.thread_view.ids_inflight = true;
            // T121 / codex R1: partition the id set by each id's
            // original-event author's NIP-65 write relays via the kernel's
            // `outbox_router`. Ids whose authors aren't yet in the local
            // store fall through to the cold-start bootstrap discovery
            // seed (the router's lane 7 / AppRelay fallback). Mirrors
            // T105's outbox partition pattern.
            let partition = self.partition_ids_via_router(&ids);
            let seq = self.thread_view.seq;
            for (relay_url, served_ids) in partition {
                let sub_id = format!("thread-ids-{}-{}", seq, relay_short(&relay_url));
                requests.push(self.req_for_relay(
                    RelayRole::Content,
                    relay_url,
                    &sub_id,
                    "thread context ids (NIP-65 outbox)",
                    json!({"ids":served_ids,"limit":20}),
                ));
            }
        }

        if !self.thread_view.pending_reply_targets.is_empty() && !self.thread_view.replies_inflight
        {
            let ids = self
                .thread_view
                .pending_reply_targets
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for id in &ids {
                self.thread_view.pending_reply_targets.remove(id);
                self.thread_view.requested_reply_targets.insert(id.clone());
            }
            self.thread_view.seq = self.thread_view.seq.saturating_add(1);
            self.thread_view.replies_inflight = true;
            // T121 / codex R1: route the `#e` recursive-replies REQ to the
            // root event author's resolved write relays (per-id partition)
            // via the kernel's `outbox_router`. Reply authors write to
            // their own relays of course; routing to the root author's
            // relays is the deliberate compromise — the root's relays
            // usually carry the thread context rather than fanning to
            // every participant. Unknown-author ids fall back to the
            // bootstrap discovery seed via the router's lane 7.
            let partition = self.partition_ids_via_router(&ids);
            let seq = self.thread_view.seq;
            for (relay_url, served_ids) in partition {
                let sub_id = format!("thread-replies-{}-{}", seq, relay_short(&relay_url));
                requests.push(self.req_for_relay(
                    RelayRole::Content,
                    relay_url,
                    &sub_id,
                    "thread recursive replies (NIP-65 outbox)",
                    json!({"kinds":[1,6],"#e":served_ids,"limit":200}),
                ));
            }
        }

        requests
    }
}
