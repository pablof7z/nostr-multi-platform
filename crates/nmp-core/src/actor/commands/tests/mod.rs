//! T66a command-path unit tests.
//!
//! Each test drives the public command handlers against a real `Kernel` +
//! `IdentityRuntime` (no mocks) and asserts on the snapshot projections the
//! FFI surfaces — exactly what the SwiftUI screens read.
//!
//! This module is the thin entry point; the actual tests live in the sibling
//! sub-modules below, each scoped to one handler domain:
//!
//! | file                        | domain                                      |
//! | --------------------------- | ------------------------------------------- |
//! | `identity_account.rs`       | sign-in, create-account, switch, remove     |
//! | `publish_unsigned.rs`       | publish_unsigned_event (kind + tag guards)  |
//! | `publish_signed.rs`         | publish_signed_event (tamper, D10, explicit)|
//! | `publish_unsigned_to.rs`    | publish_unsigned_event_to_relays            |
//! | `follow_relay_profile.rs`   | follow/unfollow, relay CRUD, profile, bunker|
//! | `snapshot_lifecycle.rs`     | snapshot JSON shape, follows-feed lifecycle |

use super::*;
use crate::kernel::Kernel;
use crate::publish::{
    InMemoryPublishStore, PublishRecord, PublishRouteClass, PublishStore, PublishTarget,
};
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use std::sync::{mpsc, Arc, Mutex};

// ── shared constants ─────────────────────────────────────────────────────────

pub(super) const TEST_NSEC: &str =
    "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
pub(super) const SECOND_HEX: &str =
    "0000000000000000000000000000000000000000000000000000000000000abc";

/// Write relays injected via kind:10002 for tests that exercise the publish path.
///
/// T-publish-resolver-indexer (codex f81f735): `Nip65OutboxResolver` is now
/// fail-closed — an author with no kind:10002 resolves to an empty relay set
/// (`NoTargets`). Tests that assert non-empty outbound frames MUST seed a
/// kind:10002 for the active account before publishing.
pub(super) const TEST_WRITE_RELAYS: &[&str] =
    &["wss://test-write-r1.test", "wss://test-write-r2.test"];

/// Relays distinct from `TEST_WRITE_RELAYS` so assertions can discriminate
/// an honest Explicit route from a silent Auto/outbox fallback.
pub(super) const TEST_GROUP_RELAYS: &[&str] =
    &["wss://group-relay-a.test", "wss://group-relay-b.test"];

// ── shared helper fns ────────────────────────────────────────────────────────

/// Test shim preserving the pre-`AddSigner` `sign_in_nsec(id, kernel, secret,
/// relays_ready)` call shape used throughout this file. Delegates to the
/// unified `add_signer` reducer with `make_active: true` (the old `sign_in_nsec`
/// always activated the imported key).
pub(super) fn sign_in_nsec(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    secret: &str,
    relays_ready: bool,
) -> Vec<crate::relay::OutboundMessage> {
    add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(secret.to_string())),
        true,
        relays_ready,
    )
}

/// Test shim preserving the pre-`AddSigner` `sign_in_bunker(id, kernel, uri)`
/// call shape. Delegates to the unified `add_signer` reducer's `BunkerUri`
/// branch with `make_active: true` (the old bunker sign-in always activated the
/// resolved account). Needs `&mut` because the reducer stashes the
/// `make_active` flag for the async handshake round-trip.
pub(super) fn sign_in_bunker(identity: &mut IdentityRuntime, kernel: &mut Kernel, uri: &str) {
    add_signer(
        identity,
        kernel,
        crate::actor::SignerSource::BunkerUri(uri.to_string()),
        true,
        false,
    );
}

pub(super) fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

pub(super) fn fresh_with_publish_store() -> (IdentityRuntime, Kernel, Arc<InMemoryPublishStore>) {
    let publish_store = Arc::new(InMemoryPublishStore::new());
    let kernel = Kernel::with_publish_store(
        DEFAULT_VISIBLE_LIMIT,
        Arc::clone(&publish_store) as Arc<dyn PublishStore>,
    );
    (
        IdentityRuntime::new(
            new_bunker_handshake_slot(),
            crate::actor::new_signer_state_slot(),
        ),
        kernel,
        publish_store,
    )
}

