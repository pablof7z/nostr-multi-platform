//! `nmp.nip51.block_relay` / `nmp.nip51.unblock_relay` — kind:10006 (NIP-51
//! "blocked relays" list) publish action.
//!
//! Structural sibling of [`crate::publish_relay_list`] (kind:10002). Just as
//! routing owns the kind:10002 publish path end-to-end, it owns the kind:10006
//! publish path end-to-end: `nmp-core` stays wire-shape-agnostic (D0 — no
//! kind numbers, no tag names, no protocol nouns in the kernel crate).
//!
//! # Tag shape — NIP-51 § kind:10006
//!
//! ```text
//! ["relay", "<wss-url>"]
//! ```
//!
//! One tag per blocked relay URL. Non-`relay` tags, non-`wss://` URLs, and
//! `relay` tags without a URL are never emitted. The builder's wire shape
//! MUST agree with what [`crate::blocked_relays::parse_blocked_relay_list`]
//! parses so a block → publish → ingest → cache round-trip is lossless.
//!
//! # D0 — namespace
//!
//! The action namespaces are `nmp.nip51.block_relay` and
//! `nmp.nip51.unblock_relay`. `nmp-core` never names the kind:10006 wire
//! shape; the event builder lives here alongside the ingest parser and the
//! in-memory cache so routing owns the protocol end-to-end.
//!
//! # D7 — `created_at` sentinel
//!
//! The unsigned event is built with `created_at: 0`. The actor re-stamps
//! it from `kernel.now_secs()` before signing; this module never reads the
//! system clock.
//!
//! # Round-trip
//!
//! The republished kind:10006 returns through the cold-start tailing
//! subscription (`SELF_KINDS_TAILING` includes 10006) → `Kind10006Parser`
//! → re-upserts the `InMemoryBlockedRelayCache` the kernel holds as
//! `Arc<dyn BlockedRelayLookup>` → the System-A wire-plan block filter
//! drops the relay from `current_plan` on the next recompile → the
//! diagnostics projection flips its `connection_label` to `"Blocked"`.
//!
//! # Idempotency
//!
//! Blocking an already-blocked relay, or unblocking a relay that is not
//! blocked, is a no-op: `start` returns `ActionRejection::Conflict` and
//! no correlation id is minted, so no publish is attempted and no spinner
//! is left hanging.
//!
//! # Unblocking the last entry
//!
//! When the last blocked relay is removed, the builder emits a kind:10006
//! with zero `["relay", …]` tags — the "I cleared my blocked-relay list"
//! signal. `ingest` then removes the cache entry and subsequent
//! `snapshot_blocked_relays` calls return an empty set (fail-open: the
//! router treats an empty blocked set as "no relays blocked"). This is
//! intentional and symmetric with the empty-list semantics of kind:10002.

use std::collections::BTreeSet;
use std::sync::Arc;

use nmp_core::substrate::{
    ActionContext, ActionModule, ActionPayload, ActionPayloadDecodeError, ActionRegistrar,
    ActionRejection, BlockedRelayLookup,
};
use nmp_signer_iface::UnsignedEvent;
use nmp_core::actor::{ActorCommand, PublishCommand};
use nmp_kinds::KIND_BLOCKED_RELAYS;
use serde::{Deserialize, Serialize};

use crate::blocked_relays::InMemoryBlockedRelayCache;
use crate::canonical::canonicalize_relay_url;

// ─── Builder ─────────────────────────────────────────────────────────────────

/// Build a kind:10006 **unsigned** event from a set of canonical `wss://` relay
/// URLs.
///
/// The event's wire shape is the exact inverse of what
/// `blocked_relays::parse_blocked_relay_list` consumes: one `["relay", url]`
/// tag per entry, `created_at = 0` (D7 sentinel), empty content, empty pubkey
/// (the actor fills both from the signing key at sign time).
///
/// An empty `blocked_urls` slice produces a valid kind:10006 with zero tags —
/// the NIP-51 "I cleared my blocked list" signal. The caller (the unblock
/// action) MUST publish this empty-list event deliberately; doing so removes
/// the cache entry so subsequent `snapshot_blocked_relays` calls return empty.
#[must_use]
fn build_blocked_relay_list_event(blocked_urls: &BTreeSet<String>) -> UnsignedEvent {
    let tags: Vec<Vec<String>> = blocked_urls
        .iter()
        .map(|url| vec!["relay".to_string(), url.clone()])
        .collect();
    UnsignedEvent {
        // Empty placeholder — the actor re-derives the pubkey from the
        // signing key at sign time (see `ActorCommand::PublishUnsignedEvent`).
        pubkey: String::new(),
        kind: KIND_BLOCKED_RELAYS,
        tags,
        content: String::new(),
        // D7 sentinel — the actor re-stamps from `kernel.now_secs()`.
        created_at: 0,
    }
}

