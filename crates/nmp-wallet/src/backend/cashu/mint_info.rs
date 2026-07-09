//! Mint-info refresh (#3030, PR2 of 2) — fetches each wallet-relevant mint's
//! raw NUT-06 (`/v1/info`) + NUT-02 (`/v1/keys` + `/v1/keysets`) metadata and
//! caches it in [`CashuWalletState::mint_info`], so `snapshot.rs`'s
//! `mint_info_rows` can build [`crate::projection::WalletMintInfoRow`]s from
//! a plain in-memory read — never from a live fetch.
//!
//! # Off the projection-emit path (D8)
//!
//! [`run_mint_info_refresh`] blocks on one `/v1/info` + up to two `/v1/keys`/
//! `/v1/keysets` HTTP round-trips per mint. Exactly like
//! [`super::check_state::run_check_state_pass`], it is meant to run on its
//! own `std::thread` — [`spawn_mint_info_refresh`] is that spawner, with the
//! same single-flight coalescing shape `check_state::spawn_debounced` uses
//! (`CashuWalletState::mint_info_in_flight`/`mint_info_pending`). The typed
//! `"wallet.merged"` snapshot producer (`register.rs`'s
//! `wallet_merged_typed_projection` closure, wired through
//! `CashuWalletBackend::snapshot` -> `snapshot::mint_info_rows`) NEVER calls
//! into this module — it only ever reads the cache `run_mint_info_refresh`
//! already populated. This is the same off-hot-path seam `deposit/quote.rs`'s
//! `CashuDepositQuoteCommand` and `check_state.rs` already established for
//! mint HTTP: `ProtocolCommand::run()`/an `on_signed` continuation captures
//! what it needs synchronously (here: just the mint URL list + the shared
//! `Arc<Mutex<CashuWalletState>>`), then spawns a worker thread that blocks
//! on `MintClient` and writes its result back into that shared state
//! directly — no second actor round-trip, no I/O in any registered closure.
//!
//! # Trigger points
//!
//! [`spawn_mint_info_refresh`] is called from every place `CashuWalletState`'s
//! accepted-mint list can change or first become known:
//!
//! - `create_wallet.rs`'s `CreateCashuWalletCommand::on_signed` (a brand-new
//!   wallet's mint).
//! - `set_mints.rs`'s `SetCashuMintsCommand::on_signed` (the accepted-mint
//!   list changed).
//! - `ingest.rs`'s `build_passive_ingest_command` (cold-start/live-tail
//!   replay of the account's own kind:17375 — "once on launch").
//! - `recover.rs`'s `RecoverCashuWalletCommand` (both the fresh-decrypt branch
//!   and the already-loaded idempotent branch — "once on recover").
//!
//! # Graceful degradation
//!
//! A total fetch failure for a mint (network error, non-2xx, unparsable
//! body) leaves that mint's existing cache entry untouched — never an error
//! surfaced to a caller, never a `ShowErrorToken`/action-ledger report (this
//! is a passive cache refresh, not a user-initiated operation). A mint this
//! wallet cares about but has never been successfully fetched simply has no
//! cache entry, which `snapshot.rs`'s `mint_info_rows` treats as "no row",
//! never an error.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use nmp_nip60::cashu::{canonicalize_mint_url, MintClient};

use super::state::{lock_state, CachedMintInfo, CashuWalletState};

/// Coalescing entry point — mirrors `check_state::spawn_debounced` exactly,
/// except the "batch" carries the specific mint URLs to (re)fetch rather than
/// being an unconditional pass over everything held. A trigger that arrives
/// while a pass is already running merges its mints into the pending batch
/// (deduped) rather than spawning a second concurrent pass.
pub(super) fn spawn_mint_info_refresh(state: Arc<Mutex<CashuWalletState>>, mints: Vec<String>) {
    if mints.is_empty() {
        return;
    }
    let should_spawn = {
        let mut s = lock_state(&state);
        if s.mint_info_in_flight {
            let pending = s.mint_info_pending.get_or_insert_with(Vec::new);
            for mint in &mints {
                // Dedup by canonical form, not raw string equality — two
                // triggers naming the same real mint with a differently-cased
                // host or trailing slash must not queue a redundant fetch.
                let canonical = canonicalize_mint_url(mint);
                if !pending.iter().any(|p| canonicalize_mint_url(p) == canonical) {
                    pending.push(mint.clone());
                }
            }
            false
        } else {
            s.mint_info_in_flight = true;
            true
        }
    };
    if !should_spawn {
        return;
    }
    std::thread::spawn(move || {
        let mut batch = mints;
        loop {
            run_mint_info_refresh(&state, &batch);
            let mut s = lock_state(&state);
            if let Some(pending) = s.mint_info_pending.take() {
                batch = pending;
                continue;
            }
            s.mint_info_in_flight = false;
            break;
        }
    });
}

/// Fetch + cache raw mint info for every URL in `mints`. Blocking — see
/// module docs for why this must run off the projection-emit path. A
/// per-mint failure (see [`fetch_one`]) leaves that mint's existing cache
/// entry untouched rather than clearing or erroring it.
pub(super) fn run_mint_info_refresh(state: &Mutex<CashuWalletState>, mints: &[String]) {
    for mint in mints {
        let canonical = canonicalize_mint_url(mint);
        if let Some(cached) = fetch_one(&canonical) {
            let mut s = lock_state(state);
            s.mint_info.insert(canonical, cached);
        }
    }
}

/// Fetch one mint's raw `/v1/info` + `/v1/keys`/`/v1/keysets` and fold them
/// into a [`CachedMintInfo`]. `None` only when EVERY call failed (nothing
/// learned at all) — a partial result (e.g. `/v1/info` succeeds but keysets
/// fails, or vice versa) still yields `Some` with whichever half is known,
/// the same best-effort posture `MintClient::get_sat_keyset`/
/// `get_keysets_with_fees` already apply to their own internal `/v1/keysets`
/// merge.
fn fetch_one(mint: &str) -> Option<CachedMintInfo> {
    let client = MintClient::new(mint);
    let info = client.get_mint_info().ok();
    let keysets = client.get_keysets_with_fees().ok();
    if info.is_none() && keysets.is_none() {
        return None;
    }

    // BTreeMap: last active keyset for a unit wins (mints rarely run two
    // active keysets for the same unit at once), AND iteration order is
    // sorted by unit name — `units` below is derived from these keys so it
    // is deterministic across fetches, never HTTP-response-order-dependent.
    let mut fees_by_unit: BTreeMap<String, u64> = BTreeMap::new();
    for keyset in keysets.into_iter().flatten() {
        fees_by_unit.insert(keyset.unit, keyset.input_fee_ppk);
    }
    let units: Vec<String> = fees_by_unit.keys().cloned().collect();

    Some(CachedMintInfo {
        name: info.as_ref().and_then(|i| i.name.clone()),
        icon_url: info.and_then(|i| i.icon_url),
        units,
        fees_by_unit: fees_by_unit.into_iter().collect(),
    })
}

#[cfg(test)]
#[path = "tests/mint_info_tests.rs"]
mod tests;
