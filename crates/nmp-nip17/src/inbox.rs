//! `DmInboxProjection` — the receive side of NIP-17 private direct messages.
//!
//! # Overview
//!
//! This is the *inbound* counterpart to [`crate::build_dm_rumor`]. It is an
//! [`IngestParser`](nmp_core::substrate::IngestParser) registered with a
//! kind:1059 filter. For every accepted gift-wrap envelope it:
//!
//! 1. Parses the verbatim wire JSON into a signed `nostr::Event` (the `sig`
//!    is mandatory — NIP-44 decryption verifies the seal).
//! 2. Launches a two-step **port-driven** gift-UNWRAP chain (ADR-0050 §D6):
//!    `Nip44DecryptForAccount(outer)` → `Nip44DecryptForAccount(seal)` →
//!    a kind:14 rumor. The decrypts run on the actor thread through the
//!    backend-transparent signer port, so a NIP-46 **bunker** account is
//!    structurally able to unseal a gift-wrap — the inbox never holds raw
//!    `nostr::Keys` (D13).
//! 3. The chain's terminal continuation accepts only kind:14 rumors, keys
//!    them by event id for idempotency, and groups them per conversation peer.
//!
//! The accumulated state is exposed through [`DmInboxProjection::snapshot_json`]
//! — the exact shape a host `register_snapshot_projection` closure returns —
//! so the inbox surfaces on every kernel snapshot tick.
//!
//! # ADR-0050 §D6 — gift-UNWRAP through the signer port
//!
//! The projection holds a [`CommandSender`](nmp_core::CommandSender) and the
//! pubkey-only [`ActiveAccountSlot`](nmp_core::slots::ActiveAccountSlot) — NOT
//! raw `nostr::Keys`. Each envelope's two NIP-44 decrypts are issued as
//! `ActorCommand::Nip44DecryptForAccount` and resolved by the actor's dispatch
//! arm: a **local** account decrypts `nostr::nips::nip44` inside the identity
//! runtime and the continuation runs INLINE; a **remote** (NIP-46 bunker /
//! NIP-55) account parks and the continuation runs from the mailbox-completion
//! drain. The inbox cannot tell which — backend transparency (V-78).
//!
//! ## Account pinning + epoch guard (§D6)
//!
//! The chain resolves the active account's pubkey ONCE at envelope arrival and
//! passes `signer_pubkey: Some(hex)` on every port step (never `None`), so a
//! mid-chain account switch cannot decrypt with a different key than the one the
//! chain started under. The terminal insert is guarded by a generation counter
//! ([`InboxStore::generation`]) bumped by [`DmInboxProjection::clear`]: a
//! continuation completing for a stale generation discards its plaintext instead
//! of leaking the previous account's message into the new account's snapshot
//! (issue #1138 cross-account privacy leak, preserved through the async chain).
//!
//! # D-doctrine
//!
//! * **D3 / D8** — `ingest_gift_wrap` runs synchronously on the actor thread; it
//!   only parses the outer envelope and enqueues ONE port command (no blocking,
//!   no I/O, no polling). Each continuation likewise only enqueues the next step
//!   or performs a bounded map insert.
//! * **D6** — every failure path is a silent no-op / discard: a malformed
//!   envelope, a decrypt failure (addressed to someone else / another protocol's
//!   kind:1059), a non-kind:14 rumor, a poisoned mutex, a stale generation.
//!   Nothing panics across the actor boundary.
//! * **D7** — an incoming rumor's `created_at` was stamped by the *sender*; it is
//!   the real send time, stored verbatim. Presentation layers format it.
//! * **D13** — no raw `nostr::Keys` cross this crate; only ciphertext / plaintext
//!   and a pubkey-only identity slot are observed.
//!
//! # Spec
//!
//! <https://github.com/nostr-protocol/nips/blob/master/17.md>

use std::sync::Arc;