/// Validate that `url` is a `wss://` relay URL and return its canonical form.
///
/// `ws://` URLs are excluded because `parse_blocked_relay_list` requires
/// `wss://`; a `ws://` entry would be silently dropped on ingest, making the
/// block invisible. Rejecting early prevents a silent no-op.
fn validate_and_canonicalize(url: &str) -> Result<String, ActionRejection> {
    if url.starts_with("wss://") {
        // Fail-closed: reject a `wss://`-prefixed but hostless URL the
        // canonical authority cannot canonicalize, rather than persisting a
        // malformed block entry (#967).
        canonicalize_relay_url(url).ok_or_else(|| {
            ActionRejection::Invalid(format!(
                "block/unblock relay: not a valid wss:// URL (no host); got {url:?}"
            ))
        })
    } else if url.starts_with("ws://") {
        // Reject: the NIP-51 ingest parser requires wss://; a ws:// entry
        // would be silently skipped and the block would have no effect.
        Err(ActionRejection::Invalid(format!(
            "block/unblock relay: URL must be wss:// (ws:// is not persisted by \
             the kind:10006 parser); got {url:?}"
        )))
    } else {
        Err(ActionRejection::Invalid(format!(
            "block/unblock relay: expected a wss:// URL, got {url:?}"
        )))
    }
}

// ─── block_relay ─────────────────────────────────────────────────────────────

/// Wire shape for `nmp.nip51.block_relay`.
///
/// The host supplies the URL to add to the active account's kind:10006
/// blocked-relay list and the active account's hex pubkey so the module can
/// read the current blocked set to apply the edit idempotently.
///
/// `account_pubkey` MUST be the active signer's hex pubkey. The module reads
/// the current blocked set for that pubkey, adds `url` to it, and publishes
/// the new kind:10006 signed by the active account (the `signer_pubkey: None`
/// sentinel in `PublishUnsignedEvent` — the actor signs with the active
/// account regardless of which pubkey is passed here).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlockRelayInput {
    /// The relay URL to block. Must be `wss://`. Canonicalised by the module.
    pub url: String,
    /// Active account hex pubkey. Used to read the current blocked set.
    pub account_pubkey: String,
}

/// The `nmp.nip51.block_relay` [`ActionModule`].
///
/// Holds an [`Arc<InMemoryBlockedRelayCache>`] so it can read the active
/// account's current blocked set without reaching through the kernel (ADR-0052
/// rung 5.2 — stateful module carries its dependency, captured at composition
/// time by [`register_block_relay_actions`]).
pub struct BlockRelayAction {
    cache: Arc<InMemoryBlockedRelayCache>,
}

impl BlockRelayAction {
    /// Construct a [`BlockRelayAction`] backed by the given cache.
    ///
    /// Mirrors the construction pattern of `Kind10006Parser::new` — both hold
    /// an `Arc` clone of the SAME `InMemoryBlockedRelayCache` the kernel reads
    /// via `Arc<dyn BlockedRelayLookup>`, so the module sees live state.
    #[must_use]
    pub fn new(cache: Arc<InMemoryBlockedRelayCache>) -> Self {
        Self { cache }
    }
}

impl ActionModule for BlockRelayAction {
    const NAMESPACE: &'static str = "nmp.nip51.block_relay";
    type Action = BlockRelayInput;

    /// ADR-0064 (#1756): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(
        bytes: &[u8],
    ) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<BlockRelayInput as ActionPayload>::decode(bytes))
    }

    /// Validate the URL scheme and guard against idempotent re-blocks.
    ///
    /// `start` rejects:
    /// - Non-`wss://` URLs (`ActionRejection::Invalid`).
    /// - A relay already present in the active account's blocked set
    ///   (`ActionRejection::Conflict` — blocking an already-blocked relay
    ///   is a no-op; no correlation id is minted and no publish fires).
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let canonical = validate_and_canonicalize(&action.url)?;
        // Idempotent guard: if the relay is already blocked, reject as a
        // Conflict so no correlation id is minted and no publish fires.
        let current = self.cache.blocked_relays(&action.account_pubkey);
        if current.contains(&canonical) {
            return Err(ActionRejection::Conflict(format!(
                "relay {canonical:?} is already blocked for account {}",
                action.account_pubkey
            )));
        }
        Ok(())
    }

    /// Add `url` to the blocked set and publish the updated kind:10006.
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // `start` already validated the URL canonicalizes; re-derive here and
        // fail-closed if it somehow does not (#967) rather than inserting a raw
        // malformed key.
        let canonical = canonicalize_relay_url(&action.url)
            .ok_or_else(|| format!("block relay: not a valid wss:// URL: {:?}", action.url))?;
        // Re-read the current blocked set (may have changed since `start`).
        let current = self.cache.blocked_relays(&action.account_pubkey);
        let mut new_set: BTreeSet<String> = current.iter().cloned().collect();
        new_set.insert(canonical);
        let event = build_blocked_relay_list_event(&new_set);
        // Route through the kernel's generic sign+publish seam. The active
        // account signs (`signer_pubkey: None`). The correlation id threads
        // through so the publish engine reports it in `action_results` and
        // the host spinner that fired on `dispatch_action` can be cleared.
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

// ─── unblock_relay ───────────────────────────────────────────────────────────

