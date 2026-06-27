//! Unit tests for [`super::KernelReducer::apply_actor_command`] (#2045 PR-A) —
//! the publish, sign-roundtrip (NeedsSign), and unsupported-command verbs.
//!
//! The interest / relay-info / lifecycle / contacts tests live in the sibling
//! `command_apply_tests.rs` (file-size ceiling split). Together the two files
//! cover every Group-A (Applied), Group-B (NeedsSign), and Group-C
//! (Unsupported) outcome.
//!
//! The tests are written to the public `CommandApplyOutcome` shape and use the
//! same `KernelReducer` seam the wasm runtime drives — no direct `Kernel`
//! access for production paths (only for verification helpers like
//! `r.kernel.last_error_toast_snapshot()`).
//!
//! Include guard: this file is `#[path]`-included by `kernel_reducer.rs`.

use super::*;
use crate::actor::{
    ActorCommand, IdentityCommand, PublishCommand, RelayCommand, SignCommand, SignerSource,
};
use crate::publish::PublishTarget;
use crate::store::RawEvent;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

// ─── shared constants / helpers ──────────────────────────────────────────────

const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
const RELAY: &str = "wss://relay.example";

/// A genuine Schnorr-signed event — passes `verify_externally_signed_event`.
fn real_signed_event(keys: &::nostr::Keys, kind: u16, content: &str) -> SignedEvent {
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(kind), content)
        .custom_created_at(::nostr::Timestamp::from_secs(1_700_000_000))
        .sign_with_keys(keys)
        .expect("test key signs");
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event
                .tags
                .iter()
                .map(|t: &::nostr::Tag| t.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

/// A forged event: syntactically valid fields, placeholder id+sig — will fail
/// `verify_externally_signed_event`.
fn forged_signed_note() -> SignedEvent {
    SignedEvent {
        id: "a".repeat(64),
        sig: "b".repeat(128),
        unsigned: UnsignedEvent {
            pubkey: PK.to_string(),
            kind: 1,
            tags: Vec::new(),
            content: "forged".to_string(),
            created_at: 1_700_000_000,
        },
    }
}

/// Turn a `SignedEvent` into the `RawEvent` shape that `PublishCommand::SignedEvent` carries.
fn to_raw(s: &SignedEvent) -> RawEvent {
    RawEvent {
        id: s.id.clone(),
        sig: s.sig.clone(),
        pubkey: s.unsigned.pubkey.clone(),
        kind: s.unsigned.kind,
        tags: s.unsigned.tags.clone(),
        content: s.unsigned.content.clone(),
        created_at: s.unsigned.created_at,
    }
}

// ─── Group A: Applied (synchronous) — pre-signed publish verbs ───────────────

#[test]
fn signed_event_valid_returns_applied_no_malformed_toast() {
    // A well-formed, genuinely Schnorr-signed kind:1 event passes the
    // well-formedness chokepoint and returns Applied. On a fresh kernel with no
    // NIP-65 outbox the publish engine reports NoTargets via toast, but the
    // _malformed-event_ toast must be absent (the event is not malformed).
    let keys = ::nostr::Keys::generate();
    let signed = real_signed_event(&keys, 1, "hello from apply_actor_command");
    let raw = to_raw(&signed);
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::SignedEvent {
        raw,
        target: PublishTarget::Auto,
        correlation_id: None,
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(_)),
        "valid SignedEvent must return Applied"
    );
    let toast = r.kernel.last_error_toast_snapshot();
    assert!(
        !toast
            .as_deref()
            .map_or(false, |t| t.contains("signed event rejected")),
        "a well-formed event must NOT trigger the malformed-event toast; got: {toast:?}"
    );
}

