//! V-66 regression tests — `no_configured_relays` must be projected through the
//! KernelUpdate JSON envelope when a user is signed in but `configured_relays` is
//! empty (i.e. the kernel is routing through the hardcoded FALLBACK relays).
//!
//! The failure mode being fixed: when an account is active and no relay rows are
//! configured the kernel silently used `FALLBACK_CONTENT_RELAY` /
//! `FALLBACK_INDEXER_RELAY`. The host had no observable signal to distinguish
//! "user has zero configured relays" from "relay list loaded fine". The user
//! appeared to be online (publishes succeeded against the fallback), but was
//! publishing to relays they did not consent to.
//!
//! Test structure:
//!
//! 1. Seam test — signed-in + empty rows: injects an active account via
//!    `set_active_account_for_test`, verifies `no_configured_relays: true`
//!    appears in the snapshot JSON.
//!
//! 2. Steady-state test — no active account: a fresh kernel with no account
//!    and no rows must NOT emit `no_configured_relays` (pre-sign-in is the
//!    expected cold state, not a user-observable problem).
//!
//! 3. Healthy-state test — signed-in + rows present: once relay rows exist
//!    the field must be absent — we stop emitting the diagnostic as soon as
//!    the condition is resolved.

use super::*;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

/// When an account is active but `configured_relays` is empty, the kernel is
/// silently using fallback relays. The `no_configured_relays: true` field must
/// appear in the KernelUpdate snapshot so the host can surface a diagnostic.
///
/// Pre-fix: `make_update` never checked `active_account`/`configured_relays` →
/// the key was absent → host could not observe the silent fallback → FAILS.
/// Post-fix: `make_update` emits `no_configured_relays: true` → PASSES.
#[test]
fn v66_signed_in_empty_rows_emits_no_configured_relays() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Simulate: account is loaded but relay rows are empty.
    kernel.set_active_account_for_test(
        "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    );
    // configured_relays starts as Vec::new() — the condition is already true.

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    let field = parsed
        .get("no_configured_relays")
        .and_then(serde_json::Value::as_bool);

    assert_eq!(
        field,
        Some(true),
        "V-66 (D3): when an account is active and configured_relays is empty the \
         kernel must emit `no_configured_relays: true` in the KernelUpdate \
         snapshot so the host can surface the fallback-relay diagnostic; \
         got: {:?}",
        parsed.get("no_configured_relays")
    );
}

/// Steady state (no account): a fresh kernel with no active account and no
/// relay rows must NOT emit `no_configured_relays`.  An unsigned-in kernel
/// has no user context — the absence of relay rows is expected, not a
/// user-observable problem. Guards against false positives on cold-start.
#[test]
fn v66_no_account_field_is_absent_from_snapshot() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    // Do NOT set an active account — configured_relays is empty by default,
    // but the diagnostic should only fire when an account is present.

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    assert!(
        !parsed
            .as_object()
            .map(|o| o.contains_key("no_configured_relays"))
            .unwrap_or(false),
        "V-66: with no active account the `no_configured_relays` key must be \
         absent from the snapshot (skip_serializing_if); got: {:?}",
        parsed.get("no_configured_relays")
    );
}

/// Healthy state (signed-in + rows present): once relay rows are configured
/// the `no_configured_relays` field must be absent — we stop emitting the
/// diagnostic as soon as the condition is resolved.
#[test]
fn v66_signed_in_with_rows_field_is_absent() {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Sign in an account.
    kernel.set_active_account_for_test(
        "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    );

    // Install at least one relay row (content/read role).
    kernel.set_configured_relays(vec![AppRelay::new(
        "wss://relay.example.com".to_string(),
        "read".to_string(),
    )]);

    let snapshot_json = kernel.make_update_json_for_test(true);
    let parsed: serde_json::Value =
        serde_json::from_str(&snapshot_json).expect("snapshot must be valid JSON");

    assert!(
        !parsed
            .as_object()
            .map(|o| o.contains_key("no_configured_relays"))
            .unwrap_or(false),
        "V-66: with an active account AND relay rows present the \
         `no_configured_relays` key must be absent from the snapshot; \
         got: {:?}",
        parsed.get("no_configured_relays")
    );
}