/// Wire shape for `nmp.nip51.unblock_relay`.
///
/// Symmetric to [`BlockRelayInput`]: supply the URL to remove from the active
/// account's kind:10006 blocked-relay list and the active account pubkey.
///
/// Unblocking the last entry republishes an empty kind:10006 deliberately —
/// the NIP-51 "I cleared my blocked list" signal (see module-level docs).
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct UnblockRelayInput {
    /// The relay URL to unblock. Must be `wss://`. Canonicalised by the module.
    pub url: String,
    /// Active account hex pubkey. Used to read the current blocked set.
    pub account_pubkey: String,
}

/// The `nmp.nip51.unblock_relay` [`ActionModule`].
///
/// Symmetric to [`BlockRelayAction`]. Removes a URL from the blocked set and
/// republishes kind:10006. If the relay is not currently blocked, rejects with
/// `ActionRejection::Conflict` (no publish, no correlation id).
pub struct UnblockRelayAction {
    cache: Arc<InMemoryBlockedRelayCache>,
}

impl UnblockRelayAction {
    /// Construct an [`UnblockRelayAction`] backed by the given cache.
    #[must_use]
    pub fn new(cache: Arc<InMemoryBlockedRelayCache>) -> Self {
        Self { cache }
    }
}

impl ActionModule for UnblockRelayAction {
    const NAMESPACE: &'static str = "nmp.nip51.unblock_relay";
    type Action = UnblockRelayInput;

    /// ADR-0064 (#1756): opt into the typed FlatBuffers payload doorway; the
    /// fail-closed `schema_version` gate runs in `decode` (BEFORE `start`).
    fn decode_payload(
        bytes: &[u8],
    ) -> Option<Result<Self::Action, ActionPayloadDecodeError>> {
        Some(<UnblockRelayInput as ActionPayload>::decode(bytes))
    }

    /// Validate the URL scheme and guard against unblocking a non-blocked relay.
    ///
    /// `start` rejects:
    /// - Non-`wss://` URLs (`ActionRejection::Invalid`).
    /// - A relay NOT present in the active account's blocked set
    ///   (`ActionRejection::Conflict` — unblocking a non-blocked relay is a
    ///   no-op; no correlation id is minted and no publish fires).
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        let canonical = validate_and_canonicalize(&action.url)?;
        // Idempotent guard: if the relay is not blocked, reject as a Conflict.
        let current = self.cache.blocked_relays(&action.account_pubkey);
        if !current.contains(&canonical) {
            return Err(ActionRejection::Conflict(format!(
                "relay {canonical:?} is not blocked for account {}; nothing to unblock",
                action.account_pubkey
            )));
        }
        Ok(())
    }

    /// Remove `url` from the blocked set and publish the updated kind:10006.
    ///
    /// When this is the last entry, publishes an empty-tag kind:10006 (the
    /// NIP-51 "I cleared my blocked list" signal). The ingest parser will
    /// remove the cache entry on receipt, making subsequent
    /// `snapshot_blocked_relays` calls return an empty set (fail-open).
    fn execute(
        &self,
        action: Self::Action,
        correlation_id: &str,
        send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // `start` already validated the URL canonicalizes; re-derive here and
        // fail-closed if it somehow does not (#967).
        let canonical = canonicalize_relay_url(&action.url)
            .ok_or_else(|| format!("unblock relay: not a valid wss:// URL: {:?}", action.url))?;
        let current = self.cache.blocked_relays(&action.account_pubkey);
        let mut new_set: BTreeSet<String> = current.iter().cloned().collect();
        new_set.remove(&canonical);
        // An empty `new_set` is intentional: republish a kind:10006 with zero
        // tags to signal "I cleared my blocked list" (see module-level docs).
        let event = build_blocked_relay_list_event(&new_set);
        send(ActorCommand::Publish(PublishCommand::UnsignedEvent {
            event,
            correlation_id: Some(correlation_id.to_string()),
            signer_pubkey: None,
        }));
        Ok(())
    }
}

// ─── Registration ────────────────────────────────────────────────────────────

/// Register the `nmp.nip51.block_relay` and `nmp.nip51.unblock_relay` action
/// modules as **yielding defaults** (ADR-0049 Part 1): an app may pre-empt
/// either regardless of call order.
///
/// Both modules share a clone of `cache` — the SAME
/// `InMemoryBlockedRelayCache` that `Kind10006Parser` writes into and the
/// kernel reads via `Arc<dyn BlockedRelayLookup>`. Sharing one instance is
/// what lets the module's idempotency guard see live state from the ingest
/// path.
///
/// Designed to be called from `nmp_defaults::tiers::register_substrate`
/// immediately after the `InMemoryBlockedRelayCache` is constructed and handed
/// to `set_blocked_relay_lookup` and `Kind10006Parser`. The Arc clones here
/// add no overhead — the cache lives for the process lifetime.
pub fn register_block_relay_actions(
    app: &mut impl ActionRegistrar,
    cache: Arc<InMemoryBlockedRelayCache>,
) {
    app.register_default_action(BlockRelayAction::new(Arc::clone(&cache)));
    app.register_default_action(UnblockRelayAction::new(cache));
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
#[path = "block_relay_tests.rs"]
mod tests;
