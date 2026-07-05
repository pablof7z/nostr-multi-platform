//! #3030 PR2 of 2 — `snapshot::mint_info_rows`'s relevant-mint-URL join
//! against `state.mint_info`'s cache, and `CashuWalletBackend::snapshot()`'s
//! end-to-end wiring of the resulting `WalletMintInfoRow`s. Never drives a
//! real mint HTTP fetch here — these tests seed `state.mint_info` directly
//! (exactly as production code would once `mint_info::run_mint_info_refresh`
//! has run on its own worker thread), proving the SNAPSHOT-side read/join
//! logic in isolation from the fetch itself (which has its own tests
//! colocated with `mint_info.rs`, mirroring `check_state.rs`).

use crate::projection::{WalletMintFeeRow, WalletMintInfoRow, MAX_MINT_UNITS};

use super::*;

fn cached(name: &str, icon_url: &str, fees: &[(&str, u64)]) -> state::CachedMintInfo {
    state::CachedMintInfo {
        name: Some(name.to_string()),
        icon_url: Some(icon_url.to_string()),
        units: fees.iter().map(|(unit, _)| unit.to_string()).collect(),
        fees_by_unit: fees
            .iter()
            .map(|(unit, fee)| (unit.to_string(), *fee))
            .collect(),
    }
}

/// A mint that is both accepted AND cached must surface a row carrying its
/// name/icon/fees.
#[test]
fn accepted_mint_with_cached_info_yields_a_row() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![MINT.to_string()];
        state
            .mint_info
            .insert(MINT.to_string(), cached("Test Mint", "https://icon", &[("sat", 100)]));
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert_eq!(snapshot.projection.mint_info.len(), 1);
    let row = &snapshot.projection.mint_info[0];
    assert_eq!(row.url, MINT);
    assert_eq!(row.name.as_deref(), Some("Test Mint"));
    assert_eq!(row.icon_url.as_deref(), Some("https://icon"));
    assert_eq!(
        row.input_fee_ppk_by_unit,
        vec![WalletMintFeeRow {
            unit: "sat".to_string(),
            input_fee_ppk: 100
        }]
    );
}

/// An accepted mint with NO cache entry yet (never successfully fetched)
/// yields NO row at all — never an error, never a placeholder row.
#[test]
fn accepted_mint_with_no_cached_info_yields_no_row() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![MINT.to_string()];
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert!(snapshot.projection.mint_info.is_empty());
}

/// A mint this wallet holds a BALANCE at (folded via `ingest_token_event`,
/// independent of `state.mints`) is still "wallet-relevant" and must surface
/// a cached row — the mint-info table is not scoped to `accepted_mints`
/// alone.
#[test]
fn mint_with_only_a_balance_still_yields_a_cached_row() {
    let backend = CashuWalletBackend::new();
    let balance_mint = "https://balance-only.example";
    let proof = synthetic_proof(21, "c-balance-only");
    let plaintext = serde_json::json!({
        "mint": balance_mint,
        "proofs": [proof],
        "del": Vec::<String>::new(),
    })
    .to_string();
    ingest::ingest_token_event(&backend.state, "tok-1", &plaintext, "")
        .expect("ingest must succeed");
    state::lock_state(&backend.state)
        .mint_info
        .insert(balance_mint.to_string(), cached("Balance Mint", "https://icon2", &[]));

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert!(snapshot
        .projection
        .mint_info
        .iter()
        .any(|row| row.url == balance_mint && row.name.as_deref() == Some("Balance Mint")));
}

/// A cache entry for a mint that is NEITHER accepted, NOR balance-holding,
/// NOR named by any history/receive row is stale — irrelevant to THIS
/// wallet's current state — and must not leak into the projection.
#[test]
fn stale_cache_entry_for_an_irrelevant_mint_is_not_surfaced() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![MINT.to_string()];
        state
            .mint_info
            .insert(MINT.to_string(), cached("Test Mint", "https://icon", &[]));
        state.mint_info.insert(
            "https://long-gone.example".to_string(),
            cached("Stale Mint", "https://stale-icon", &[]),
        );
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert_eq!(snapshot.projection.mint_info.len(), 1);
    assert_eq!(snapshot.projection.mint_info[0].url, MINT);
}

