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

use nmp_planner::InterestShape;

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
}
