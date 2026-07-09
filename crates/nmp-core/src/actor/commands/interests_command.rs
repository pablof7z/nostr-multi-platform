//! `InterestsCommand` — subscription registry + pull cursors
//! (ADR-0076 M2 / ADR-0072).
//!
//! Grouped under `ActorCommand::Interests(InterestsCommand)`. Dispatch home:
//! `actor/dispatch/cmd_interests.rs`.

use super::super::ObservedProjectionId;

/// Subscription-registry verbs: logical interests + pull cursors + the M2
/// FFI-facing feed-subscription front door.
///
/// The kernel's subscription lifecycle registry is the single writer for live
/// relay subscriptions (D4). Each variant either mutates the registry
/// (`EnsureInterest` / `ApplyDependentInterestDelta` /
/// `DropInterestOwner` / `OpenInterest` / `OpenObservedInterest` /
/// `CloseInterest`) or the pull-cursor registry (`OpenPullCursor` /
/// `AdvancePullCursor` / `UnregisterPullCursor`).
#[derive(Debug)]
pub enum InterestsCommand {
    /// Attach one owner to a logical interest using the registry's
    /// `(owner, key, scope)` identity. Multiple owners sharing the same key
    /// keep one live subscription until the last owner is dropped.
    EnsureInterest {
        identity: crate::subs::SubIdentity,
        interest: crate::planner::LogicalInterest,
    },
    /// Apply exact child-interest open/replace/close commands for one reduced
    /// source owner. Private reconcilers use this when their resource plan is
    /// authoritative and a full set replacement would discard useful diff
    /// precision.
    ApplyDependentInterestDelta {
        owner: crate::subs::SubOwnerKey,
        delta: crate::kernel::DependentInterestDelta,
        reason: String,
    },
    /// Detach one owner from a logical interest registered through
    /// [`Self::EnsureInterest`].
    DropInterestOwner(crate::subs::SubIdentity),
    /// ADR-0072 §10, step 3a — open (or replace-by-id) a pull cursor in the
    /// non-durable cursor registry. Fire-and-forget. The host mints the
    /// `handle` via the kernel's cursor-allocation doorway (#1897). Arms an
    /// immediate pull wake when the cursor is behind the store head.
    OpenPullCursor {
        handle: crate::kernel::pull_cursor::PullCursorHandle,
        spec: crate::kernel::pull_cursor::PullCursorSpec,
    },
    /// ADR-0072 §10, step 3a — monotonically advance a registered cursor's
    /// `after_seq` (`max(old, new)`). Fire-and-forget. Re-arms an immediate
    /// pull wake when the cursor is still behind the store head. Unknown id is
    /// a no-op.
    AdvancePullCursor {
        cursor_id: crate::kernel::pull_cursor::PullCursorId,
        after_seq: u64,
    },
    /// ADR-0072 §10, step 3a — remove a cursor's registry row AND any pending
    /// pull wake entry. Fire-and-forget.
    UnregisterPullCursor {
        cursor_id: crate::kernel::pull_cursor::PullCursorId,
    },
    /// M2 (ADR-0076) — the generic FFI-facing feed-subscription front door
    /// that replaced the bespoke `OpenAuthor` / `OpenThread` /
    /// `OpenFirehoseTag` variants. The host passes a verbatim NIP-01 REQ
    /// filter; the dispatch arm parses it into an `InterestShape`, builds a
    /// `SubIdentity`, and runs the same `registry_mut().ensure_sub` +
    /// `CompileTrigger` body as [`Self::EnsureInterest`]. Lifecycle is always
    /// `Tailing`.
    ///
    /// D0: `nmp-core` carries the filter as opaque shape data. The
    /// `InterestShape` hash gives deterministic dedup: two call sites passing
    /// the same filter (regardless of JSON key/element ordering) map to the
    /// same slot.
    OpenInterest {
        /// Verbatim NIP-01 REQ filter JSON, e.g. `{"kinds":[1],"#t":["nostr"]}`.
        filter_json: String,
        /// Refcount owner key — deduplicates the live subscription across call
        /// sites that register the same filter.
        consumer_id: String,
        /// `0` = `InterestScope::ActiveAccount` (re-route on account switch),
        /// `1` = `InterestScope::Global` (account-agnostic).
        scope: u32,
    },
    /// ADR-0070 — open an interest AND simultaneously replay matching in-memory
    /// cached events to a single muted observer, then activate it.
    ///
    /// This solves the late-joiner problem: a per-open feed observer that
    /// registers AFTER events have been accepted and cached by the kernel
    /// would otherwise miss those events (the global fan-out is one-shot).
    ///
    /// Protocol:
    /// 1. The caller registers the observer in **muted** state via
    ///    `register_rust_observer_muted`, capturing the returned id.
    /// 2. The caller sends `OpenObservedInterest` with that id and the shapes
    ///    to replay.
    /// 3. The actor dispatches: `open_interest_with_observer_replay` runs
    ///    `register_interest` (relay-subscribe), then replays `self.events`,
    ///    then calls `activate_observer`. From that point the observer
    ///    participates in the normal global fan-out.
    OpenObservedInterest {
        /// Verbatim NIP-01 REQ filter JSON — same semantic as [`Self::OpenInterest`].
        filter_json: String,
        /// Refcount owner key — same semantic as [`Self::OpenInterest`].
        consumer_id: String,
        /// Scope — same semantic as [`Self::OpenInterest`].
        scope: u32,
        /// Relay pin (out-of-band routing hint): when `Some`, the interest is
        /// routed to exactly this relay via the planner's relay-pin lane,
        /// bypassing NIP-65 outbox routing. NIP-50 search opens one pinned
        /// interest per resolved search relay; `None` is the normal
        /// outbox-routed path. The pin participates in the `InterestShape`
        /// hash, so the matching [`Self::CloseInterest`] MUST carry the same
        /// pin.
        relay_pin: Option<String>,
        /// When true, route this sparse global interest through the planner's
        /// indexer-discovery relay lane.
        is_indexer_discovery: bool,
        /// #2948 — close semantics for the compiled REQ. `Tailing` (the read
        /// default) keeps the sub live after EOSE; `OneShot` CLOSEs on EOSE so a
        /// pinned collection read completes and tears down. Does NOT participate
        /// in the subscription identity (the matching [`Self::CloseInterest`]
        /// needs no lifecycle).
        lifecycle: crate::planner::InterestLifecycle,
        /// The muted observer id to replay events to and then activate.
        observer_id: ObservedProjectionId,
        /// `InterestShape`s used to match events in the read-cache during
        /// replay. May differ from the filter (e.g. thread feed uses two
        /// shapes: `#e` replies + root-by-id).
        replay_shapes: Vec<crate::planner::InterestShape>,
        /// Maximum events to replay (newest-first selection, oldest-first
        /// delivery). Typically the feed's visible window limit.
        replay_limit: usize,
    },
    /// M2 (ADR-0076) — detach one owner from an interest registered via
    /// [`Self::OpenInterest`]. Drops the live subscription when the last owner
    /// leaves (mirrors [`Self::DropInterestOwner`]).
    CloseInterest {
        filter_json: String,
        consumer_id: String,
        scope: u32,
        /// Relay pin matching the open (NIP-50 search / NIP-29 groups). MUST
        /// equal the pin the corresponding [`Self::OpenInterest`] /
        /// [`Self::OpenObservedInterest`] used, so the reconstructed
        /// `InterestShape` hash lands on the same registry slot. `None` for
        /// the normal outbox-routed path.
        relay_pin: Option<String>,
        /// Must match the corresponding open. It participates in the
        /// subscription identity because it changes planner routing.
        is_indexer_discovery: bool,
    },
}
