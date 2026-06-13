//! Local replaceable-event publish projection tests.

use super::*;
use crate::actor::{new_event_observer_slot, register_rust_observer, KernelEventObserver};
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use crate::substrate::{KernelEvent, SignedEvent, UnsignedEvent};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

const FOLLOWED: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn signed_profile(keys: &::nostr::Keys, display_name: &str, created_at: u64) -> SignedEvent {
    let content = format!(r#"{{"display_name":"{display_name}","name":"{display_name}"}}"#);
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(0u16), content)
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

fn signed_relay_list(keys: &::nostr::Keys, write_url: &str, created_at: u64) -> SignedEvent {
    let event = ::nostr::EventBuilder::new(::nostr::Kind::from(10002u16), "")
        .tags([::nostr::Tag::parse(["r", write_url, "write"]).expect("valid r tag")])
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

/// #1193 (read-your-writes for kind:0): a locally published kind:0 profile
/// must fan out to registered `KernelEventObserver`s — the SAME seam the
/// relay ingest arm uses — AND populate the kernel's profile projection
/// immediately, without waiting for the relay echo (which dedups to
/// `Duplicate` and never re-fires fan-out). Retires the
/// `local_profile_intents` overlay (single-mechanism, ADR-0045 Rev 2).
#[test]
fn local_kind0_publish_fans_out_to_event_observers() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_profile(&keys, "Nova", 1_700_000_000);

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
        "local kind:0 publish must fire the observer fan-out exactly once"
    );
    let captured = observer
        .last
        .lock()
        .unwrap()
        .clone()
        .expect("observer must have received the locally published kind:0");
    assert_eq!(captured.kind, 0, "observed event must be the kind:0 profile");
    assert_eq!(captured.author, author, "observed event author == publisher");

    // The profile projection reflects the local edit immediately — this is the
    // read-your-writes property the overlay used to provide, now served by the
    // single store-first mechanism. `profile_card_for` reads through
    // `profile_for_pubkey`, so a non-placeholder display name proves the local
    // kind:0 landed in the canonical `profiles` cache.
    // D1 (#606): the `has_profile` render-gate boolean was removed; resolution
    // is proven by the real fields carrying the locally published kind:0 data
    // (a non-placeholder `display_name`) rather than a "loaded" flag.
    let card = kernel.profile_card_for(&author, "Waiting for kind:0 from indexer");
    assert_eq!(
        card.display_name.as_deref(),
        Some("Nova"),
        "profile display name must reflect the locally published kind:0"
    );
}

/// Read-your-writes for the relay echo (kind:0): after a local kind:0 publish
/// has already fired the observer fan-out, the relay's echo of the SAME event
/// id dedups to `Duplicate` in the store and must NOT fire the fan-out a
/// second time (D4).
#[test]
fn relay_echo_of_local_kind0_does_not_double_fire_observers() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_profile(&keys, "Nova", 1_700_000_000);

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
    // Duplicate, so the local-publish `Inserted | Replaced` gate is false and
    // fan-out must not fire again.
    let _ = kernel.inject_replaceable_event(
        &signed.id,
        &author,
        signed.unsigned.created_at,
        0,
        signed.unsigned.tags.clone(),
        "wss://write.test",
        2_000,
    );

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "relay echo of an already-published local kind:0 must NOT re-fire the observer"
    );
}

/// #1193 (generic replaceable arm): a locally published kind:10002 relay list
/// must notify registered `KernelEventObserver`s immediately — the SAME seam
/// the relay wildcard ingest arm uses — so routing/mailbox-driven projections
/// react to the local edit without waiting for the relay echo.
#[test]
fn local_kind10002_publish_notifies_event_observers_immediately() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let signed = signed_relay_list(&keys, "wss://write.test", 1_700_000_000);

    let slot = new_event_observer_slot();
    let observer = CapturingObserver::new();
    register_rust_observer(&slot, observer.clone());

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.set_event_observers_handle(slot);
    kernel.active_account = Some(author.clone());
    // Seed an OLDER kind:10002 (created_at < the published list) so the publish
    // engine's Nip65 outbox resolver has a current write relay to route to, yet
    // the freshly-published kind:10002 (created_at 1_700_000_000) supersedes it
    // in the store (`Replaced`) — the real production shape (you publish a newer
    // relay list to the relays your current list names). `seed_kind10002_for_test`
    // stamps `u64::MAX`, which would make any real-timestamp publish a no-op
    // `Superseded`, so we seed inline at an older timestamp here.
    kernel.inject_replaceable_event(
        &author,
        &author,
        1_699_000_000,
        10002,
        vec![vec![
            "r".to_string(),
            "wss://write.test".to_string(),
            "write".to_string(),
        ]],
        "wss://seed",
        500,
    );

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert!(!outbound.is_empty(), "publish should have an outbox target");

    assert_eq!(
        observer.count.load(Ordering::SeqCst),
        1,
        "local kind:10002 publish must notify the observer exactly once"
    );
    let captured = observer
        .last
        .lock()
        .unwrap()
        .clone()
        .expect("observer must have received the locally published kind:10002");
    assert_eq!(
        captured.kind, 10002,
        "observed event must be the kind:10002 relay list"
    );
    assert_eq!(captured.author, author, "observed event author == publisher");
}