#[test]
fn signed_event_forged_returns_applied_empty_with_malformed_toast() {
    // A forged event (placeholder id+sig that fail Schnorr verification) must
    // be rejected fail-closed: Applied(empty) + the malformed-event toast set.
    // This proves `apply_actor_command` runs `publish_externally_signed` (which
    // verifies the signature) rather than bypassing verification.
    let raw = to_raw(&forged_signed_note());
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::SignedEvent {
        raw,
        target: PublishTarget::Auto,
        correlation_id: None,
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "forged SignedEvent must be rejected fail-closed: Applied(empty)"
    );
    let toast = r.kernel.last_error_toast_snapshot();
    assert!(
        toast
            .as_deref()
            .map_or(false, |t| t.contains("signed event rejected")),
        "forged event must set the malformed-event toast; got: {toast:?}"
    );
}

#[test]
fn signed_event_gift_wrap_auto_d10_guard() {
    // D10 routing-policy GUARD: a kind:1059 gift-wrap sent with
    // PublishTarget::Auto must be refused at the routing gate (D10 policy
    // says private envelopes require an explicit relay pin). Applied(empty)
    // + NO malformed-event toast (the outer envelope is well-formed — the
    // refusal is a routing-policy decision, not a sig/id failure).
    //
    // This test is a D10 GUARD: it already passed pre-change and must
    // continue to pass. If it starts failing, D10 routing was weakened.
    let keys = ::nostr::Keys::generate();
    let signed = real_signed_event(&keys, 1059, "AESGCM-ciphertext");
    let raw = to_raw(&signed);
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::SignedEvent {
        raw,
        target: PublishTarget::Auto,
        correlation_id: None,
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Applied(v) if v.is_empty()),
        "kind:1059 + Auto must be refused (D10 gate): Applied(empty)"
    );
    let toast = r.kernel.last_error_toast_snapshot();
    assert!(
        !toast
            .as_deref()
            .map_or(false, |t| t.contains("signed event rejected")),
        "D10 refusal must NOT set the malformed-event toast (opacity ADR-0025); got: {toast:?}"
    );
}

// ─── Group B: NeedsSign (async sign round-trip required) ─────────────────────

#[test]
fn unsigned_event_needs_sign_with_correlation_id() {
    // UnsignedEvent dispatches to a NeedsSign outcome when an active account
    // is present. The sign-request's `account_pubkey` must match the active
    // account, and `action_correlation_id` carries the command's cid.
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(PK.to_string());

    let cid = Some("unsigned-cid-1".to_string());
    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::UnsignedEvent {
        event: UnsignedEvent {
            pubkey: PK.to_string(), // ignored; filled from active account
            kind: 1,
            tags: vec![],
            content: "unsigned note".to_string(),
            created_at: 1_700_000_000,
        },
        correlation_id: cid.clone(),
        signer_pubkey: None,
    }));

    match outcome {
        CommandApplyOutcome::NeedsSign {
            request,
            target: _,
            action_correlation_id,
        } => {
            assert_eq!(
                request.account_pubkey, PK,
                "sign request account must be the active account"
            );
            assert_eq!(
                action_correlation_id, cid,
                "action_correlation_id must propagate from the command"
            );
        }
        other => panic!("expected NeedsSign, got {other:?}"),
    }
}

#[test]
fn raw_event_needs_sign_with_correlation_id() {
    // RawEvent (kind + tags + content) → NeedsSign when active account present.
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(PK.to_string());

    let cid = Some("raw-cid-2".to_string());
    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::RawEvent {
        kind: 1,
        tags: vec![],
        content: "raw note".to_string(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
        correlation_id: cid.clone(),
    }));

    match outcome {
        CommandApplyOutcome::NeedsSign {
            request,
            target: PublishTarget::Auto,
            action_correlation_id,
        } => {
            assert_eq!(request.account_pubkey, PK);
            assert_eq!(action_correlation_id, cid);
        }
        other => panic!("expected NeedsSign(Auto), got {other:?}"),
    }
}