use nmp_planner::{
    InterestId, InterestLifecycle, InterestScope, LogicalInterest, PTagRouting,
};
use nmp_core::slots::ActiveAccountSlot;
use nmp_store::VerifiedEvent;
use nmp_core::substrate::{IngestParser, ViewDependencies};
use nmp_core::{CommandSender, KindFilter};
use nmp_nip59::KIND_GIFT_WRAP;
use nostr::{Event, JsonUtil};
use serde::{Deserialize, Serialize};

use store::InboxStore;

/// One decrypted NIP-17 direct message, ready for a chat row.
///
/// A flat carrier — threading is represented only by `reply_to`; nested
/// rendering is a host concern. Fields are the minimum a shell needs to draw
/// one message.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmMessage {
    /// Inner kind:14 rumor event id (hex). Also the dedupe key in the inbox.
    pub id: String,
    /// Pubkey (hex) of whoever wrote the message — taken from the verified
    /// kind:13 seal, NOT from any tag (a tag could be forged; the seal is
    /// NIP-44-authenticated).
    pub sender_pubkey: String,
    /// Plaintext kind:14 `content`, verbatim.
    pub content: String,
    /// Unix seconds — the rumor's own `created_at`, stamped by the sender
    /// (D7: a received message's timestamp is real, not the `0` sentinel).
    /// Presentation layer formats this for display (aim.md §2: NMP is a
    /// data framework; backend sends raw timestamps, shells own
    /// formatting).
    pub created_at: u64,
    /// When the rumor carries a NIP-10 `["e", <id>, _, "reply"]` marker, the
    /// id of the message this one replies to.
    pub reply_to: Option<String>,
    /// `true` when the local account wrote this message — `sender_pubkey`
    /// equals the active account's pubkey. Pre-classified in Rust so the
    /// host shell never compares pubkeys to decide bubble alignment
    /// (thin-shell rule: that comparison is a protocol decision — the
    /// kind:13 seal authenticated this pubkey, and the host should not
    /// re-do that work).
    pub is_outgoing: bool,
    /// Relay URLs that delivered the gift-wrap envelope for this message.
    /// Populated from the kernel raw observer source provenance and kept
    /// deduplicated in first-seen order.
    #[serde(default)]
    pub source_relays: Vec<String>,
}

/// One DM thread — every message exchanged with a single peer.
///
/// Carries only the raw protocol identifier for the peer. Presentation layers
/// own all formatting: bech32 encoding (`npub1…`), abbreviation, avatar
/// initials, avatar tint colour, and any join against a profile cache for
/// the peer's display name / picture — see aim.md §2 (NMP is a data
/// framework; projection and snapshot code sends raw data only).
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmConversation {
    /// The OTHER party in the thread (hex pubkey, 64 chars) — never the
    /// local user.
    pub peer_pubkey: String,
    /// Messages in this thread, ordered chronologically — **oldest first,
    /// newest last**. This is the natural render order of a chat log so the
    /// host shell never re-sorts or reverses (thin-shell rule). The
    /// thread-level "most recent message" used by the inbox sort is
    /// `messages.last()`.
    pub messages: Vec<DmMessage>,
}

/// The serialised read-model a DM screen consumes: every conversation the
/// local account participates in.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct DmInboxSnapshot {
    /// Conversations, ordered by most-recent message (newest thread first).
    pub conversations: Vec<DmConversation>,
    /// **ADR-0050 §D7** — the decrypt-pipeline policy state, an errors-as-state
    /// (D6) tri-state that REPLACES the old `remote_signer_unsupported: bool`.
    /// Stable wire tokens the host switches on:
    ///
    /// * `"unavailable"` — no active account (not signed in); the host should
    ///   hide the DM screen entirely.
    /// * `"limited"` — an active account with `undecrypted_count > 0`: a bunker
    ///   backfill is pending or throttled by the bounded per-account decrypt
    ///   queue (§D7). NOT a silent drop — the host surfaces the count.
    /// * `"ok"` — an active account with everything decrypted.
    ///
    /// Additive default `"ok"` is deliberately NOT used; `Default` yields the
    /// empty string, and a fresh projection reports `"unavailable"` via
    /// [`Self::empty`] / a live no-account snapshot.
    #[serde(default)]
    pub decrypt_state: String,
    /// **ADR-0050 §D7** — count of envelopes admitted-but-not-yet-decrypted plus
    /// those not admitted because the per-account bound was full. Non-zero
    /// exactly when `decrypt_state == "limited"`. The host renders e.g. "N
    /// messages still decrypting" instead of silently hiding them.
    #[serde(default)]
    pub undecrypted_count: u32,
}

