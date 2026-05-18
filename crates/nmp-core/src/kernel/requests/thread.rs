//! Thread view open/close/hydration request builders.
//!
//! # M2 migration plan (compiler.md §3.5)
//! Per `docs/design/subscription-compilation/compiler.md` §3.5, these request
//! builders are scheduled for replacement by `SubscriptionCompiler`-driven
//! interest registration once the wire-emitter, InterestRegistry, and
//! trigger-based recompilation infrastructure land (M2 full migration):
//!
//! - `open_thread` → register Thread view-module spec; return interests with
//!   event_ids + #e-tag shapes
//! - `close_thread` → drop interests by InterestId; recompile(Trigger::ViewClose)
//! - `prepare_thread_requests` → moves to ThreadViewModule.reduce() in nmp-nip10;
//!   hydration cascade becomes view module emitting new interests as event ids surface
//! - `enqueue_thread_id` → ThreadViewModule internal state
//! - `enqueue_thread_reply_target` → ThreadViewModule internal state
//! - `maybe_open_thread_hydration` → ThreadViewModule.reduce() returning interests
//!
//! The `close_subscriptions_with_prefixes` helper disappears: the wire-emitter
//! closes by WireSubId (compiler diff output), not string-prefix matching.

use super::super::*;

impl Kernel {
    pub(crate) fn open_thread(
        &mut self,
        event_id: String,
        can_send: bool,
    ) -> Vec<OutboundMessage> {
        match self.selected_thread.as_mut() {
            Some(interest) if interest.key == event_id => {
                interest.refcount = interest.refcount.saturating_add(1);
            }
            _ => {
                self.selected_thread = Some(ViewInterest {
                    key: event_id.clone(),
                    refcount: 1,
                });
                self.pending_thread_ids.clear();
                self.requested_thread_ids.clear();
                self.pending_thread_reply_targets.clear();
                self.requested_thread_reply_targets.clear();
            }
        }
        self.thread_request_pending = true;
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
        let Some(interest) = self.selected_thread.as_mut() else {
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

        self.selected_thread = None;
        self.thread_request_pending = false;
        self.pending_thread_ids.clear();
        self.pending_thread_reply_targets.clear();
        self.thread_ids_inflight = false;
        self.thread_replies_inflight = false;
        self.changed_since_emit = true;
        self.log(format!("close thread view {}", short_hex(event_id)));
        self.close_subscriptions_with_prefixes(&["thread-ids-", "thread-replies-", "thread-more-"])
    }

    pub(crate) fn prepare_thread_requests(&mut self) -> Vec<OutboundMessage> {
        let Some(focused_id) = self
            .selected_thread
            .as_ref()
            .map(|interest| interest.key.clone())
        else {
            self.thread_request_pending = false;
            return Vec::new();
        };

        self.thread_request_pending = false;
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
        if is_hex_id(&id) && !self.requested_thread_ids.contains(&id) {
            self.pending_thread_ids.insert(id);
        }
    }

    pub(crate) fn enqueue_thread_reply_target(&mut self, id: String) {
        if is_hex_id(&id)
            && self.requested_thread_reply_targets.len() < 96
            && !self.requested_thread_reply_targets.contains(&id)
        {
            self.pending_thread_reply_targets.insert(id);
        }
    }

    pub(crate) fn maybe_open_thread_hydration(&mut self) -> Vec<OutboundMessage> {
        let mut requests = Vec::new();
        if !self.pending_thread_ids.is_empty() && !self.thread_ids_inflight {
            let ids = self
                .pending_thread_ids
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for id in &ids {
                self.pending_thread_ids.remove(id);
                self.requested_thread_ids.insert(id.clone());
            }
            self.thread_view_seq = self.thread_view_seq.saturating_add(1);
            self.thread_ids_inflight = true;
            requests.push(self.req(
                RelayRole::Content,
                &format!("thread-ids-{}", self.thread_view_seq),
                "thread context ids",
                json!({"ids":ids,"limit":20}),
            ));
        }

        if !self.pending_thread_reply_targets.is_empty() && !self.thread_replies_inflight {
            let ids = self
                .pending_thread_reply_targets
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>();
            for id in &ids {
                self.pending_thread_reply_targets.remove(id);
                self.requested_thread_reply_targets.insert(id.clone());
            }
            self.thread_view_seq = self.thread_view_seq.saturating_add(1);
            self.thread_replies_inflight = true;
            requests.push(self.req(
                RelayRole::Content,
                &format!("thread-replies-{}", self.thread_view_seq),
                "thread recursive replies",
                json!({"kinds":[1,6],"#e":ids,"limit":200}),
            ));
        }

        requests
    }
}
