//! Tests for the pure NIP-59 seal/wrap functions (ADR-0072 §D5).
//!
//! [`gift_wrap_local`] composes the four pure steps end-to-end; the chain it
//! produces must round-trip through [`unwrap_gift_wrap`] to the original rumor.
//! [`build_seal_unsigned`] + [`wrap_signed_seal`] are exercised directly to prove
//! the granular halves the actor's port chain calls compose to the same result.
use super::*;
use crate::wrap::unwrap_gift_wrap;
use nostr::nips::nip44::{self, Version as Nip44Version};
use nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK;
use nostr::{EventBuilder, JsonUtil, Kind, Tag};

/// Build a kind:14 chat-message rumor for the given sender pubkey.
/// Mirrors the shape `nmp_nip17::build_dm_rumor` produces.
fn sample_rumor(sender_pubkey: PublicKey, content: &str) -> UnsignedEvent {
    EventBuilder::new(Kind::from_u16(14), content)
        .tag(Tag::public_key(sender_pubkey))
        .custom_created_at(Timestamp::from(1_700_000_000))
        .build(sender_pubkey)
}

#[test]
fn gift_wrap_local_round_trips_to_original_rumor() {
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let rumor = sample_rumor(sender.public_key(), "hello pure functions");

    let seal_ts = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
    let gift_wrap = gift_wrap_local(&sender, &receiver.public_key(), &rumor, seal_ts)
        .expect("local gift-wrap must succeed");

    assert_eq!(gift_wrap.kind, Kind::GiftWrap, "outer is kind:1059");
    // The outer wrap is signed by a fresh EPHEMERAL key, never the sender — the
    // NIP-59 unlinkability guarantee.
    assert_ne!(
        gift_wrap.pubkey,
        sender.public_key(),
        "outer wrap must NOT be signed by the sender (ephemeral unlinkability)"
    );

    let unwrapped = unwrap_gift_wrap(&receiver, &gift_wrap).expect("unwrap must succeed");
    assert_eq!(
        unwrapped.sender,
        sender.public_key(),
        "sender recovered from the verified seal"
    );
    assert_eq!(unwrapped.rumor.content, "hello pure functions");
    assert_eq!(u16::from(unwrapped.rumor.kind), 14);
}

#[test]
fn manual_compose_matches_gift_wrap_local() {
    // Prove the granular pure halves (the ones the actor port chain calls)
    // compose to the same observable result as the local convenience: seal +
    // wrap a rumor by hand, then round-trip it through unwrap.
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let rumor = sample_rumor(sender.public_key(), "manual compose");
    let seal_ts = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);

    // Step 1 — seal-content encrypt (the `Nip44EncryptForAccount` port verb in
    // the real chain; a local nip44 encrypt here).
    let ciphertext = nip44::encrypt(
        sender.secret_key(),
        &receiver.public_key(),
        &rumor.as_json(),
        Nip44Version::V2,
    )
    .expect("seal encrypt");

    // Step 2 — build the seal UnsignedEvent (pure, on-actor in the chain).
    let seal_unsigned = build_seal_unsigned(sender.public_key(), ciphertext, seal_ts);
    assert_eq!(seal_unsigned.kind, Kind::Seal, "seal is kind:13");
    assert_eq!(seal_unsigned.pubkey, sender.public_key());

    // Step 3 — sign the seal (the `SignEventForAccount` port verb in the chain).
    let seal_event = seal_unsigned.sign_with_keys(&sender).expect("seal sign");

    // Step 4 — wrap with a fresh ephemeral key (pure, in-process).
    let gift_wrap =
        wrap_signed_seal(&receiver.public_key(), &seal_event).expect("wrap signed seal");

    let unwrapped = unwrap_gift_wrap(&receiver, &gift_wrap).expect("unwrap");
    assert_eq!(unwrapped.sender, sender.public_key());
    assert_eq!(unwrapped.rumor.content, "manual compose");
}

/// NIP-59 §1 privacy: the kind:13 seal and the kind:1059 outer wrap carry
/// INDEPENDENTLY randomized `created_at` values so a relay cannot correlate the
/// two events by their timestamp.
///
/// Build N=20 gift wraps and assert at least one pair carries distinct
/// timestamps. With two draws from a uniform ~2-day window the probability of
/// all N pairs being equal is astronomically small.
#[test]
fn wrap_and_seal_timestamps_are_independently_randomized() {
    let sender = Keys::generate();
    let receiver = Keys::generate();

    let mut found_distinct = false;
    const ATTEMPTS: usize = 20;

    for i in 0..ATTEMPTS {
        let rumor = sample_rumor(sender.public_key(), &format!("ts-independence-{i}"));
        let seal_ts = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);

        let gift_wrap = gift_wrap_local(&sender, &receiver.public_key(), &rumor, seal_ts)
            .expect("local gift-wrap must succeed");
        assert_eq!(gift_wrap.kind, Kind::GiftWrap);

        // Decrypt the outer wrap (receiver key + ephemeral pubkey on envelope).
        let seal_json =
            nip44::decrypt(receiver.secret_key(), &gift_wrap.pubkey, &gift_wrap.content)
                .expect("outer wrap must decrypt");
        let seal_event = Event::from_json(&seal_json).expect("seal JSON");
        assert_eq!(seal_event.kind, Kind::Seal);

        if gift_wrap.created_at != seal_event.created_at {
            found_distinct = true;
            break;
        }
    }

    assert!(
        found_distinct,
        "after {ATTEMPTS} gift wraps, seal and outer wrap timestamps were ALWAYS equal — \
         they must be drawn independently (NIP-59 §1 privacy requirement)"
    );
}

#[test]
fn every_wrap_mints_a_distinct_ephemeral_outer_key() {
    // Unlinkability: two wraps of the same rumor to the same receiver must NOT
    // share an outer pubkey (a fresh ephemeral per call).
    let sender = Keys::generate();
    let receiver = Keys::generate();
    let rumor = sample_rumor(sender.public_key(), "distinct ephemeral");
    let ts = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);

    let a = gift_wrap_local(&sender, &receiver.public_key(), &rumor, ts).expect("wrap a");
    let b = gift_wrap_local(&sender, &receiver.public_key(), &rumor, ts).expect("wrap b");

    assert_ne!(
        a.pubkey, b.pubkey,
        "each gift-wrap must carry a distinct ephemeral outer pubkey"
    );
}
