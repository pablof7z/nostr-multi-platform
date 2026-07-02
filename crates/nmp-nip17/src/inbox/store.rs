//! `InboxStore` — the shared, epoch-guarded decrypt-result store for the DM
//! inbox, plus the small pure rumor/relay helpers the port chain uses.
//!
//! Extracted from `inbox.rs` to keep that file under the 500-LOC ceiling. The
//! store is held behind an `Arc` so each in-flight gift-UNWRAP port chain
//! (`super::chain`) carries a clone into its terminal continuation and inserts
//! the decrypted message even though the chain outlives the synchronous
//! `ingest_gift_wrap` call (ADR-0072 §D6). The `generation` counter is the §D6
//! epoch guard against cross-account leaks.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use nmp_core::substrate::{BoundedMessageMap, MAX_PROJECTION_MESSAGES};

use super::{DmConversation, DmMessage};

/// Per-account bound on concurrently-in-flight decrypt chains (ADR-0072 §D7).
///
/// A LOCAL account resolves each decrypt `Ready` inline on the actor thread, so
/// its chains terminate within the same `ingest_gift_wrap` dispatch and the
/// in-flight count is back to 0 before the next envelope — it never approaches
/// this bound. A REMOTE (bunker) account parks each decrypt and resolves it from
/// a later mailbox drain, so concurrent backfill envelopes accumulate in flight;
/// this bound caps the outstanding interactive round-trips (one chain = up to 2
/// sequential bunker RPCs). The async-vs-inline resolution difference makes the
/// bound self-target remote accounts WITHOUT the projection branching on a
/// signer-kind label (one mechanism, §D7 "strictly sequential per-account").
pub(crate) const MAX_IN_FLIGHT_DECRYPTS: u64 = 8;

/// Policy state for the inbox decrypt pipeline (ADR-0072 §D7) — the
/// errors-as-state replacement for the old `remote_signer_unsupported: bool`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DecryptState {
    /// An active account; every admitted envelope has decrypted (nothing
    /// pending, nothing over the bound).
    Ok,
    /// An active account, but envelopes are pending decryption or were not
    /// admitted because the per-account bound is full (bunker backfill in
    /// progress / throttled). `undecrypted_count` is non-zero. NOT a silent
    /// drop — the host surfaces the count.
    Limited,
    /// No active account (not signed in) — the host should hide the DM screen.
    Unavailable,
}

#[derive(Debug, Default)]
struct BatchBackfillState {
    generation: u64,
    in_flight: bool,
    completed_candidate_count: usize,
    blocked_candidate_count: usize,
    unsupported: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum BatchBackfillFinish {
    Succeeded,
    Unsupported,
    Failed,
}

impl DecryptState {
    /// The stable wire token other platforms switch on.
    pub(crate) fn as_wire(self) -> &'static str {
        match self {
            DecryptState::Ok => "ok",
            DecryptState::Limited => "limited",
            DecryptState::Unavailable => "unavailable",
        }
    }
}

/// Shared, decrypt-result store for the DM inbox.
pub(crate) struct InboxStore {
    /// Accepted decrypted messages keyed by inner-rumor event id. The value
    /// pairs the conversation peer with the message. Idempotent — a
    /// re-delivered envelope replaces rather than duplicates. Bounded by
    /// [`MAX_PROJECTION_MESSAGES`] so a long-running inbox cannot grow
    /// unboundedly across a session; once full, the oldest-by-insertion
    /// rumor is evicted, keeping per-tick snapshot serialisation O(cap).
    messages: Mutex<BoundedMessageMap<String, (String, DmMessage)>>,
    /// Account-switch epoch (§D6). Bumped by [`super::DmInboxProjection::clear`];
    /// each chain captures the value live at launch and a terminal continuation
    /// discards its plaintext if the counter has since advanced (the active
    /// account changed mid-flight) — so a previous account's message can never
    /// leak into the new account's snapshot (#1138, async-chain-safe).
    generation: AtomicU64,
    /// Chains launched (outer decrypt sent) but not yet terminated (§D7).
    /// Incremented by [`Self::admit`], decremented by [`Self::chain_done`] on
    /// EVERY chain exit path (store, discard, decrypt error). Caps interactive
    /// bunker backfill at [`MAX_IN_FLIGHT_DECRYPTS`].
    in_flight: AtomicU64,
    /// Envelopes NOT admitted because the bound was full when they arrived
    /// (§D7 — never silently dropped; surfaced as the undecrypted count). Reset
    /// by [`Self::clear`] (account switch starts a fresh backfill budget).
    over_bound: AtomicU64,
    /// Number of store-sourced candidates being processed by an active
    /// decrypt-session batch replay (#1259). Kept separate from scalar
    /// `in_flight` so the snapshot can report pending work without treating the
    /// old scalar queue as the candidate source.
    batch_pending: AtomicU64,
    batch_state: Mutex<BatchBackfillState>,
}

