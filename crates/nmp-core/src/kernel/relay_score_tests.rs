//! Relay-search-radius scoring tests.
//!
//! Cases:
//! 1. `weight_zero_for_unknown_cell`
//! 2. `weight_above_threshold_after_clean_hit` (EoseNoMatch is neutral)
//! 3. `weight_unchanged_after_eose_no_match`
//! 4. `decay_halves_weight_at_14_days`
//! 5. `record_hit_sets_now_and_increments`
//! 6. `record_failure_increments_by_three`
//! 7. `canonicalization_consolidates_trailing_slash_to_one_cell` (§8.10)
//! 8. `saturating_add_does_not_panic_at_u32_max`
//!
//! Test scope: `cargo test -p nmp-core --test relay_score_tests`.
//! (Same scoping rule as the rest of the crate's `#[cfg(test)]` modules
//! per CLAUDE.md.)

use super::relay_score::{
    canon, ClaimOutcome, RelayAuthorScore, RelayAuthorScoreMap, DECAY_HALFLIFE_DAYS, WARM_THRESHOLD,
};

const ALICE: &str = "alice_pubkey_hex";

/// Helper: `now_unix_s` baseline for tests — arbitrary recent timestamp
/// (2026-01-01T00:00:00Z).
const NOW: u64 = 1_767_225_600;

/// Helper: seconds in `n_days`.
fn days(n: u64) -> u64 {
    n * 86_400
}

#[test]
fn weight_zero_for_unknown_cell() {
    let s = RelayAuthorScore::default();
    // No hits, no failures → weight is 0 regardless of age.
    assert!((s.weight(NOW)).abs() < f32::EPSILON);
    assert!((s.weight(0)).abs() < f32::EPSILON);
}

#[test]
fn weight_above_threshold_after_clean_hit() {
    let mut s = RelayAuthorScore::default();
    s.record_hit(NOW);
    // One hit, zero failures, zero age → 1 / (1 + 0 + 1) = 0.5
    let w = s.weight(NOW);
    assert!(
        w > WARM_THRESHOLD,
        "weight {w} should exceed WARM_THRESHOLD={WARM_THRESHOLD}"
    );
    assert!((w - 0.5_f32).abs() < 1e-3, "weight {w} should be ~0.5");
    assert!(s.is_warm(NOW));
}

#[test]
fn weight_unchanged_after_eose_no_match() {
    // Scoring contract: EoseNoMatch is neutral. A relay that EOSEs without
    // matching does not lose its warm status — only its recency stamp
    // moves. The Gigi math (10/(40+10+1) ≈ 0.196 < 0.40) drove this.
    let mut s = RelayAuthorScore::default();
    s.record_hit(NOW);
    let w_after_hit = s.weight(NOW);
    let counters_after_hit = (s.successes, s.failures);

    s.record_eose_no_match(NOW + 60);
    let w_after_eose = s.weight(NOW + 60);
    let counters_after_eose = (s.successes, s.failures);

    assert_eq!(
        counters_after_hit, counters_after_eose,
        "EoseNoMatch must not change success/failure counters"
    );
    // Weight is nearly identical (only ~60 s of decay applied — negligible
    // against a 14-day half-life).
    let drift = (w_after_hit - w_after_eose).abs();
    assert!(drift < 0.01, "60s decay drift {drift} should be < 0.01");
    assert!(s.is_warm(NOW + 60));
}

#[test]
fn decay_halves_weight_at_14_days() {
    let mut s = RelayAuthorScore::default();
    s.record_hit(NOW);
    let w_now = s.weight(NOW);
    let w_14d = s.weight(NOW + days(DECAY_HALFLIFE_DAYS as u64));
    // After one half-life, weight should be ~50% of the fresh value.
    let ratio = w_14d / w_now;
    assert!(
        (ratio - 0.5_f32).abs() < 0.01,
        "14d ratio {ratio} should be ~0.5 (got w_now={w_now}, w_14d={w_14d})"
    );
}

