use super::super::snapshot::{BrowserSnapshotCache, SnapshotOutcome};
use super::*;

#[test]
#[should_panic(expected = "empty set")]
fn declare_projections_empty_panics() {
    let _builder = crate::BrowserAppBuilder::new()
        .in_memory()
        .declare_projections(Vec::<String>::new());
}

#[test]
fn next_frame_returns_frame_on_valid_handle() {
    let mut handle = started_handle();
    match handle.next_frame(true) {
        SnapshotOutcome::Frame(bytes) => {
            assert!(!bytes.is_empty(), "merged frame must be non-empty");
        }
        other => panic!("expected Frame, got {other:?}"),
    }
}

#[test]
fn snapshot_cache_degraded_on_corrupt_bytes() {
    let mut cache = BrowserSnapshotCache::new();
    let outcome = cache.apply_frame(b"this is not a valid update frame at all");
    match outcome {
        SnapshotOutcome::Degraded { last_good, .. } => {
            assert!(
                last_good.is_none(),
                "no prior good frame -> last_good must be None"
            );
        }
        other => panic!("expected Degraded, got {other:?}"),
    }
    assert!(
        cache.last_good().is_none(),
        "last_good must still be None after a failed frame"
    );
}

#[test]
fn snapshot_cache_panic_on_panic_frame() {
    let mut cache = BrowserSnapshotCache::new();
    let panic_bytes = nmp_core::encode_panic("kernel explosion: boom");
    match cache.apply_frame(&panic_bytes) {
        SnapshotOutcome::Panic(msg) => {
            assert!(
                msg.contains("kernel explosion"),
                "panic message must be forwarded: {msg}"
            );
        }
        other => panic!("expected Panic, got {other:?}"),
    }
    assert!(
        cache.last_good().is_none(),
        "panic frame must not update last_good"
    );
}

#[test]
fn signer_state_slot_round_trip_via_diagnostics() {
    use nmp_core::SignerStateModel;

    let mut handle = started_handle();

    let diag = handle.diagnostics();
    assert!(
        diag.signer_kind.is_none(),
        "no signer set -> signer_kind must be None"
    );
    assert!(
        diag.signer_state.is_none(),
        "no signer set -> signer_state must be None"
    );

    handle.set_signer_state(Some(SignerStateModel {
        signer_kind: "nip46".to_string(),
        state: "ready".to_string(),
        is_ready: true,
        ..Default::default()
    }));

    let diag = handle.diagnostics();
    assert_eq!(
        diag.signer_kind.as_deref(),
        Some("nip46"),
        "diagnostics must reflect the installed signer_kind"
    );
    assert_eq!(
        diag.signer_state.as_deref(),
        Some("ready"),
        "diagnostics must reflect the installed state"
    );

    handle.set_signer_state(None);
    let diag = handle.diagnostics();
    assert!(
        diag.signer_kind.is_none(),
        "cleared slot -> signer_kind None"
    );
    assert!(
        diag.signer_state.is_none(),
        "cleared slot -> signer_state None"
    );
}

#[test]
fn diagnostics_redacts_identity_to_npub_prefix() {
    let mut handle = started_handle();
    let pubkey_hex = "ab".repeat(32);
    handle.set_active_account_for_test(&pubkey_hex);

    let _ = handle.next_frame(true);

    let diag = handle.diagnostics();
    let prefix = diag
        .active_account_npub_prefix
        .expect("active account must produce a prefix");
    assert!(
        prefix.starts_with("npub1"),
        "prefix must be a bech32 npub1 start: {prefix}"
    );
    assert!(
        prefix.len() <= 8,
        "prefix must be at most 8 chars (redacted), got {} chars: {prefix}",
        prefix.len()
    );
    assert_ne!(
        prefix.len(),
        64,
        "must NOT be the full hex key (64 hex chars)"
    );
}

#[test]
fn diagnostics_json_does_not_leak_full_pubkey() {
    let mut handle = started_handle();
    let pubkey_hex = "cd".repeat(32);
    handle.set_active_account_for_test(&pubkey_hex);
    let _ = handle.next_frame(true);

    let json = handle.diagnostics().to_json();
    assert!(
        !json.contains(&pubkey_hex),
        "diagnostics JSON must not contain the full hex pubkey"
    );
}

#[test]
fn injected_clock_is_observed_after_start() {
    use nmp_core::{time::SystemTime, Clock};
    use std::sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    };

    struct StubClock(Arc<AtomicU64>);
    impl Clock for StubClock {
        fn now(&self) -> SystemTime {
            self.0.fetch_add(1, Ordering::Relaxed);
            SystemTime::UNIX_EPOCH
        }
    }

    let call_count = Arc::new(AtomicU64::new(0));
    let clock = Arc::new(StubClock(Arc::clone(&call_count)));

    let mut handle = crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default())
        .with_clock(clock)
        .start();

    let _ = handle.next_frame(true);

    assert!(
        call_count.load(Ordering::Relaxed) > 0,
        "injected clock must be called after start() and next_frame()"
    );
}