impl DmInboxSnapshot {
    /// An empty, no-active-account inbox — what a fresh projection (or a poisoned
    /// mutex, D6) reports: no conversations and `decrypt_state: "unavailable"`.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            conversations: Vec::new(),
            decrypt_state: store::DecryptState::Unavailable.as_wire().to_string(),
            undecrypted_count: 0,
        }
    }
}

/// Accumulates decrypted NIP-17 direct messages into a per-peer conversation
/// model, decrypting each gift-wrap through the actor's signer port
/// (ADR-0050 §D6).
///
/// Construct with the actor [`CommandSender`] and the pubkey-only
/// [`ActiveAccountSlot`], register the `Arc` as an `IngestParser` with
/// [`Self::kind_filter`], and capture it in a snapshot-projection closure
/// (`snapshot_json`).
pub struct DmInboxProjection {
    /// Sends `Nip44DecryptForAccount` port commands into the actor inbox
    /// (ADR-0050 §D6). Cloned into each chain step. Cheap, `Send + Sync`.
    tx: CommandSender,
    /// Pubkey-only active-account slot — populated by the kernel for EVERY
    /// backend (local AND bunker). Read once per envelope to pin the chain's
    /// `signer_pubkey` (§D6). `None` → not signed in → silent no-op (D6); the
    /// inbox NEVER holds secret key material (D13).
    active_pubkey: ActiveAccountSlot,
    /// Shared decrypt store + epoch guard, `Arc` so in-flight chains can carry a
    /// clone into their terminal continuation.
    store: Arc<InboxStore>,
}

impl DmInboxProjection {
    /// Construct an inbox bound to the actor command sender and the pubkey-only
    /// active-account slot (ADR-0050 §D6). The message store starts empty;
    /// envelopes arrive via [`IngestParser::parse`] and decrypt through the port.
    #[must_use]
    pub fn new(tx: CommandSender, active_pubkey: ActiveAccountSlot) -> Self {
        Self {
            tx,
            active_pubkey,
            store: Arc::new(InboxStore::new()),
        }
    }

    /// The kind filter to register this observer with — kind:1059 only.
    #[must_use]
    pub fn kind_filter() -> KindFilter {
        KindFilter::from_kinds([KIND_GIFT_WRAP])
    }

    /// Read the active account's hex pubkey (`None` if not signed in / poisoned
    /// slot). The chain pins this for `signer_pubkey: Some(hex)` (§D6).
    fn active_pubkey_hex(&self) -> Option<String> {
        self.active_pubkey.lock().ok().and_then(|slot| slot.clone())
    }