#[test]
fn record_hit_sets_now_and_increments() {
    let mut s = RelayAuthorScore::default();
    s.record_hit(NOW);
    assert_eq!(s.successes, 1);
    assert_eq!(s.failures, 0);
    assert_eq!(s.last_used_unix_s, NOW);

    s.record_hit(NOW + 100);
    assert_eq!(s.successes, 2);
    assert_eq!(s.last_used_unix_s, NOW + 100);
}

#[test]
fn record_failure_increments_by_three() {
    let mut s = RelayAuthorScore::default();
    s.record_failure(NOW);
    assert_eq!(s.successes, 0);
    assert_eq!(s.failures, 3);
    assert_eq!(s.last_used_unix_s, NOW);
}

#[test]
fn canonicalization_consolidates_trailing_slash_to_one_cell() {
    // §8.10 — scoring under `wss://r.example/` and looking up under
    // `wss://r.example` must hit the same cell.
    let mut map = RelayAuthorScoreMap::new();
    let with_slash = "wss://relay.example.com/";
    let without_slash = "wss://relay.example.com";

    map.record(&ALICE.to_string(), with_slash, ClaimOutcome::Hit, NOW);

    // Lookup under the alternate spelling must hit the same cell.
    let cell = map.get(&ALICE.to_string(), without_slash);
    assert_eq!(
        cell.successes, 1,
        "trailing-slash cell must match no-slash cell"
    );
    assert!(map.is_warm(&ALICE.to_string(), without_slash, NOW));

    // And the map only contains one entry.
    assert_eq!(map.len(), 1, "two URL spellings must collapse to one cell");

    // Sanity: canonicalization function itself.
    assert_eq!(canon(with_slash), canon(without_slash));
}

#[test]
fn saturating_add_does_not_panic_at_u32_max() {
    // D6: even a pathological author/relay cell must never panic.
    let mut s = RelayAuthorScore {
        successes: u32::MAX,
        failures: u32::MAX,
        last_used_unix_s: NOW,
    };
    // Both helpers should be no-ops at saturation rather than panic.
    s.record_hit(NOW + 1);
    s.record_failure(NOW + 2);
    assert_eq!(s.successes, u32::MAX);
    assert_eq!(s.failures, u32::MAX);
}

#[test]
fn record_via_map_sets_dirty_flag() {
    let mut map = RelayAuthorScoreMap::new();
    assert!(!map.is_dirty());

    map.record(
        &ALICE.to_string(),
        "wss://r.example",
        ClaimOutcome::Hit,
        NOW,
    );
    assert!(map.is_dirty(), "record should set dirty");

    map.mark_clean();
    assert!(!map.is_dirty());
}

#[test]
fn bulk_load_does_not_set_dirty() {
    // W2's hydration path: loading from LMDB should not mark the map
    // dirty (those rows already exist in the store).
    let mut map = RelayAuthorScoreMap::new();
    let cells = vec![(
        ALICE.to_string(),
        "wss://r.example".to_string(),
        RelayAuthorScore {
            successes: 1,
            failures: 0,
            last_used_unix_s: NOW,
        },
    )];
    map.bulk_load(cells);
    assert!(
        !map.is_dirty(),
        "bulk_load is hydration, not a write — must stay clean"
    );
    assert!(map.is_warm(&ALICE.to_string(), "wss://r.example", NOW));
}

#[test]
fn eose_no_match_for_unknown_cell_does_not_create_row() {
    let mut map = RelayAuthorScoreMap::new();
    map.record(
        &ALICE.to_string(),
        "wss://niche.example",
        ClaimOutcome::EoseNoMatch,
        NOW,
    );
    assert_eq!(
        map.len(),
        0,
        "neutral EOSE must not create a zero-score row"
    );
    assert!(!map.is_dirty(), "no persisted state changed");
}

#[test]
fn snapshot_returns_all_cells() {
    let mut map = RelayAuthorScoreMap::new();
    map.record(&ALICE.to_string(), "wss://r1", ClaimOutcome::Hit, NOW);
    map.record(&ALICE.to_string(), "wss://r2", ClaimOutcome::Failed, NOW);

    let snap = map.snapshot();
    assert_eq!(snap.len(), 2);
}