/// Decode the typed-projection keys present in a merged `Frame` outcome.
fn frame_keys(outcome: &SnapshotOutcome) -> Vec<String> {
    let SnapshotOutcome::Frame(bytes) = outcome else {
        panic!("expected Frame, got {outcome:?}");
    };
    nmp_core::decode_snapshot_typed_projections(bytes)
        .expect("merged frame decodes")
        .into_iter()
        .map(|row| row.key)
        .collect()
}

/// BLOCKER-1 regression (#2074): set `Some` -> the merged frame carries a
/// `signer_state` sidecar; set `None` -> the NEXT merged frame DROPS it. A
/// closure returning `None` on clear would leave the key STALE in the merge
/// cache (omitted == unchanged); the one-shot `Cleared` row fixes that.
#[test]
fn set_signer_state_none_drops_sidecar_from_merged_frame() {
    use nmp_core::SignerStateModel;

    let mut handle = started_handle();

    // Initially absent.
    let f0 = handle.next_frame(true);
    assert!(
        !frame_keys(&f0).iter().any(|k| k == "signer_state"),
        "no signer set -> no signer_state sidecar"
    );

    // Set Some -> sidecar appears.
    handle.set_signer_state(Some(SignerStateModel {
        signer_kind: "nip46".to_string(),
        state: "ready".to_string(),
        is_ready: true,
        ..Default::default()
    }));
    let f1 = handle.next_frame(true);
    assert!(
        frame_keys(&f1).iter().any(|k| k == "signer_state"),
        "active signer -> signer_state sidecar present"
    );

    // Clear -> the NEXT merged frame must DROP the key (not retain it stale).
    handle.set_signer_state(None);
    let f2 = handle.next_frame(true);
    assert!(
        !frame_keys(&f2).iter().any(|k| k == "signer_state"),
        "cleared signer -> signer_state sidecar must be dropped from merged frame"
    );

    // And it stays dropped on subsequent frames (one-shot Cleared, then absent).
    let f3 = handle.next_frame(true);
    assert!(
        !frame_keys(&f3).iter().any(|k| k == "signer_state"),
        "signer_state stays absent after the one-shot clear"
    );
}

/// SHOULD-FIX-4 (#2074): a sole registered provider seeds the signer-state slot
/// to a `ready` state at `start()`, so the projection reflects reality rather
/// than being silently empty.
#[test]
fn sole_provider_seeds_ready_signer_state() {
    use nmp_signers::{LocalKeySigner, Signer};
    use std::sync::Arc;

    let signer = LocalKeySigner::from_secret_hex(&"ab".repeat(32)).expect("valid secret");
    let signer: Arc<dyn Signer> = Arc::new(signer);

    let builder = crate::BrowserAppBuilder::new()
        .in_memory()
        .consume_all_builtin_projections()
        .without_initial_relays()
        .decide_providers(crate::BrowserRunConfig::default());
    builder.with_capability_providers([Arc::clone(&signer)]);
    let handle = builder.start();

    let diag = handle.diagnostics();
    assert_eq!(
        diag.signer_kind.as_deref(),
        Some("local"),
        "sole LocalKey provider seeds signer_kind=local"
    );
    assert_eq!(
        diag.signer_state.as_deref(),
        Some("ready"),
        "sole provider seeds state=ready"
    );
}

/// SHOULD-FIX-3 (D6): a poisoned read yields defaults (no sidecar / `None`),
/// never recovered-poisoned data. A poisoned WRITE is recovered so it is not
/// dropped (a dropped write would leave stale-but-presented state).
#[test]
fn poisoned_slot_read_defaults_and_write_recovers() {
    use super::super::signer_state::{new_signer_state_slot, update_signer_state};
    use nmp_core::SignerStateModel;
    use std::sync::Arc;

    let slot = new_signer_state_slot();

    update_signer_state(
        &slot,
        Some(SignerStateModel {
            signer_kind: "nip46".to_string(),
            state: "ready".to_string(),
            is_ready: true,
            ..Default::default()
        }),
    );
    // Poison the mutex by panicking while the guard is held.
    let poison_slot = Arc::clone(&slot);
    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _guard = poison_slot.lock().expect("lock");
        panic!("poison the mutex");
    }));
    assert!(slot.is_poisoned(), "mutex must be poisoned after the panic");

    // A WRITE must still land (recovered, not dropped): clear then re-set.
    update_signer_state(&slot, None);
    update_signer_state(
        &slot,
        Some(SignerStateModel {
            signer_kind: "local".to_string(),
            state: "ready".to_string(),
            is_ready: true,
            ..Default::default()
        }),
    );
    let guard = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(
        guard.model().map(|m| m.signer_kind.as_str()),
        Some("local"),
        "poisoned write must be recovered + applied, not dropped"
    );
}