    /// Snapshot the current inbox as a typed [`DmInboxSnapshot`].
    ///
    /// Messages are grouped per peer, each conversation ordered
    /// chronologically (oldest first, newest last — the natural render order
    /// of a chat log), and conversations ordered by their most-recent message
    /// (newest thread first). Ties break on a stable secondary key so the
    /// order is total and deterministic across snapshot ticks.
    ///
    /// D6: a poisoned mutex degrades to [`DmInboxSnapshot::empty`] rather than
    /// panicking — this runs on the actor thread inside a snapshot tick.
    ///
    /// `decrypt_state` / `undecrypted_count` carry the §D7 policy state:
    /// `"unavailable"` with no active account, `"limited"` while a (bunker)
    /// backfill is pending or throttled by the bounded per-account decrypt queue,
    /// else `"ok"`. A bunker account decrypts structurally (§D6); the bound caps
    /// its concurrent interactive round-trips and surfaces the residue as state
    /// (never a silent drop).
    #[must_use]
    pub fn snapshot(&self) -> DmInboxSnapshot {
        let signed_in = self.active_pubkey_hex().is_some();
        let (state, undecrypted_count) = self.store.decrypt_status(signed_in);
        DmInboxSnapshot {
            conversations: self.store.snapshot_conversations(),
            decrypt_state: state.as_wire().to_string(),
            undecrypted_count,
        }
    }

    /// Clear all accumulated messages and advance the epoch, making the inbox
    /// empty and invalidating any in-flight decrypt chain's terminal insert.
    ///
    /// Called on account switch so the previous account's decrypted DMs cannot
    /// appear in the new account's snapshot (issue #1138). After this call
    /// `snapshot()` returns [`DmInboxSnapshot::empty`] until new messages arrive
    /// and decrypt under the new epoch.
    ///
    /// D6 — a poisoned mutex is a silent no-op for the message drop; the epoch
    /// bump is infallible so stale chains are still discarded.
    pub fn clear(&self) {
        self.store.clear();
    }

    /// Snapshot as a `serde_json::Value` — the exact shape a host
    /// `register_snapshot_projection` closure must return.
    ///
    /// D6: a serialisation failure (not expected for this plain struct)
    /// collapses to `{"conversations": []}` rather than propagating.
    #[must_use]
    pub fn snapshot_json(&self) -> serde_json::Value {
        serde_json::to_value(self.snapshot()).unwrap_or_else(|_| {
            serde_json::json!({
                "conversations": [],
                "decrypt_state": "unavailable",
                "undecrypted_count": 0,
            })
        })
    }

    /// Launch the port-driven gift-UNWRAP chain for one accepted kind:1059
    /// envelope (ADR-0050 §D6). Returns `true` when the chain was LAUNCHED
    /// (outer envelope parsed + an account is active); `false` for every
    /// pre-launch silent no-op (malformed envelope, not signed in). The
    /// decrypted message — if any — lands in the store asynchronously when the
    /// chain's terminal continuation runs (inline for a local account, from the
    /// mailbox drain for a bunker). Factored out of the observer/parser impls so
    /// the unit tests can assert the launch outcome.
    pub fn ingest_gift_wrap(&self, json: &str, source_relay_url: Option<&str>) -> bool {
        // Parse the verbatim signed event off the borrowed buffer. A malformed
        // envelope is a silent no-op (D6).
        let Ok(event) = Event::from_json(json) else {
            return false;
        };

        // §D6 account pinning — resolve the active pubkey ONCE. `None` (not
        // signed in) → silent no-op (D6). NEVER reads secret key material (D13).
        let Some(signer_hex) = self.active_pubkey_hex() else {
            return false;
        };

        // Capture the live epoch so a mid-chain account switch invalidates this
        // chain's terminal insert (§D6).
        let generation = self.store.generation();

        chain::launch_unwrap(
            self.tx.clone(),
            Arc::clone(&self.store),
            signer_hex,
            generation,
            event,
            source_relay_url.map(str::to_string),
        )
    }
}