/// Deterministic order: two cached, relevant mints must always encode in the
/// same (sorted-by-URL) order regardless of insertion order — required for
/// the byte-equality emission compare (`projections-and-emission.md`).
#[test]
fn mint_info_rows_are_sorted_by_url_regardless_of_insertion_order() {
    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![
            "https://zzz.example".to_string(),
            "https://aaa.example".to_string(),
        ];
        state
            .mint_info
            .insert("https://zzz.example".to_string(), cached("Zzz", "", &[]));
        state
            .mint_info
            .insert("https://aaa.example".to_string(), cached("Aaa", "", &[]));
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let urls: Vec<&str> = snapshot
        .projection
        .mint_info
        .iter()
        .map(|row| row.url.as_str())
        .collect();
    assert_eq!(urls, vec!["https://aaa.example", "https://zzz.example"]);
}

/// Bare `WalletProjection::empty()`-shaped fresh backend: no mints, no
/// balances, no cache — `mint_info` is simply empty, never populated with
/// placeholder rows.
#[test]
fn fresh_backend_has_empty_mint_info() {
    let backend = CashuWalletBackend::new();
    let snapshot = backend.snapshot(WalletProjectionScope::default());
    assert!(snapshot.projection.mint_info.is_empty());
}

/// D5 per-row bound (#3030 PR2 of 2): a mint advertising more than
/// `MAX_MINT_UNITS` units/keysets must have BOTH its `units` and
/// `input_fee_ppk_by_unit` vectors clamped to the first `MAX_MINT_UNITS`
/// (in the canonical sorted order the cache holds them), so one pathological
/// mint can never bloat a single row across FFI.
#[test]
fn a_row_with_too_many_units_is_clamped_to_the_cap() {
    // Build a cache entry with 2x the cap's worth of units, named so their
    // sorted order is predictable: u000, u001, ... u031.
    let over = MAX_MINT_UNITS * 2;
    let unit_names: Vec<String> = (0..over).map(|i| format!("u{i:03}")).collect();
    let cached_over = state::CachedMintInfo {
        name: Some("Pathological Mint".to_string()),
        icon_url: None,
        units: unit_names.clone(),
        fees_by_unit: unit_names
            .iter()
            .enumerate()
            .map(|(i, unit)| (unit.clone(), i as u64))
            .collect(),
    };

    let backend = CashuWalletBackend::new();
    {
        let mut state = state::lock_state(&backend.state);
        state.mints = vec![MINT.to_string()];
        state.mint_info.insert(MINT.to_string(), cached_over);
    }

    let snapshot = backend.snapshot(WalletProjectionScope::default());
    let row = &snapshot.projection.mint_info[0];
    assert_eq!(row.units.len(), MAX_MINT_UNITS, "units clamped to the cap");
    assert_eq!(
        row.input_fee_ppk_by_unit.len(),
        MAX_MINT_UNITS,
        "fee rows clamped to the cap"
    );
    // Deterministic: the FIRST cap-many in sorted order (u000..).
    assert_eq!(row.units[0], "u000");
    assert_eq!(row.units[MAX_MINT_UNITS - 1], format!("u{:03}", MAX_MINT_UNITS - 1));
    assert_eq!(row.input_fee_ppk_by_unit[0].unit, "u000");
    assert_eq!(row.input_fee_ppk_by_unit[0].input_fee_ppk, 0);
}

/// Direct unit test of `snapshot::mint_info_rows` (bypassing the whole
/// backend snapshot) covering nested multi-unit fee rows end to end.
#[test]
fn mint_info_rows_carries_multi_unit_fees() {
    let mut state = state::CashuWalletState::new();
    state.mints = vec![MINT.to_string()];
    state.mint_info.insert(
        MINT.to_string(),
        cached("Multi Unit Mint", "https://icon", &[("sat", 50), ("usd", 10)]),
    );

    let rows = super::super::snapshot::mint_info_rows(&state, &[], &[], &[]);
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0],
        WalletMintInfoRow {
            url: MINT.to_string(),
            name: Some("Multi Unit Mint".to_string()),
            icon_url: Some("https://icon".to_string()),
            units: vec!["sat".to_string(), "usd".to_string()],
            input_fee_ppk_by_unit: vec![
                WalletMintFeeRow {
                    unit: "sat".to_string(),
                    input_fee_ppk: 50
                },
                WalletMintFeeRow {
                    unit: "usd".to_string(),
                    input_fee_ppk: 10
                },
            ],
        }
    );
}
