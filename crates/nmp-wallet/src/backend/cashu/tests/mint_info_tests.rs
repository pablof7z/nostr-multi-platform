//! `mint_info::run_mint_info_refresh`/`spawn_mint_info_refresh`/`fetch_one`
//! (#3030 PR2 of 2) — exercised against the same local mock mint
//! (`tests/mod.rs`'s `spawn_mock_mint`) `check_state_tests.rs` uses. `fetch_one`
//! calls, in order: `GET /v1/info` (`get_mint_info`), then
//! `get_keysets_with_fees` (`GET /v1/keys` then `GET /v1/keysets`) — so a mock
//! sequence for one mint needs exactly 3 queued responses in that order.

use crate::backend::cashu::tests::spawn_mock_mint;
use crate::backend::cashu::CashuWalletBackend;

use super::*;

const KEYS_BODY: &str =
    r#"{"keysets":[{"id":"00sat","unit":"sat","keys":{"1":"02aa"}}]}"#;
const KEYSETS_BODY: &str = r#"{"keysets":[{"id":"00sat","unit":"sat","input_fee_ppk":100}]}"#;
const INFO_BODY: &str =
    r#"{"name":"Test Mint","pubkey":"02deadbeef","icon_url":"https://mint.example/icon.png"}"#;

/// The full happy path: all 3 requests succeed, the cache gets a fully
/// populated entry keyed by the (canonicalized) mint URL.
#[test]
fn run_mint_info_refresh_populates_the_cache_on_success() {
    let mock_mint = spawn_mock_mint(vec![
        (200, INFO_BODY.to_string()),
        (200, KEYS_BODY.to_string()),
        (200, KEYSETS_BODY.to_string()),
    ]);

    let backend = CashuWalletBackend::new();
    run_mint_info_refresh(&backend.state, &[mock_mint.clone()]);

    let state = lock_state(&backend.state);
    let cached = state
        .mint_info
        .get(&canonicalize_mint_url(&mock_mint))
        .expect("mint info must be cached");
    assert_eq!(cached.name.as_deref(), Some("Test Mint"));
    assert_eq!(cached.icon_url.as_deref(), Some("https://mint.example/icon.png"));
    assert_eq!(cached.units, vec!["sat".to_string()]);
    assert_eq!(cached.fees_by_unit, vec![("sat".to_string(), 100)]);
}

/// A mint that errors on EVERY request (total failure) leaves no cache entry
/// at all — never a placeholder, never an error surfaced to a caller.
#[test]
fn a_total_fetch_failure_leaves_no_cache_entry() {
    let mock_mint = spawn_mock_mint(vec![
        (500, "{\"code\":1,\"detail\":\"boom\"}".to_string()),
        (500, "{\"code\":1,\"detail\":\"boom\"}".to_string()),
        (500, "{\"code\":1,\"detail\":\"boom\"}".to_string()),
    ]);

    let backend = CashuWalletBackend::new();
    run_mint_info_refresh(&backend.state, &[mock_mint.clone()]);

    let state = lock_state(&backend.state);
    assert!(state
        .mint_info
        .get(&canonicalize_mint_url(&mock_mint))
        .is_none());
}

/// A pre-existing cache entry must survive a subsequent failed refresh —
/// graceful degradation never clears/replaces known-good info with nothing.
#[test]
fn a_failed_refresh_leaves_a_pre_existing_cache_entry_untouched() {
    let mock_mint = spawn_mock_mint(vec![
        (500, "boom".to_string()),
        (500, "boom".to_string()),
        (500, "boom".to_string()),
    ]);

    let backend = CashuWalletBackend::new();
    let canonical = canonicalize_mint_url(&mock_mint);
    {
        let mut state = lock_state(&backend.state);
        state.mint_info.insert(
            canonical.clone(),
            state_mint_info_fixture("Stale But Known", "https://icon"),
        );
    }

    run_mint_info_refresh(&backend.state, &[mock_mint]);

    let state = lock_state(&backend.state);
    let cached = state.mint_info.get(&canonical).expect("entry must survive");
    assert_eq!(cached.name.as_deref(), Some("Stale But Known"));
}

/// `spawn_mint_info_refresh` on an empty mint list is a pure no-op — never
/// spawns a thread, never touches `mint_info_in_flight`.
#[test]
fn spawn_mint_info_refresh_on_an_empty_list_is_a_no_op() {
    let backend = CashuWalletBackend::new();
    spawn_mint_info_refresh(std::sync::Arc::clone(&backend.state), Vec::new());
    assert!(!lock_state(&backend.state).mint_info_in_flight);
}

/// #2977-style single-flight guard: a trigger that arrives while a pass is
/// already marked in-flight must coalesce into `mint_info_pending` rather
/// than spawn a second concurrent pass (deterministic — sets the flag
/// directly rather than racing a real thread, mirroring
/// `check_state_tests.rs`'s `spawn_debounced_coalesces_a_trigger_...` test).
#[test]
fn spawn_mint_info_refresh_coalesces_a_trigger_that_arrives_while_in_flight() {
    let backend = CashuWalletBackend::new();
    lock_state(&backend.state).mint_info_in_flight = true;

    spawn_mint_info_refresh(
        std::sync::Arc::clone(&backend.state),
        vec!["https://mint.example".to_string()],
    );

    let state = lock_state(&backend.state);
    assert!(state.mint_info_in_flight, "still in-flight — owned by the running pass");
    assert_eq!(
        state.mint_info_pending.as_deref(),
        Some(["https://mint.example".to_string()].as_slice())
    );
}

/// The ordinary case: no pass in flight, `spawn_mint_info_refresh` actually
/// spawns one and it runs to completion, leaving no in-flight state behind.
#[test]
fn spawn_mint_info_refresh_runs_a_pass_and_clears_the_flag_when_none_was_running() {
    let mock_mint = spawn_mock_mint(vec![
        (200, INFO_BODY.to_string()),
        (200, KEYS_BODY.to_string()),
        (200, KEYSETS_BODY.to_string()),
    ]);

    let backend = CashuWalletBackend::new();
    spawn_mint_info_refresh(std::sync::Arc::clone(&backend.state), vec![mock_mint.clone()]);

    // The pass runs on its own thread; give it a bounded window (no polling
    // loop — a single sleep well above what a local-socket mock mint round
    // -trip needs), mirroring `check_state_tests.rs`'s equivalent test.
    std::thread::sleep(std::time::Duration::from_millis(300));

    let state = lock_state(&backend.state);
    assert!(
        state.mint_info.contains_key(&canonicalize_mint_url(&mock_mint)),
        "the pass must have populated the cache"
    );
    assert!(!state.mint_info_in_flight, "must clear the flag once no rerun is pending");
}

fn state_mint_info_fixture(name: &str, icon_url: &str) -> crate::backend::cashu::state::CachedMintInfo {
    crate::backend::cashu::state::CachedMintInfo {
        name: Some(name.to_string()),
        icon_url: Some(icon_url.to_string()),
        units: vec!["sat".to_string()],
        fees_by_unit: vec![("sat".to_string(), 0)],
    }
}
