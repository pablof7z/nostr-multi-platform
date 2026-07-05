//! Read-interest shapes for the wallet's self-authored NIP-60/NIP-61 events
//! and inbound nutzap receipts (epic #2864 W6).
//!
//! Mirrors `nmp-nip57::interests`/`nmp-nip17`'s per-crate convention: a
//! protocol/product crate owns its own subscription shapes rather than
//! folding into the kernel's framework-default bootstrap self-kinds list
//! (`nmp-core`'s `SELF_KINDS_TAILING`), which stays wallet-noun-free (D0) —
//! `nmp-core` must not learn Cashu/NWC/mint kind numbers.
//!
//! # Why kind:9321 is `#p`-only, not `#p` + `#u`
//!
//! NIP-61 nutzap receipts (`kind:9321`) are validated against the active
//! account's *accepted mint set*, which grows at runtime as the Cashu backend
//! creates/recovers a wallet — unlike the fixed shapes below, it is not known
//! at interest-open time and can change after. `ObservedProjection`'s filter
//! is fixed for the life of the registration (close+reopen is the only way to
//! change it), so narrowing the relay-side filter to `#u ∈ accepted_mints`
//! would mean closing and reopening this interest on every mint acceptance —
//! and would still need the SAME in-Rust "is this mint accepted" check on
//! receipt for the redemption invariant (nip60-nip61-wallet-design.md,
//! "Receiving": verify mint + P2PK lock + DLEQ before counting a nutzap as
//! valid). So the relay-side filter stays the coarser, stable `#p = self`
//! shape, and the observer enforces the mint-acceptance check unconditionally
//! at receive time regardless of what the relay chose to send.

use std::collections::BTreeSet;

use nmp_core::subs::{SubIdentity, SubKey, SubOwnerKey, SubScope};
use nmp_planner::stable_hash::stable_hash64;
use nmp_planner::{InterestId, InterestLifecycle, InterestScope, InterestShape, LogicalInterest};

use nmp_nip60::kinds::{
    KIND_NIP60_HISTORY, KIND_NIP60_QUOTE, KIND_NIP60_TOKEN, KIND_NIP60_WALLET, KIND_NIP61_NUTZAP,
    KIND_NIP61_NUTZAP_INFO,
};

/// Self-authored NIP-60 wallet/token/history/quote events plus the account's
/// own published NIP-61 nutzap info (`kind:10019`) — all replaceable or
/// addressable, all authored by the active account, no tag filter needed.
#[must_use]
pub fn wallet_self_authored_shape(pubkey: &str) -> InterestShape {
    InterestShape {
        authors: BTreeSet::from([pubkey.to_string()]),
        kinds: BTreeSet::from([
            KIND_NIP60_WALLET,
            KIND_NIP60_TOKEN,
            KIND_NIP60_HISTORY,
            KIND_NIP60_QUOTE,
            KIND_NIP61_NUTZAP_INFO,
        ]),
        ..Default::default()
    }
}

/// Inbound `kind:9321` nutzaps `#p`-tagged to the active account. See the
/// module doc for why this is `#p`-only rather than also filtering `#u`.
#[must_use]
pub fn nutzap_receipts_shape(pubkey: &str) -> InterestShape {
    InterestShape {
        kinds: BTreeSet::from([KIND_NIP61_NUTZAP]),
        tags: [("p".to_string(), BTreeSet::from([pubkey.to_string()]))]
            .into_iter()
            .collect(),
        ..Default::default()
    }
}

/// Stable id for a recipient kind:10019 lookup subscription — folds the
/// same `("wallet.recipient_nutzap_info", pubkey)` tuple `identity` below
/// hashes, so id and identity always agree (mirrors
/// `nmp-marmot::interest::key_package_lookup_interest_id`).
fn recipient_nutzap_info_interest_id(pubkey: &str) -> InterestId {
    InterestId(stable_hash64(("wallet.recipient_nutzap_info", pubkey)))
}