impl InboxStore {
    pub(crate) fn new() -> Self {
        Self {
            messages: Mutex::new(BoundedMessageMap::new(MAX_PROJECTION_MESSAGES)),
            generation: AtomicU64::new(0),
            in_flight: AtomicU64::new(0),
            over_bound: AtomicU64::new(0),
            batch_pending: AtomicU64::new(0),
            batch_state: Mutex::new(BatchBackfillState::default()),
        }
    }

    /// Current epoch — captured by a chain at launch (§D6 account pinning).
    pub(crate) fn generation(&self) -> u64 {
        self.generation.load(Ordering::Acquire)
    }

    /// Try to admit one new decrypt chain under the §D7 bound. Returns `true`
    /// and reserves an in-flight slot when under [`MAX_IN_FLIGHT_DECRYPTS`];
    /// returns `false` and records the over-bound envelope (never a silent drop)
    /// when the bound is full. A LOCAL account's chains terminate inline before
    /// the next envelope so it effectively never sees a full bound; a bunker's
    /// concurrent backfill is what this throttles.
    ///
    /// **Over-bound drain** (fix for #1349 Defect 1): when the admit succeeds
    /// AND the `over_bound` counter is non-zero, one previously-deferred slot is
    /// consumed — `over_bound` tracks CURRENTLY-DEFERRED envelopes, not a
    /// monotonic reject count. The [`InterestLifecycle::Tailing`] subscription
    /// re-delivers every rejected envelope, so each successful re-admit must
    /// balance one earlier rejection. Once all deferred envelopes re-arrive and
    /// are admitted, `over_bound` returns to 0 and
    /// [`Self::decrypt_status`] returns `(Ok, 0)`.
    pub(crate) fn admit(&self) -> bool {
        // CAS-free reserve: increment, and if we blew the bound, undo + count.
        let prior = self.in_flight.fetch_add(1, Ordering::AcqRel);
        if prior >= MAX_IN_FLIGHT_DECRYPTS {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
            self.over_bound.fetch_add(1, Ordering::AcqRel);
            return false;
        }
        // Successful admit: consume one deferred slot when over_bound is
        // non-zero (the Tailing sub re-delivers previously rejected envelopes;
        // each one that is now admitted is no longer "deferred").  Saturating
        // so a spurious concurrent call can never underflow.
        let prior_over = self.over_bound.load(Ordering::Acquire);
        if prior_over > 0 {
            self.over_bound.fetch_sub(1, Ordering::AcqRel);
        }
        true
    }

    /// Release the in-flight slot an admitted chain reserved — called on EVERY
    /// chain exit (terminal store, kind/peer discard, decrypt error).
    ///
    /// `generation` must be the epoch captured by the chain at launch (the
    /// value returned by [`Self::generation`] before [`chain::launch_unwrap`]
    /// enqueued the first decrypt command). If the current generation has since
    /// advanced (account switch mid-flight, §D6), the decrement is skipped:
    /// [`Self::clear`] already reset `in_flight` to 0 for the new epoch, and
    /// applying a stale decrement would corrupt the new account's counter
    /// (fix for #1349 Defect 2 — epoch-safe chain_done).
    pub(crate) fn chain_done(&self, generation: u64) {
        // Epoch guard: stale old-epoch completions must not touch the new
        // account's in_flight counter.  The generation check is not atomic
        // with the fetch_sub, but the window is benign: if a concurrent
        // clear() races exactly here it bumps the generation BEFORE zeroing
        // in_flight, so our (now-stale) fetch_sub can at worst briefly
        // under-count the NEW account's in_flight — self-healing on the very
        // next chain_done from a legitimately-new chain, because clear() reset
        // in_flight to 0 and the new chains build from 0.  The guard eliminates
        // the SYSTEMATIC corruption described in #1349 §D2; the residual
        // one-tick under-count is transient and not user-visible.
        if self.generation.load(Ordering::Acquire) != generation {
            return;
        }
        let prior = self.in_flight.load(Ordering::Acquire);
        if prior > 0 {
            self.in_flight.fetch_sub(1, Ordering::AcqRel);
        }
    }

