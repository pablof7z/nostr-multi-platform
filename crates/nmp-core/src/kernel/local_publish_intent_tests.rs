//! Local replaceable-event publish projection tests.

use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{KernelEvent, SignedEvent, UnsignedEvent};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const FOLLOWED: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn signed_contact_list(keys: &::nostr::Keys, follow: &str, created_at: u64) -> SignedEvent {
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(3u16), "")
        .tags([::nostr::Tag::parse(["p", follow]).expect("valid p tag")])
        .custom_created_at(::nostr::Timestamp::from_secs(created_at))
        .sign_with_keys(keys)
        .expect("generated keys sign");
    SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: event.kind.as_u16() as u32,
            tags: event
                .tags
                .iter()
                .map(|tag: &::nostr::Tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    }
}

/// V-112 (ADR-0042): `author_view.primary_action` was deleted with the author
/// view state machine. The underlying property being tested — that publishing a
/// kind:3 contact list updates `kernel.contacts` — is now observed directly.
#[test]
fn local_kind3_publish_updates_contacts_set() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_contact_list(&keys, FOLLOWED, 1_700_000_000);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    // Before publishing kind:3, FOLLOWED is not in seed_contacts for this author.
    assert!(
        kernel
            .seed_contacts
            .get(&author)
            .map_or(true, |follows| !follows.contains(&FOLLOWED.to_string())),
        "precondition: FOLLOWED must not be in seed_contacts before publish"
    );

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);

    assert!(!outbound.is_empty(), "publish should have an outbox target");
    // After publishing kind:3 with FOLLOWED in the p-tags, seed_contacts is updated.
    assert!(
        kernel
            .seed_contacts
            .get(&author)
            .map_or(false, |follows| follows.contains(&FOLLOWED.to_string())),
        "FOLLOWED must be in seed_contacts[author] after kind:3 publish"
    );
}

/// A `KernelEventObserver` that records every event it receives.
struct CapturingObserver {
    count: AtomicU32,
    last: Mutex<Option<KernelEvent>>,
}

impl CapturingObserver {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            count: AtomicU32::new(0),
            last: Mutex::new(None),
        })
    }
}

impl KernelEventObserver for CapturingObserver {
    fn on_kernel_event(&self, event: &KernelEvent) {
        self.count.fetch_add(1, Ordering::SeqCst);
        if let Ok(mut guard) = self.last.lock() {
            *guard = Some(event.clone());
        }
    }
}

/// FINDING A (read-your-writes): a locally published kind:3 contact list must
/// fan out to registered `KernelEventObserver`s — the SAME seam the relay
/// ingest arm uses — so sidecar projections (`FollowListProjection`,
/// `ActiveFollowSet`) update immediately, without waiting for the relay echo
/// (which dedups to `Duplicate` and never re-fires fan-out) or an account
/// switch / restart.
#[test]
fn local_kind3_publish_fans_out_to_event_observers() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_contact_list(&keys, FOLLOWED, 1_700_000_000);

    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert!(!outbound.is_empty(), "publish should have an outbox target");

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "local kind:3 publish must fire the observer fan-out exactly once"
    );
    let captured = observer
        .last
        .lock()
        .unwrap()
        .clone()
        .expect("observer must have received the locally published kind:3");
    assert_eq!(
        captured.kind, 3,
        "observed event must be the kind:3 contacts list"
    );
    assert_eq!(captured.author, author, "observed event author == publisher");
    assert!(
        captured.tags.iter().any(|t| t.first().map(String::as_str)
            == Some("p")
            && t.get(1).map(String::as_str) == Some(FOLLOWED)),
        "observed kind:3 must carry the followed pubkey in its p-tags"
    );
}

/// Read-your-writes for the relay echo: after a local kind:3 publish has
/// already fired the observer fan-out, the relay's echo of the SAME event id
/// dedups to `Duplicate` in the store and must NOT fire the fan-out a second
/// time (D4 — observers fire exactly once per accepted event, never on the
/// duplicate echo).
#[test]
fn relay_echo_of_local_kind3_does_not_double_fire_observers() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_contact_list(&keys, FOLLOWED, 1_700_000_000);

    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);
    kernel.active_account = Some(author.clone());
    kernel.seed_kind10002_for_test(&author, &["wss://write.test"]);

    kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "local publish fires once"
    );

    // The relay echoes the same signed event id back. The store returns
    // Duplicate, so the relay kind:3 arm's `Inserted | Replaced` gate is false
    // and fan-out must not fire again.
    let _ = kernel.inject_replaceable_event(
        &signed.id,
        &author,
        signed.unsigned.created_at,
        3,
        signed.unsigned.tags.clone(),
        "wss://write.test",
        2_000,
    );

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "relay echo of an already-published local kind:3 must NOT re-fire the observer"
    );
}
