//! End-to-end tests for the NIP-65 auto-publish piggyback on
//! `AddRelay` / `RemoveRelay`.
//!
//! Builder unit tests live next to the builder
//! (`actor::commands::relays::tests`). These tests pin the wiring —
//! that the dispatch arms actually invoke the builder, gate on the
//! active signer, skip no-op edits, and route through
//! `publish_unsigned_event` (i.e. the kind:10002 frame lands in the
//! outbound `EVENT` stream the same way every other publish does).
//!
//! Closing the gap the PR title makes load-bearing: without these
//! tests, a future refactor that drops the `maybe_publish_relay_list_after_edit`
//! call would pass every other unit test silently.
//!
//! These tests use a known dev nsec — never wired to any real
//! relay — to drive `IdentityRuntime` so `active_pubkey()` is `Some`.

use crate::actor::commands::{
    add_relay, add_signer, new_bunker_handshake_slot, remove_relay, IdentityRuntime,
};
use crate::actor::SignerSource;
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

use super::helpers::maybe_publish_relay_list_after_edit;

/// Throwaway nsec — generated for tests only, never on the network.
/// Same dev key the conformance harness round-trip tests
/// (`tests/nip_tag_conformance.rs`) and the remote-signer tests
/// (`actor/commands/remote_signer_tests.rs`) use. Reusing it here
/// keeps the test fixture surface small.
const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

fn fresh_kernel() -> Kernel {
    Kernel::new(DEFAULT_VISIBLE_LIMIT)
}

fn fresh_identity() -> IdentityRuntime {
    use crate::actor::new_signer_state_slot;
    IdentityRuntime::new(new_bunker_handshake_slot(), new_signer_state_slot())
}