    /// Derive the §D7 policy state `(decrypt_state, undecrypted_count)` for the
    /// snapshot. `signed_in` is whether an active account exists.
    ///
    /// `undecrypted_count` = in-flight (admitted, pending decryption) +
    /// over-bound (arrived while the bound was full). `decrypt_state` is
    /// `Unavailable` with no account, `Limited` while that count is non-zero
    /// (backfill pending/throttled), else `Ok`.
    pub(crate) fn decrypt_status(&self, signed_in: bool) -> (DecryptState, u32) {
        if !signed_in {
            return (DecryptState::Unavailable, 0);
        }
        let pending = self.in_flight.load(Ordering::Acquire);
        let over = self.over_bound.load(Ordering::Acquire);
        let batch = self.batch_pending.load(Ordering::Acquire);
        let undecrypted = pending.saturating_add(over).saturating_add(batch);
        let state = if undecrypted == 0 {
            DecryptState::Ok
        } else {
            DecryptState::Limited
        };
        (state, undecrypted.min(u64::from(u32::MAX)) as u32)
    }

    /// Mark a store-backed batch replay as in-flight for `generation`.
    ///
    /// Returns `false` when there is no work, a replay is already running, the
    /// selected signer already reported unsupported for this account epoch, or
    /// the same candidate set was already processed/failed. This prevents sync
    /// callbacks from re-spamming signer-session begins while keeping new store
    /// candidates eligible for a later replay.
    pub(crate) fn begin_batch_backfill(&self, generation: u64, candidate_count: usize) -> bool {
        if candidate_count == 0 {
            return false;
        }
        let Ok(mut state) = self.batch_state.lock() else {
            return false;
        };
        if state.generation != generation {
            *state = BatchBackfillState {
                generation,
                ..Default::default()
            };
        }
        if state.in_flight
            || state.unsupported
            || candidate_count <= state.completed_candidate_count
            || candidate_count <= state.blocked_candidate_count
        {
            return false;
        }
        state.in_flight = true;
        self.batch_pending
            .store(candidate_count as u64, Ordering::Release);
        true
    }

    /// Cheap pre-query guard for sync callbacks.
    ///
    /// We still need the store query to know whether a completed/failed replay
    /// has new candidates, but once a signer reports the optional batch
    /// capability unsupported for an account epoch there is no reason to scan
    /// the store again until the epoch changes.
    pub(crate) fn may_probe_batch_backfill(&self, generation: u64) -> bool {
        let Ok(state) = self.batch_state.lock() else {
            return false;
        };
        if state.generation != generation {
            return true;
        }
        !state.in_flight && !state.unsupported
    }

    pub(crate) fn finish_batch_backfill(
        &self,
        generation: u64,
        candidate_count: usize,
        finish: BatchBackfillFinish,
    ) {
        let Ok(mut state) = self.batch_state.lock() else {
            self.batch_pending.store(0, Ordering::Release);
            return;
        };
        if state.generation != generation {
            return;
        }
        state.in_flight = false;
        self.batch_pending.store(0, Ordering::Release);
        match finish {
            BatchBackfillFinish::Succeeded => {
                state.completed_candidate_count =
                    state.completed_candidate_count.max(candidate_count);
                state.blocked_candidate_count = 0;
                // Successful replay used the store's real candidate set, so
                // any scalar over-bound residue is stale accounting, not
                // remaining work.
                self.over_bound.store(0, Ordering::Release);
            }
            BatchBackfillFinish::Unsupported => {
                state.unsupported = true;
            }
            BatchBackfillFinish::Failed => {
                state.blocked_candidate_count = state.blocked_candidate_count.max(candidate_count);
            }
        }
    }

    /// Insert one decrypted message under epoch `gen`. A no-op (returns `false`)
    /// when `gen` is stale (the account switched mid-flight, §D6) or the mutex
    /// is poisoned (D6). When the id already exists, merges source-relay
    /// provenance instead of duplicating (idempotent re-delivery).
    pub(crate) fn insert(
        &self,
        gen: u64,
        message_id: String,
        peer_pubkey: String,
        message: DmMessage,
        source_relay_url: Option<&str>,
    ) -> bool {
        // §D6 epoch guard — discard a completion for a superseded account.
        if gen != self.generation.load(Ordering::Acquire) {
            return false;
        }
        let Ok(mut messages) = self.messages.lock() else {
            return false;
        };
        if let Some((_peer, existing)) = messages.get_mut(&message_id) {
            merge_source_relay(&mut existing.source_relays, source_relay_url);
            return true;
        }
        messages.insert(message_id, (peer_pubkey, message));
        true
    }