#[test]
fn reply_needs_sign_with_nip10_tags_from_stored_parent() {
    let parent_id = "a1".repeat(32);
    let parent_author = "b2".repeat(32);
    let root_id = "c3".repeat(32);
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(PK.to_string());
    r.kernel
        .event_store_handle()
        .insert(
            crate::store::VerifiedEvent::from_raw_unchecked(RawEvent {
                id: parent_id.clone(),
                pubkey: parent_author.clone(),
                created_at: 1_700_000_000,
                kind: 1,
                tags: vec![vec![
                    "e".to_string(),
                    root_id.clone(),
                    "".to_string(),
                    "root".to_string(),
                ]],
                content: "parent".to_string(),
                sig: "0".repeat(128),
            }),
            &RELAY.to_string(),
            0,
        )
        .expect("seed parent");

    let cid = Some("reply-cid".to_string());
    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::Reply {
        content: "reply body".to_string(),
        reply_to_event_id: parent_id.clone(),
        target: PublishTarget::Auto,
        signer_pubkey: None,
        correlation_id: cid.clone(),
    }));

    match outcome {
        CommandApplyOutcome::NeedsSign {
            request,
            target: PublishTarget::Auto,
            action_correlation_id,
        } => {
            assert_eq!(request.account_pubkey, PK);
            assert_eq!(action_correlation_id, cid);
            let unsigned: serde_json::Value =
                serde_json::from_str(&request.unsigned_json).expect("unsigned json");
            assert_eq!(unsigned["kind"], 1);
            assert_eq!(unsigned["content"], "reply body");
            let tags = unsigned["tags"].as_array().expect("tags array");
            assert!(
                tags.iter().any(|tag| tag
                    .as_array()
                    .is_some_and(|row| row.get(0) == Some(&serde_json::json!("e"))
                        && row.get(1) == Some(&serde_json::json!(root_id))
                        && row.get(3) == Some(&serde_json::json!("root")))),
                "reply must carry root e-tag: {}",
                request.unsigned_json
            );
            assert!(
                tags.iter().any(|tag| tag
                    .as_array()
                    .is_some_and(|row| row.get(0) == Some(&serde_json::json!("e"))
                        && row.get(1) == Some(&serde_json::json!(parent_id))
                        && row.get(3) == Some(&serde_json::json!("reply")))),
                "reply must carry direct reply e-tag: {}",
                request.unsigned_json
            );
            assert!(
                tags.iter().any(|tag| tag
                    .as_array()
                    .is_some_and(|row| row.get(0) == Some(&serde_json::json!("p"))
                        && row.get(1) == Some(&serde_json::json!(parent_author)))),
                "reply must p-tag the parent author: {}",
                request.unsigned_json
            );
        }
        other => panic!("expected NeedsSign(Auto), got {other:?}"),
    }
}

#[test]
fn reply_missing_parent_returns_unsupported() {
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(PK.to_string());

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::Reply {
        content: "reply body".to_string(),
        reply_to_event_id: "a1".repeat(32),
        target: PublishTarget::Auto,
        signer_pubkey: None,
        correlation_id: Some("reply-missing".to_string()),
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Unsupported { ref reason } if reason.contains("reply_target_unknown")),
        "missing parent must fail closed; got {outcome:?}"
    );
}

#[test]
fn profile_needs_sign_with_correlation_id() {
    // Profile (kind:0) → NeedsSign with target Auto and the command's cid.
    let mut r = KernelReducer::new();
    let _ = r.set_active_account(PK.to_string());

    let cid = Some("profile-cid-3".to_string());
    let mut fields = serde_json::Map::new();
    fields.insert(
        "name".to_string(),
        serde_json::Value::String("Alice".to_string()),
    );

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::Profile {
        fields,
        correlation_id: cid.clone(),
    }));

    match outcome {
        CommandApplyOutcome::NeedsSign {
            request,
            target: PublishTarget::Auto,
            action_correlation_id,
        } => {
            assert_eq!(request.account_pubkey, PK);
            assert_eq!(action_correlation_id, cid);
            // The unsigned JSON must be a kind:0 event.
            assert!(
                request.unsigned_json.contains("\"kind\":0"),
                "Profile must build a kind:0 unsigned event; got: {}",
                request.unsigned_json
            );
        }
        other => panic!("expected NeedsSign(Auto) for Profile, got {other:?}"),
    }
}