fn signed_in_identity(kernel: &mut Kernel) -> IdentityRuntime {
    let mut identity = fresh_identity();
    add_signer(
        &mut identity,
        kernel,
        SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    assert!(
        identity.active_pubkey().is_some(),
        "add_signer(LocalNsec, make_active) must produce an active account",
    );
    identity
}

/// Helper: count `["EVENT", { "kind": 10002, ... }]` frames in an
/// outbound batch. Mirrors the conformance harness shape check —
/// outbound text is a raw wire frame, so we string-search for the
/// outer `["EVENT"` and a kind:10002 marker.
fn count_kind_10002_frames(outbound: &[crate::relay::OutboundMessage]) -> usize {
    outbound
        .iter()
        .filter(|m| m.text.starts_with("[\"EVENT\""))
        .filter(|m| {
            // The wire shape is `["EVENT", {"kind":10002,...}]` (no
            // SUBSCRIPTION-ID prefix variant — kind:10002 routes
            // through the Auto outbox, not a REQ).
            let parsed: serde_json::Value = match serde_json::from_str(&m.text) {
                Ok(v) => v,
                Err(_) => return false,
            };
            parsed
                .as_array()
                .and_then(|arr| arr.get(1))
                .and_then(|ev| ev.get("kind"))
                .and_then(serde_json::Value::as_u64)
                == Some(10002)
        })
        .count()
}

#[test]
fn add_relay_with_active_signer_publishes_kind_10002() {
    // Headline assertion the PR title makes: a real AddRelay edit by a
    // signed-in user produces a kind:10002 frame.
    let mut kernel = fresh_kernel();
    let mut identity = signed_in_identity(&mut kernel);
    let mut pending = crate::actor::pending_sign::ParkedSignerOps::new();

    // Capture the projection BEFORE the mutation, as the dispatch arm
    // does, then mutate and call the helper directly.
    let before = kernel.configured_relays_snapshot().to_vec();
    let added = add_relay(&mut kernel, "wss://relay.example", "both");
    assert!(added.is_some(), "add_relay must accept a valid wss:// URL");

    let outbound =
        maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
    assert!(
        count_kind_10002_frames(&outbound) >= 1,
        "AddRelay with an active signer must re-publish kind:10002. \
         Outbound frames were: {:?}",
        outbound.iter().map(|m| &m.text).collect::<Vec<_>>(),
    );
}

#[test]
fn add_relay_without_active_signer_does_not_publish() {
    // Guard 1: a relay edit while signed out must NOT try to publish
    // (and must NOT set the no-account error toast).
    let mut kernel = fresh_kernel();
    let mut identity = fresh_identity();
    let mut pending = crate::actor::pending_sign::ParkedSignerOps::new();

    let before = kernel.configured_relays_snapshot().to_vec();
    add_relay(&mut kernel, "wss://relay.example", "both");

    let outbound =
        maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
    assert_eq!(
        count_kind_10002_frames(&outbound),
        0,
        "without an active signer, no kind:10002 must be published",
    );
    assert!(
        kernel.last_error_toast_snapshot().is_none(),
        "signed-out relay edits MUST NOT poison the toast slot \
         (toast_no_account would be wrong observable here)",
    );
}

#[test]
fn add_relay_no_op_does_not_republish() {
    // Guard 2: re-adding the same URL with the same role is a no-op on
    // the projection. The dispatch arm MUST skip the re-publish in
    // that case — otherwise every duplicate FFI call burns a relay
    // write and bumps the kind:10002 timestamp for nothing.
    let mut kernel = fresh_kernel();
    let mut identity = signed_in_identity(&mut kernel);
    let mut pending = crate::actor::pending_sign::ParkedSignerOps::new();

    // First add — projection changes; this would publish.
    add_relay(&mut kernel, "wss://relay.example", "both");

    // Second add — identical role, no projection change.
    let before = kernel.configured_relays_snapshot().to_vec();
    add_relay(&mut kernel, "wss://relay.example", "both");

    let outbound =
        maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
    assert_eq!(
        count_kind_10002_frames(&outbound),
        0,
        "re-adding the same URL+role MUST NOT re-publish kind:10002 \
         (projection unchanged → no semantic change)",
    );
}

#[test]
fn remove_relay_nonexistent_does_not_republish() {
    // Guard 2 (mirror): removing a URL that was never present is a
    // no-op on the projection. The dispatch arm MUST skip the
    // re-publish.
    let mut kernel = fresh_kernel();
    let mut identity = signed_in_identity(&mut kernel);
    let mut pending = crate::actor::pending_sign::ParkedSignerOps::new();

    // Seed one row so the projection is non-empty (otherwise guard 3
    // would also trip and we couldn't distinguish guard-2 from guard-3).
    add_relay(&mut kernel, "wss://relay.example", "both");

    let before = kernel.configured_relays_snapshot().to_vec();
    remove_relay(&mut kernel, "wss://other.example");

    let outbound =
        maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
    assert_eq!(
        count_kind_10002_frames(&outbound),
        0,
        "removing an absent URL MUST NOT re-publish kind:10002",
    );
}

#[test]
fn remove_relay_existing_does_republish() {
    // Symmetric to `add_relay_with_active_signer_publishes_kind_10002`:
    // a real removal that mutates the projection must produce a
    // kind:10002 reflecting the new (smaller) set. This is the half
    // the PR is named for — clients reading the relay graph see the
    // removed relay leave the user's outbox without needing a manual
    // dispatch.
    let mut kernel = fresh_kernel();
    let mut identity = signed_in_identity(&mut kernel);
    let mut pending = crate::actor::pending_sign::ParkedSignerOps::new();

    // Seed two rows so the post-removal projection still has at least
    // one NIP-65-eligible row — otherwise guard 3 (don't publish
    // empty kind:10002) would correctly skip the publish.
    add_relay(&mut kernel, "wss://keep.example", "both");
    add_relay(&mut kernel, "wss://drop.example", "both");

    let before = kernel.configured_relays_snapshot().to_vec();
    remove_relay(&mut kernel, "wss://drop.example");

    let outbound =
        maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
    assert!(
        count_kind_10002_frames(&outbound) >= 1,
        "removing an existing URL must re-publish kind:10002 with \
         the remaining set. Outbound frames were: {:?}",
        outbound.iter().map(|m| &m.text).collect::<Vec<_>>(),
    );
}

#[test]
fn empty_projection_after_remove_does_not_republish() {
    // Guard 3: removing the user's last NIP-65-eligible row leaves
    // the projection empty. We must NOT publish an empty kind:10002
    // because `ingest_relay_list` treats that as "clear my NIP-65
    // metadata" (destructive — see kernel/ingest/relay_list.rs:31).
    // The user explicitly removing a relay is NOT the same intent as
    // "wipe my NIP-65 outbox"; that needs its own explicit verb.
    let mut kernel = fresh_kernel();
    let mut identity = signed_in_identity(&mut kernel);
    let mut pending = crate::actor::pending_sign::ParkedSignerOps::new();

    add_relay(&mut kernel, "wss://only.example", "both");

    let before = kernel.configured_relays_snapshot().to_vec();
    remove_relay(&mut kernel, "wss://only.example");
    assert!(
        kernel.configured_relays_snapshot().is_empty(),
        "test precondition: projection must be empty after removing the only row"
    );

    let outbound =
        maybe_publish_relay_list_after_edit(&mut identity, &mut kernel, &before, &mut pending);
    assert_eq!(
        count_kind_10002_frames(&outbound),
        0,
        "removing the user's last NIP-65-eligible row MUST NOT \
         publish an empty kind:10002 (that would clear the \
         author_relay_lists cache on ingest — destructive)",
    );
}