    /// Snapshot the current messages grouped per peer (see
    /// [`super::DmInboxProjection::snapshot`] for ordering semantics).
    pub(crate) fn snapshot_conversations(&self) -> Vec<DmConversation> {
        let Ok(messages) = self.messages.lock() else {
            return Vec::new();
        };
        let mut by_peer: BTreeMap<String, Vec<DmMessage>> = BTreeMap::new();
        for (peer, msg) in messages.values() {
            by_peer.entry(peer.clone()).or_default().push(msg.clone());
        }
        let mut conversations: Vec<DmConversation> = by_peer
            .into_iter()
            .map(|(peer_pubkey, mut msgs)| {
                // Chronological within the thread — oldest first, newest last;
                // tie-break on id ascending so the order is total even when two
                // messages share a `created_at`.
                msgs.sort_by(|a, b| {
                    a.created_at
                        .cmp(&b.created_at)
                        .then_with(|| a.id.cmp(&b.id))
                });
                DmConversation {
                    peer_pubkey,
                    messages: msgs,
                }
            })
            .collect();
        // Newest conversation first — keyed on the thread's most-recent message;
        // tie-break on peer pubkey descending for a total, stable order.
        conversations.sort_by(|a, b| {
            let a_latest = a.messages.last().map_or(0, |m| m.created_at);
            let b_latest = b.messages.last().map_or(0, |m| m.created_at);
            b_latest
                .cmp(&a_latest)
                .then_with(|| b.peer_pubkey.cmp(&a.peer_pubkey))
        });
        conversations
    }

    /// Drop all messages and bump the epoch so any chain in flight under the
    /// previous epoch discards its terminal insert (§D6). Also resets the §D7
    /// backfill counters: the new account starts with a fresh in-flight budget
    /// and zero over-bound. In-flight chains from the OLD epoch still call
    /// `chain_done` when they resolve; resetting `in_flight` to 0 here means
    /// those stale `chain_done` calls are absorbed by the saturating guard
    /// (they cannot drive the new account's count negative).
    pub(crate) fn clear(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
        self.in_flight.store(0, Ordering::Release);
        self.over_bound.store(0, Ordering::Release);
        self.batch_pending.store(0, Ordering::Release);
        if let Ok(mut state) = self.batch_state.lock() {
            *state = BatchBackfillState::default();
            state.generation = self.generation();
        }
        if let Ok(mut messages) = self.messages.lock() {
            *messages = BoundedMessageMap::new(MAX_PROJECTION_MESSAGES);
        }
    }
}

/// First `["p", <pubkey>]` tag value on a rumor, if any.
pub(crate) fn first_p_tag(rumor: &nostr::UnsignedEvent) -> Option<String> {
    rumor.tags.iter().find_map(|tag| {
        let slice = tag.as_slice();
        match slice {
            [name, value, ..] if name == "p" => Some(value.clone()),
            _ => None,
        }
    })
}

/// First NIP-10 reply marker — `["e", <event-id>, <relay-hint>, "reply"]` —
/// on a rumor, returning the referenced event id.
pub(crate) fn first_reply_e_tag(rumor: &nostr::UnsignedEvent) -> Option<String> {
    rumor.tags.iter().find_map(|tag| {
        let slice = tag.as_slice();
        match slice {
            [name, value, _hint, marker, ..] if name == "e" && marker == "reply" => {
                Some(value.clone())
            }
            _ => None,
        }
    })
}

pub(crate) fn source_relays_from(source_relay_url: Option<&str>) -> Vec<String> {
    let mut relays = Vec::new();
    merge_source_relay(&mut relays, source_relay_url);
    relays
}

pub(crate) fn merge_source_relay(relays: &mut Vec<String>, source_relay_url: Option<&str>) {
    let Some(source) = source_relay_url.filter(|source| !source.is_empty()) else {
        return;
    };
    if !relays.iter().any(|existing| existing == source) {
        relays.push(source.to_string());
    }
}