impl IngestParser for DmInboxProjection {
    /// Receive a kind:1059 gift-wrap from the substrate ingest dispatcher.
    ///
    /// The dispatcher registration guarantees `kind == 1059`; the `debug_assert`
    /// below is the defence-in-depth guard. The event has already passed
    /// Schnorr signature verification at the ingest gate — no re-verify needed.
    /// We reconstruct the verbatim signed JSON that [`Self::ingest_gift_wrap`]
    /// needs (NIP-44 decryption requires the `sig` field) via a plain
    /// `serde_json::to_string` of the [`nmp_store::RawEvent`] that
    /// [`VerifiedEvent::raw`] exposes.
    ///
    /// Source relay provenance is unavailable at the `IngestParser` seam today
    /// (the dispatcher API carries only the `VerifiedEvent`); relay-delivered
    /// events therefore accumulate no `source_relays` entries via this path.
    /// Callers that have the relay URL available should use
    /// [`Self::ingest_gift_wrap`] directly.
    ///
    /// D3/D8 — runs synchronously on the actor thread; bounded per-event work
    /// (one JSON serialisation, one outer-envelope parse, ONE port command).
    /// D6 — every pre-launch failure is a silent no-op.
    fn parse(&self, evt: &VerifiedEvent) {
        debug_assert_eq!(
            evt.raw().kind,
            KIND_GIFT_WRAP,
            "dispatcher misconfigured: DmInboxProjection IngestParser received kind {}",
            evt.raw().kind
        );
        // Reconstruct the verbatim signed NIP-01 JSON from the RawEvent.
        // `RawEvent` derives `Serialize` with the exact NIP-01 field order,
        // so this is lossless — no field is dropped.
        let Ok(json) = serde_json::to_string(evt.raw()) else {
            return;
        };
        let _ = self.ingest_gift_wrap(&json, None);
    }
}

/// Stable id for the active-account-owned gift-wrap inbox interest.
///
/// The id is intentionally independent of the pubkey so an account switch
/// replaces the prior `#p` filter instead of accumulating one long-lived
/// subscription per account.
#[must_use]
pub fn active_giftwrap_inbox_interest_id() -> InterestId {
    InterestId(nmp_planner::stable_hash::stable_hash64(
        "nip17.giftwrap.active",
    ))
}

/// Tailing [`LogicalInterest`] for kind:1059 `#p <pubkey>` gift-wraps — the
/// subscription a host pushes (via `NmpApp::push_interest`) so the DM inbox
/// actually receives envelopes.
///
/// The filter targets a concrete `#p <pubkey>` because NIP-17 gift-wraps are
/// addressed to a real account. The stable id + [`InterestScope::ActiveAccount`]
/// scope makes the registration lifecycle single-slot: re-pushing for a new
/// active account replaces the old filter, and logout withdraws one known id.
/// The kernel routes the `#p` filter to the account's kind:10050 DM relays via
/// [`PTagRouting::Nip17DmRelays`]; if the kind:10050 list is unknown or empty,
/// the compiler emits no subscription instead of falling back to public NIP-65
/// read relays.
#[must_use]
pub fn active_giftwrap_inbox_interest(pubkey: &str) -> LogicalInterest {
    let deps = ViewDependencies {
        kinds: vec![KIND_GIFT_WRAP],
        tag_refs: vec![("p".to_string(), pubkey.to_string())],
        ..Default::default()
    };
    let mut interest = deps.into_logical_interest(
        active_giftwrap_inbox_interest_id(),
        InterestScope::ActiveAccount,
        InterestLifecycle::Tailing,
    );
    interest.shape.p_tag_routing = PTagRouting::Nip17DmRelays;
    interest
}

#[path = "inbox/store.rs"]
mod store;

#[path = "inbox/chain.rs"]
mod chain;

#[cfg(test)]
#[path = "inbox/tests.rs"]
mod tests;

// ADR-0050 §D6 port-driven gift-UNWRAP chain tests — kept in a sibling file so
// `inbox.rs` and `inbox/tests.rs` each stay under the 500-LOC ceiling.
#[cfg(test)]
#[path = "inbox/chain_tests.rs"]
mod chain_tests;

// `InboxStore` unit tests (admit/chain_done/decrypt_status) — kept in a sibling
// file for the 500-LOC ceiling. Contains the #1349 regression tests.
#[cfg(test)]
#[path = "inbox/store_tests.rs"]
mod store_tests;
