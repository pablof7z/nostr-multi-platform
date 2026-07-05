//! `CachedMintInfo` — split out of `state.rs` (AGENTS.md LOC discipline),
//! mirroring `state_pending_deposit.rs`/`state_nutzap_await.rs`.
//!
//! This is the actor-side CACHE `mint_info.rs`'s off-projection-path fetch
//! writes into and `snapshot.rs`'s `mint_info_rows` reads from — never
//! mutated by the typed-projection producer closure itself (D8; see
//! `mint_info.rs`'s module docs for the fetch/trigger seam).

use crate::projection::{WalletMintFeeRow, WalletMintInfoRow, MAX_MINT_UNITS};

/// One mint's cached raw NUT-06/NUT-02 metadata, keyed by canonical mint URL
/// in [`super::CashuWalletState::mint_info`]. Field-for-field the same shape
/// [`WalletMintInfoRow`] carries minus the URL (the map key already is the
/// URL) — this type exists only so [`Self::to_row`] can construct the wire
/// row without re-deriving `input_fee_ppk_by_unit`'s `Vec<(String, u64)>` ->
/// `Vec<WalletMintFeeRow>` shape at every call site.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub(in crate::backend::cashu) struct CachedMintInfo {
    pub(in crate::backend::cashu) name: Option<String>,
    pub(in crate::backend::cashu) icon_url: Option<String>,
    /// Units this mint advertises active keysets for, sorted (see
    /// `mint_info::fetch_one` — derived from `fees_by_unit`'s `BTreeMap` keys
    /// so this is always in the same order that map iterates, deterministic
    /// across fetches).
    pub(in crate::backend::cashu) units: Vec<String>,
    pub(in crate::backend::cashu) fees_by_unit: Vec<(String, u64)>,
}

impl CachedMintInfo {
    /// Build the wire-facing [`WalletMintInfoRow`] for `url` (the cache's map
    /// key, not stored redundantly on this type itself).
    ///
    /// The per-row nested vectors are clamped to [`MAX_MINT_UNITS`] (D5 —
    /// bounded snapshot across FFI): `units` and `fees_by_unit` are both in
    /// the same canonical (sorted-by-unit) order `fetch_one` built them in,
    /// so taking the first `MAX_MINT_UNITS` of each keeps them consistent and
    /// deterministic — a mint advertising a pathologically large keyset list
    /// can never bloat one row.
    pub(in crate::backend::cashu) fn to_row(&self, url: String) -> WalletMintInfoRow {
        WalletMintInfoRow {
            url,
            name: self.name.clone(),
            icon_url: self.icon_url.clone(),
            units: self.units.iter().take(MAX_MINT_UNITS).cloned().collect(),
            input_fee_ppk_by_unit: self
                .fees_by_unit
                .iter()
                .take(MAX_MINT_UNITS)
                .map(|(unit, input_fee_ppk)| WalletMintFeeRow {
                    unit: unit.clone(),
                    input_fee_ppk: *input_fee_ppk,
                })
                .collect(),
        }
    }
}