/// On-demand lookup of ANOTHER account's kind:10019 nutzap info (mints,
/// relays, Cashu P2PK pubkey) — opened by `send.rs` the first time
/// `SendNutzap` targets a recipient this account has never cached a
/// kind:10019 from (#2936, epic #2864). `wallet_self_authored_shape` above
/// only ever subscribes to the ACTIVE account's own kind:10019; this is the
/// mirror-image shape for a THIRD PARTY's, keyed by the recipient's pubkey.
///
/// `Global` scope + `Tailing` lifecycle (not `OneShot`): kind:10019 is a
/// replaceable event a recipient may update (rotate mints/relays/P2PK key)
/// after this account first resolves it, and `ensure_interest` is
/// register-if-absent — an already-registered `OneShot` interest's wire REQ
/// closes at EOSE and won't reopen on a bare retry, so a later republish
/// would never be observed. `Tailing` keeps the recipient's kind:10019
/// current for as long as this account keeps sending them nutzaps. No
/// `limit`: it's a single replaceable kind, and the planner's own
/// replaceable-kind precedent leaves `limit` unset rather than assuming a
/// single-event cap (a strict relay may still coalesce to the latest copy).
///
/// No explicit teardown/`drop_interest_owner` — same accumulate-by-identity
/// tradeoff `nmp-marmot`'s `key_package_lookup_interest` already ships with:
/// one global tail per distinct recipient pubkey, deduped by
/// `recipient_nutzap_info_identity` so re-sending to the same recipient
/// never registers a second interest.
#[must_use]
pub fn recipient_nutzap_info_interest(pubkey: &str) -> LogicalInterest {
    LogicalInterest {
        id: recipient_nutzap_info_interest_id(pubkey),
        scope: InterestScope::Global,
        shape: InterestShape {
            authors: BTreeSet::from([pubkey.to_string()]),
            kinds: BTreeSet::from([KIND_NIP61_NUTZAP_INFO]),
            ..Default::default()
        },
        lifecycle: InterestLifecycle::Tailing,
        ..Default::default()
    }
}

/// Scoped registry identity for [`recipient_nutzap_info_interest`] — same
/// `("wallet.recipient_nutzap_info", pubkey)` key tuple as the interest id,
/// so `ensure_interest` dedupes repeat lookups for the same recipient.
#[must_use]
pub fn recipient_nutzap_info_identity(pubkey: &str) -> SubIdentity {
    SubIdentity::new(
        SubOwnerKey::new(("wallet.recipient_nutzap_info", pubkey)),
        SubKey::new(("wallet.recipient_nutzap_info", pubkey)),
        SubScope::Global,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const PK: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn wallet_self_authored_shape_scopes_to_the_account_and_its_kinds() {
        let shape = wallet_self_authored_shape(PK);
        assert_eq!(shape.authors, BTreeSet::from([PK.to_string()]));
        for kind in [
            KIND_NIP60_WALLET,
            KIND_NIP60_TOKEN,
            KIND_NIP60_HISTORY,
            KIND_NIP60_QUOTE,
            KIND_NIP61_NUTZAP_INFO,
        ] {
            assert!(shape.kinds.contains(&kind), "missing kind {kind}");
        }
        assert!(shape.tags.is_empty());
    }

    #[test]
    fn nutzap_receipts_shape_is_p_tagged_only_no_u_filter() {
        let shape = nutzap_receipts_shape(PK);
        assert!(shape.kinds.contains(&KIND_NIP61_NUTZAP));
        let p_values = shape.tags.get("p").cloned().unwrap_or_default();
        assert!(p_values.contains(PK));
        assert!(
            !shape.tags.contains_key("u"),
            "mint acceptance must be checked in-Rust at receive time, not \
             narrowed at the relay filter (see module docs)"
        );
    }

    const RECIPIENT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    #[test]
    fn recipient_nutzap_info_interest_targets_only_the_recipients_kind_10019() {
        let interest = recipient_nutzap_info_interest(RECIPIENT);
        assert_eq!(interest.shape.authors, BTreeSet::from([RECIPIENT.to_string()]));
        assert_eq!(interest.shape.kinds, BTreeSet::from([KIND_NIP61_NUTZAP_INFO]));
        assert!(interest.shape.tags.is_empty());
        assert!(interest.shape.limit.is_none());
        assert!(matches!(interest.lifecycle, InterestLifecycle::Tailing));
        assert!(matches!(interest.scope, InterestScope::Global));
        assert_eq!(interest.id, recipient_nutzap_info_interest_id(RECIPIENT));
    }

    #[test]
    fn recipient_nutzap_info_interest_id_is_deterministic_per_pubkey() {
        assert_eq!(
            recipient_nutzap_info_interest_id(RECIPIENT),
            recipient_nutzap_info_interest_id(RECIPIENT)
        );
        assert_ne!(
            recipient_nutzap_info_interest_id(RECIPIENT),
            recipient_nutzap_info_interest_id(PK)
        );
    }

    #[test]
    fn recipient_nutzap_info_identity_dedupes_the_same_recipient() {
        let a = recipient_nutzap_info_identity(RECIPIENT);
        let b = recipient_nutzap_info_identity(RECIPIENT);
        assert_eq!(a, b, "same recipient must produce the same registry identity");
        let other = recipient_nutzap_info_identity(PK);
        assert_ne!(a, other);
    }
}
