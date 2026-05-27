//! `CompileTrigger` enum + auxiliary types.
//!
//! See `docs/design/subscription-compilation/recompilation.md` §4.1 for the
//! canonical enum. This module ships the seam shape only — the actual
//! reconciler (M4), NIP-42 handshake (M5), publisher (M7), and multi-account
//! session machine (M8) emit triggers into the inbox; this module does not
//! implement their semantics.
//!
//! ## Type aliases
//!
//! `AccountId` and `SignerId` are opaque newtypes so the M6/M8 implementations
//! can substitute richer types without breaking this module's API. The
//! `RelayAuthState` enum is the seam for M5 — T40 may add variants (e.g.
//! `Failed { reason }`) without breaking the trigger ABI.

use crate::planner::{InterestId, RelayUrl};

// ─── Opaque newtypes (M6/M8 will substitute) ────────────────────────────────

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct AccountId(pub String);

#[derive(Clone, Debug, Hash, Eq, PartialEq, Ord, PartialOrd)]
pub struct SignerId(pub String);

// ─── RelayAuthState (T77 — single substrate type) ───────────────────────────

/// Per-relay NIP-42 auth state.
///
/// **T77:** this was a kernel-local placeholder enum kept variant-identical
/// to `nmp_nip42::state::RelayAuthState` by a hand-written translation
/// function. It is now the single substrate type from `nmp-nip42-types`,
/// re-exported here so every existing `crate::subs::RelayAuthState` call
/// site (and the `subs::mod` re-export consumed by the kernel) is
/// unchanged. M8-subs branches on `ChallengeReceived` / `Authenticating`
/// (paused) and `Authenticated` (flush); `Failed` is fail-closed (ADR-0019).
pub use nmp_nip42_types::RelayAuthState;

// ─── InvalidateReason (A6) ──────────────────────────────────────────────────

/// Why an external `InvalidateCompile` trigger was emitted.
///
/// Listed for diagnostic provenance only — the compiler treats them identically.
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub enum InvalidateReason {
    /// Operator pressed "Force re-route now" in diagnostics.
    DiagnosticsManualRefresh,
    /// Test harness force-recompile (the audit gate, the integration tests).
    TestForceRecompile,
    /// Catch-all with a diagnostic string.
    External(String),
}

// ─── CompileTrigger ─────────────────────────────────────────────────────────

/// The eleven canonical recompilation triggers from
/// `docs/design/subscription-compilation/recompilation.md` §4.1.
///
/// All triggers fan into the actor's trigger inbox. Per-tick coalescing
/// guarantees ≤ 1 compile per tick regardless of fan-in (D8).
#[derive(Clone, Debug)]
pub enum CompileTrigger {
    /// A1 — kind:10002 just replaced an author's mailbox entry.
    Nip65Arrived { pubkey: String, created_at: u64 },
    /// A1-DM — kind:10050 just replaced an author's NIP-17 DM-relay list.
    ///
    /// This re-routes only interests whose `#p` routing mode explicitly asks
    /// for kind:10050 DM relays. Generic `#p` interests continue to use
    /// kind:10002 read relays.
    DmRelayListChanged { pubkey: String, created_at: u64 },
    /// A2 — view registered one or more interests.
    ViewOpened { interest_ids: Vec<InterestId> },
    /// A3 — view's warmth grace expired; interests dropped.
    ViewClosed {
        interest_ids: Vec<InterestId>,
        warmth_expired_at_ms: u64,
    },
    /// A4 — active account changed (M8 multi-account).
    ActiveAccountChanged {
        from: Option<AccountId>,
        to: Option<AccountId>,
    },
    /// A5 — relay reconnected after backoff. Pure replay; not a recompile.
    RelayReconnected { url: RelayUrl },
    /// A6 — external force-recompile.
    InvalidateCompile { reason: InvalidateReason },
    /// A7 — user-configured relay set changed.
    UserConfiguredRelaysChanged { generation: u64 },
    /// A8 — kernel-configured indexer set changed.
    IndexerSetChanged { generation: u64 },
    /// A9 — NIP-42 auth-state transition (M5 / T40 seam).
    RelayAuthStateChanged {
        url: RelayUrl,
        state: RelayAuthState,
    },
    /// A10 — signer became available for an account (M6 / T43 seam).
    SignerAvailable {
        account: AccountId,
        signer_id: SignerId,
    },
    /// A12 — a relay was marked dead or alive in the lifecycle's
    /// `dead_relays` set. The selector will exclude dead relays from its
    /// candidate set on the next recompile, forcing affected authors onto
    /// alternative NIP-65 write relays. Symmetric: re-marking a relay alive
    /// also fires this trigger so authors can route back to it.
    RelayHealthChanged { url: RelayUrl, dead: bool },
    /// A11 — active account's kind:3 contact list replaced with a fresher
    /// event. Emitted by the kind:3 ingest fan after `Inserted | Replaced`
    /// from the event store (D4). The compiler re-runs every view whose
    /// `dependencies()` declares `kind 3` or whose `interests()` consumes the
    /// active account's follow-set as a filter shape input.
    ///
    /// `new_follows` is the extracted "p"-tagged pubkey vec from the fresher
    /// kind:3; callers may use it to update view-module author sets before
    /// triggering the recompile. The compiler itself does not inspect this
    /// field — it recompiles unconditionally when this trigger fires.
    FollowListChanged {
        account_id: AccountId,
        new_follows: Vec<String>,
    },
}

impl CompileTrigger {
    /// Returns true if this trigger requires invoking the compiler. False for
    /// pure-replay triggers (A5) which the lifecycle handles via
    /// `handle_reconnect` directly.
    ///
    /// Kept `pub` so that the M4 / M5 / M7 in-flight tasks (T39/T40/T45) can
    /// classify triggers as they fan them into the inbox without re-encoding
    /// the rule.
    #[allow(dead_code)]
    #[must_use]
    pub fn requires_recompile(&self) -> bool {
        !matches!(self, Self::RelayReconnected { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_does_not_require_recompile() {
        let t = CompileTrigger::RelayReconnected {
            url: "wss://a".to_string(),
        };
        assert!(!t.requires_recompile());
    }

    #[test]
    fn invalidate_compile_requires_recompile() {
        let t = CompileTrigger::InvalidateCompile {
            reason: InvalidateReason::TestForceRecompile,
        };
        assert!(t.requires_recompile());
    }

    #[test]
    fn follow_list_changed_requires_recompile() {
        let t = CompileTrigger::FollowListChanged {
            account_id: AccountId("alice".to_string()),
            new_follows: vec!["bob".to_string()],
        };
        assert!(t.requires_recompile());
    }
}