#[test]
fn unsigned_event_without_active_account_returns_unsupported() {
    // UnsignedEvent with no active account: Unsupported (no signer available).
    // D6 — no panic.
    let mut r = KernelReducer::new();
    // Do NOT call set_active_account — no active account.

    let outcome = r.apply_actor_command(ActorCommand::Publish(PublishCommand::UnsignedEvent {
        event: UnsignedEvent {
            pubkey: "".to_string(),
            kind: 1,
            tags: vec![],
            content: "no account".to_string(),
            created_at: 1_700_000_000,
        },
        correlation_id: None,
        signer_pubkey: None,
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Unsupported { .. }),
        "UnsignedEvent with no active account must return Unsupported"
    );
}

// ─── Group C: Unsupported (native actor thread required) ─────────────────────

#[test]
fn sign_event_for_return_is_unsupported() {
    // SignCommand verbs are native-actor-only (they drive ParkedOp queues backed
    // by thread-local waiters). `apply_actor_command` must return Unsupported.
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Sign(SignCommand::EventForReturn {
        account_pubkey: PK.to_string(),
        unsigned_json: r#"{"kind":1,"tags":[],"content":"x","created_at":0}"#.to_string(),
        correlation_id: "sign-cid".to_string(),
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Unsupported { reason } if !reason.is_empty()),
        "SignCommand::EventForReturn must be Unsupported in the headless runtime"
    );
}

#[test]
fn identity_add_signer_is_unsupported() {
    // IdentityCommand verbs require the native actor's roster + signer runtime.
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Identity(IdentityCommand::AddSigner {
        source: SignerSource::LocalNsec(zeroize::Zeroizing::new(
            "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq".to_string(),
        )),
        make_active: false,
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Unsupported { .. }),
        "IdentityCommand::AddSigner must be Unsupported in the headless runtime"
    );
}

#[test]
fn relay_add_relay_is_unsupported() {
    // RelayCommand::AddRelay requires the native relay runtime (spawn worker,
    // update channel sender, etc.) — headless runtime returns Unsupported.
    let mut r = KernelReducer::new();

    let outcome = r.apply_actor_command(ActorCommand::Relay(RelayCommand::AddRelay {
        url: RELAY.to_string(),
        role: "both".to_string(),
    }));

    assert!(
        matches!(outcome, CommandApplyOutcome::Unsupported { .. }),
        "RelayCommand::AddRelay must be Unsupported in the headless runtime"
    );
}

#[test]
fn unsupported_reason_is_non_empty_string() {
    // The reason string for every Unsupported outcome must be non-empty so the
    // host's CapabilityFailure message is human-readable (D6-honest).
    let mut r = KernelReducer::new();

    let cases: Vec<ActorCommand> = vec![
        ActorCommand::Sign(SignCommand::EventForReturn {
            account_pubkey: PK.to_string(),
            unsigned_json: "{}".to_string(),
            correlation_id: "x".to_string(),
        }),
        ActorCommand::Identity(IdentityCommand::SwitchActive {
            identity_id: PK.to_string(),
        }),
        ActorCommand::Relay(RelayCommand::AddRelay {
            url: RELAY.to_string(),
            role: "both".to_string(),
        }),
    ];

    for cmd in cases {
        let label = format!("{cmd:?}");
        let outcome = r.apply_actor_command(cmd);
        match outcome {
            CommandApplyOutcome::Unsupported { reason } => {
                assert!(!reason.is_empty(), "reason must be non-empty for {label}");
            }
            other => panic!("expected Unsupported for {label}, got {other:?}"),
        }
    }
}