/// Sign in with TEST_NSEC and seed kind:10002 write relays for the active
/// account so the `Nip65OutboxResolver` has NIP-65 data and publish commands
/// produce non-empty outbound frames.
pub(super) fn sign_in_with_nip65(id: &mut IdentityRuntime, kernel: &mut Kernel) {
    sign_in_nsec(id, kernel, TEST_NSEC, false);
    let pubkey = id
        .active_pubkey()
        .expect("active account after sign_in_nsec");
    kernel.seed_kind10002_for_test(&pubkey, TEST_WRITE_RELAYS);
}

#[derive(Debug)]
pub(super) struct PendingCaptureRemoteSigner {
    pubkey: String,
    captured: Arc<Mutex<Vec<nmp_signer_iface::UnsignedEvent>>>,
    senders: Mutex<
        Vec<mpsc::Sender<Result<nmp_signer_iface::SignedEvent, nmp_signer_iface::SignerError>>>,
    >,
}

impl PendingCaptureRemoteSigner {
    pub(super) fn new(pubkey: &str) -> Self {
        Self {
            pubkey: pubkey.to_string(),
            captured: Arc::new(Mutex::new(Vec::new())),
            senders: Mutex::new(Vec::new()),
        }
    }

    pub(super) fn captured_handle(&self) -> Arc<Mutex<Vec<nmp_signer_iface::UnsignedEvent>>> {
        Arc::clone(&self.captured)
    }
}

impl nmp_signer_iface::RemoteSignerHandle for PendingCaptureRemoteSigner {
    fn pubkey_hex(&self) -> String {
        self.pubkey.clone()
    }

    fn signer_kind(&self) -> &'static str {
        "nip46"
    }

    fn sign(
        &self,
        unsigned: &nmp_signer_iface::UnsignedEvent,
    ) -> nmp_signer_iface::SignerOp<nmp_signer_iface::SignedEvent> {
        self.captured
            .lock()
            .expect("capture mutex")
            .push(unsigned.clone());
        let (tx, rx) = mpsc::channel();
        self.senders.lock().expect("sender mutex").push(tx);
        nmp_signer_iface::SignerOp::Pending(rx)
    }

    fn nip44_encrypt(
        &self,
        _recipient_pubkey: &str,
        _plaintext: &str,
    ) -> nmp_signer_iface::SignerOp<String> {
        nmp_signer_iface::SignerOp::err(nmp_signer_iface::SignerError::Unsupported(
            "test signer only supports sign".to_string(),
        ))
    }

    fn nip44_decrypt(
        &self,
        _sender_pubkey: &str,
        _ciphertext: &str,
    ) -> nmp_signer_iface::SignerOp<String> {
        nmp_signer_iface::SignerOp::err(nmp_signer_iface::SignerError::Unsupported(
            "test signer only supports sign".to_string(),
        ))
    }

    fn deliver_response(&self, _response_json: &str) {}
}

pub(super) fn record_of_kind(records: &[PublishRecord], kind: u32) -> &PublishRecord {
    records
        .iter()
        .find(|record| record.event.unsigned.kind == kind)
        .unwrap_or_else(|| panic!("expected pending publish record for kind:{kind}"))
}

pub(super) fn target_relays(record: &PublishRecord) -> Vec<String> {
    let mut relays: Vec<String> = record
        .per_relay
        .iter()
        .map(|(relay, _state)| relay.clone())
        .collect();
    relays.sort();
    relays
}

/// Pull out the most recent published event JSON the kernel emitted on the
/// wire so a test can assert on its tag shape.
pub(super) fn last_published_event_json(
    outbound: &[crate::relay::OutboundMessage],
) -> serde_json::Value {
    let frame = outbound
        .iter()
        .rev()
        .find(|m| m.text.starts_with("[\"EVENT\""))
        .expect("at least one EVENT frame");
    let parsed: serde_json::Value = serde_json::from_str(&frame.text).expect("EVENT frame is JSON");
    parsed
        .as_array()
        .and_then(|arr| arr.get(1).cloned())
        .expect("EVENT frame is [\"EVENT\", <event>]")
}

pub(super) fn tags_of(event_json: &serde_json::Value) -> Vec<Vec<String>> {
    event_json["tags"]
        .as_array()
        .expect("tags array")
        .iter()
        .map(|t| {
            t.as_array()
                .expect("tag is array")
                .iter()
                .map(|c| c.as_str().expect("tag column is string").to_string())
                .collect()
        })
        .collect()
}

