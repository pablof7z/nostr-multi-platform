//! Kernel-side publish dispatch — T117 thin shim over `PublishEngine`.
//!
//! Before T117 this file contained a one-shot publish path: resolve NIP-65
//! relays, emit a single `EVENT` frame on `RelayRole::Content`, stamp
//! `accepted_locally`, and forget. The publish-retry FSM
//! (`crate::publish::state`) was dead code (relay-lifecycle review §G5).
//!
//! T117 deletes that pathway and routes every publish through
//! [`Kernel::run_publish_engine`] (`kernel/publish_engine.rs`). The engine:
//!
//! 1. Resolves NIP-65 outbox relays (D3).
//! 2. Drives the per-(event, relay) state machine and pushes per-relay frames
//!    into the kernel's `QueueDispatcher`.
//! 3. Surfaces ack handling, retry policy, AUTH-REQUIRED reauth, and durable
//!    `pending_retries` across kernel restart.
//! 4. Folds inbound `OK` frames back through `Kernel::handle_publish_ok` —
//!    the engine is the single writer of publish state (D4).
//!
//! This file remains the kernel's public `publish_signed` entrypoint so
//! `actor/commands/publish.rs` stays untouched.

use super::*;
use crate::publish::PublishTarget;
use crate::substrate::SignedEvent;

impl Kernel {
    /// Publish a signed event through the publish engine (T117).
    ///
    /// Returns the outbound frames the kernel must send: one per resolved
    /// outbox relay (D3). When the resolver returns no targets the engine
    /// records a `RecentFailure` row and the kernel surfaces a toast (D6) —
    /// the return is `Vec::new()`. The retry / ack / reauth lifecycle is
    /// owned entirely by the engine; the kernel only feeds OK frames in via
    /// `handle_publish_ok` (called from `kernel::ingest::handle_text`).
    pub(crate) fn publish_signed(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, PublishTarget::Auto, None)
    }

    /// [`Kernel::publish_signed`] with an action `correlation_id` to report in
    /// `last_action_result`. The `PublishNote` dispatch path uses this: the
    /// host received a registry-minted correlation_id before the actor signed
    /// the event, so the publish engine must report that id (not the signed
    /// event's `id`) for the host spinner to be cleared. Every other publish
    /// path (`react`, `follow`, `publish_unsigned_event`, …) uses the plain
    /// [`Kernel::publish_signed`], which reports the event id.
    pub(crate) fn publish_signed_with_correlation(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        correlation_id_override: Option<String>,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, PublishTarget::Auto, correlation_id_override)
    }

    /// Publish a signed event to an EXPLICIT relay set — the named D3 opt-out
    /// (`PublishTarget::Explicit`). The verbatim event is routed to exactly
    /// `target`'s relays, bypassing the NIP-65 outbox resolver; everything
    /// else (retry / ack / reauth lifecycle, D6 toast contract) is identical
    /// to [`Kernel::publish_signed`]. `PublishTarget::Auto` callers reach the
    /// resolver unchanged via [`Kernel::publish_signed`]; this sibling exists
    /// so Marmot can pin kind:445 group messages / kind:1059 gift-wraps to
    /// relays the author's own kind:10002 outbox does not cover.
    pub(crate) fn publish_signed_to(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        target: PublishTarget,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, target, None)
    }

    /// [`Kernel::publish_signed_to`] with an action `correlation_id` override.
    /// The remote-signer (NIP-46) `PublishNote` path uses this: a parked sign
    /// op carries the registry-minted correlation_id, and when the broker
    /// turns the request around the idle-tick loop publishes through here so
    /// the engine reports the dispatch correlation_id rather than the freshly
    /// signed event's `id`.
    pub(crate) fn publish_signed_to_with_correlation(
        &mut self,
        signed: &SignedEvent,
        p_tags: &[String],
        target: PublishTarget,
        correlation_id_override: Option<String>,
    ) -> Vec<OutboundMessage> {
        self.run_publish_engine(signed, p_tags, target, correlation_id_override)
    }

    /// Hex pubkey of the author of `event_id_hex`, or `None` if that event is
    /// not in the kernel's read-cache.
    ///
    /// Reads `self.events` — the same lightweight read-cache
    /// `reply_tags_for_parent` consults for NIP-10 parent-author re-notification
    /// — rather than the store directly. Production ingest
    /// (`ingest/timeline.rs`) populates both in lockstep, so the read-cache is a
    /// faithful view; the choice keeps reaction-author resolution byte-aligned
    /// with the reply path and avoids a store round-trip on the publish hot
    /// path. `None` is a normal result (the event simply hasn't been ingested);
    /// the caller degrades gracefully (D6 — emit the reaction with only the `e`
    /// tag, never panic).
    pub(crate) fn event_author(&self, event_id_hex: &str) -> Option<String> {
        self.events.get(event_id_hex).map(|e| e.author.clone())
    }

    /// Latest kind:3 follow set for `author_hex` (hex pubkeys from `p` tags),
    /// read from the shared store. Empty if no kind:3 is known yet.
    pub(crate) fn current_follows(&self, author_hex: &str) -> Vec<String> {
        let Some(author) = crate::kernel::hex_to_pubkey_bytes(author_hex) else {
            return Vec::new();
        };
        let Ok(mut iter) = self
            .store
            .scan_by_author_kind(&author, &[3], None, None, 1)
        else {
            return Vec::new();
        };
        let stored = match iter.next() {
            Some(Ok(stored)) => stored,
            _ => return Vec::new(),
        };
        stored
            .raw
            .tags
            .iter()
            .filter(|t: &&Vec<String>| t.first().map(String::as_str) == Some("p"))
            .filter_map(|t| t.get(1).cloned())
            .filter(|pk| is_hex_pubkey(pk))
            .collect()
    }
}