/// Seed an existing kind:3 contact list for `author` containing `follows`,
/// using the kernel's verification-free replaceable-event injector so
/// `current_follows` reads it back. `created_at` is well in the past so a
/// subsequent `follow` command (stamped `now_secs()`) supersedes it.
pub(super) fn seed_contact_list(kernel: &mut Kernel, author: &str, follows: &[&str]) {
    let p_tags: Vec<Vec<String>> = follows
        .iter()
        .map(|p| vec!["p".to_string(), (*p).to_string()])
        .collect();
    kernel.inject_replaceable_event(
        &"3".repeat(64),
        author,
        1_700_000_000,
        3,
        p_tags,
        "wss://seed-relay.test",
        1,
    );
}

/// Produce a genuine flat NIP-01 JSON for a real signed event over `id`'s
/// active keys (kind:30023 article — generic, kind-agnostic).
pub(super) fn signed_nip01_json(id: &IdentityRuntime, content: &str) -> (String, String, String) {
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(), // ignored by signer
        kind: 30023,
        tags: vec![
            vec!["d".into(), "signed-test".into()],
            vec!["title".into(), "Signed".into()],
        ],
        content: content.into(),
        created_at: 1_700_000_000,
    };
    let signed = crate::actor::commands::identity::sign_active_nonblocking(id, &unsigned)
        .expect("sign_active_nonblocking ok")
        .poll()
        .expect("local sign resolves Ready immediately")
        .expect("sign produces a real signed event");
    let raw = crate::store::RawEvent {
        id: signed.id.clone(),
        pubkey: signed.unsigned.pubkey.clone(),
        created_at: signed.unsigned.created_at,
        kind: signed.unsigned.kind,
        tags: signed.unsigned.tags.clone(),
        content: signed.unsigned.content.clone(),
        sig: signed.sig.clone(),
    };
    let json = serde_json::to_string(&raw).expect("serialize flat NIP-01");
    (json, signed.id, signed.sig)
}

/// Produce a genuine signed kind:1059 (NIP-59 gift-wrap shape) RawEvent.
///
/// The body is a placeholder ciphertext — the gift-wrap construction's
/// authenticity gate is the outer Schnorr signature, and
/// `sign_active_nonblocking` mints a real Schnorr over the active keys.
/// `VerifiedEvent::try_from_raw` (the gate that runs first inside
/// `publish_signed_event`) accepts this as a well-formed signed event; only
/// the kernel-level D10 guard rejects it.
pub(super) fn signed_kind_1059_raw(id: &IdentityRuntime) -> crate::store::RawEvent {
    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: String::new(), // ignored by signer
        kind: 1059,
        tags: vec![vec![
            "p".into(),
            "0000000000000000000000000000000000000000000000000000000000000abc".into(),
        ]],
        content: "AAAA-placeholder-ciphertext".into(),
        created_at: 1_700_000_000,
    };
    let signed = crate::actor::commands::identity::sign_active_nonblocking(id, &unsigned)
        .expect("sign_active_nonblocking ok")
        .poll()
        .expect("local sign resolves Ready immediately")
        .expect("sign produces a real signed kind:1059 envelope");
    crate::store::RawEvent {
        id: signed.id.clone(),
        pubkey: signed.unsigned.pubkey.clone(),
        created_at: signed.unsigned.created_at,
        kind: signed.unsigned.kind,
        tags: signed.unsigned.tags.clone(),
        content: signed.unsigned.content.clone(),
        sig: signed.sig.clone(),
    }
}

// ── sub-module declarations ───────────────────────────────────────────────────

mod follow_relay_profile;
mod identity_account;
mod publish_signed;
mod publish_signed_d10;
mod publish_unsigned;
mod publish_unsigned_to;
mod snapshot_lifecycle;

// Issue #1246 kind:3 full-edit follow tests — separate sibling file so each
// file stays under 500 LOC. Child module so `use super::*` inherits the shared
// test helpers (`fresh`, `seed_contact_list`, `follow`, ...).
#[path = "../tests_follow_kind3_fulledit.rs"]
mod tests_follow_kind3_fulledit;
