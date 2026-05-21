//! NIP-17 gift-wrapped DM send handler.
//!
//! `ActorCommand::SendGiftWrappedDm` arrives carrying an **unsigned** kind:14
//! chat-message rumor (built host-side by `nmp_nip17::build_dm_rumor`). This
//! handler:
//!
//! 1. Resolves the active account's local `nostr::Keys`. A remote (NIP-46)
//!    signer exposes no local key — sealing a NIP-59 rumor is not a single
//!    "sign this event" op a remote signer can serve — so that case is a
//!    graceful `Err` surfaced as a toast, never a panic (D6).
//! 2. Re-stamps `rumor.created_at` from `kernel.now_secs()` (D7 — the kernel
//!    owns the wall clock; the host sends `0` as a sentinel).
//! 3. Gift-wraps the rumor TWICE via `nmp_nip59::gift_wrap`: once to the
//!    recipient, once to the sender's own pubkey (the self-copy, so sent
//!    messages stay readable). Each call mints a fresh ephemeral key for the
//!    outer kind:1059 envelope — the unlinkability guarantee.
//! 4. Publishes both kind:1059 envelopes to the configured Content relays via
//!    the explicit-target publish path. The envelopes are already signed (by
//!    their ephemeral keys); they MUST NOT be re-signed with the account key,
//!    which would destroy unlinkability — so they route through
//!    `publish_signed_event`, not the unsigned publish path.
//!
//! # Phase 1 relay routing
//!
//! Each envelope SHOULD go to the recipient's kind:10050 DM-relay list. That
//! lookup is a follow-up; Phase 1 falls back to the actor's configured Content
//! relays for both envelopes. See the `TODO(nip17-dm-relays)` below.

use nostr::{EventBuilder, Kind, PublicKey, Tag, Timestamp};

use crate::actor::commands::identity::IdentityRuntime;
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::store::RawEvent;
use crate::substrate::UnsignedEvent;

/// Seal + gift-wrap a NIP-17 kind:14 rumor and publish the kind:1059 envelopes.
///
/// Returns the outbound wire frames for both envelopes (recipient + self-copy),
/// or an empty vec when the send could not proceed — in which case a toast has
/// been set on the kernel (D6: the error is observable state, never silent).
pub(crate) fn send_gift_wrapped_dm(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    mut rumor: UnsignedEvent,
    recipient_pubkey: &str,
) -> Vec<OutboundMessage> {
    // 1. Active local keys. A remote (NIP-46) signer has no local secret key;
    //    gift-wrap sealing cannot run through the remote-sign RPC, so this is a
    //    graceful degrade — surface a toast, publish nothing (D6).
    let Some(keys) = identity.active_local_keys() else {
        kernel.set_last_error_toast(Some(
            "cannot send DM: gift-wrap needs a local key — remote (bunker) \
             signers are not yet supported for NIP-17"
                .to_string(),
        ));
        return Vec::new();
    };

    // 2. D7: re-stamp the rumor timestamp from the kernel clock. The host sends
    //    `created_at: 0` as the sentinel; the kernel owns the wall clock.
    if rumor.created_at == 0 {
        rumor.created_at = kernel.now_secs();
    }

    // 3. Convert the substrate rumor → `nostr::UnsignedEvent`. The rumor is
    //    NEVER signed; `EventBuilder::build` produces the unsigned form that
    //    `gift_wrap` seals.
    let nostr_rumor = match build_nostr_rumor(&rumor, keys.public_key()) {
        Ok(r) => r,
        Err(reason) => {
            kernel.set_last_error_toast(Some(format!("cannot send DM: {reason}")));
            return Vec::new();
        }
    };

    // Recipient pubkey must parse — a malformed hex pubkey is a caller bug;
    // refuse the send rather than wrap to a garbage key (D6).
    let recipient = match PublicKey::parse(recipient_pubkey) {
        Ok(pk) => pk,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!(
                "cannot send DM: malformed recipient pubkey: {e}"
            )));
            return Vec::new();
        }
    };
    let sender = keys.public_key();

    // 4. Gift-wrap TWICE — fresh ephemeral outer key per call (NIP-59).
    //    Envelope A: wrapped to the recipient.
    //    Envelope B: the self-copy, wrapped to the sender's own pubkey.
    let mut outbound = Vec::new();
    for (label, receiver) in [("recipient", &recipient), ("self-copy", &sender)] {
        let envelope = match nmp_nip59::gift_wrap(keys, receiver, nostr_rumor.clone(), None) {
            Ok(ev) => ev,
            Err(e) => {
                kernel.set_last_error_toast(Some(format!(
                    "cannot send DM: gift-wrap ({label}) failed: {e}"
                )));
                return Vec::new();
            }
        };
        // The kind:1059 envelope is already signed by its ephemeral key. Route
        // it through the signed-event publish path so the kernel verifies and
        // forwards it VERBATIM — re-signing with the account key would destroy
        // the unlinkability gift-wrap exists to provide.
        let raw = nostr_event_to_raw(&envelope);
        // TODO(nip17-dm-relays): resolve the receiver's kind:10050 DM-relay
        // list and pin the envelope there. Phase 1 routes both envelopes to
        // the actor's configured Content relays (empty → NIP-65 outbox of the
        // ephemeral pubkey, a defensive degrade).
        let relays = kernel.bootstrap_urls_for_role(crate::relay::RelayRole::Content);
        outbound.extend(super::publish::publish_signed_event(kernel, raw, &relays));
    }

    outbound
}

/// Build a `nostr::UnsignedEvent` (the rumor) from the substrate flat repr.
///
/// Mirrors `commands::publish::sign_with`'s tag/kind validation, but stops at
/// `EventBuilder::build` — the rumor is unsigned by design (NIP-59 seals it).
fn build_nostr_rumor(
    rumor: &UnsignedEvent,
    pubkey: PublicKey,
) -> Result<nostr::UnsignedEvent, String> {
    if rumor.kind > u16::MAX as u32 {
        return Err(format!(
            "invalid kind {}: must be in range [0, 65535]",
            rumor.kind
        ));
    }
    let kind = Kind::from_u16(rumor.kind as u16);

    let mut tags = Vec::with_capacity(rumor.tags.len());
    let mut malformed = 0usize;
    for t in &rumor.tags {
        match Tag::parse(t) {
            Ok(tag) => tags.push(tag),
            Err(_) => malformed += 1,
        }
    }
    if malformed > 0 {
        return Err(format!("dropped {malformed} malformed tag(s)"));
    }

    Ok(EventBuilder::new(kind, &rumor.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(rumor.created_at))
        .build(pubkey))
}

/// Convert a signed `nostr::Event` (the kind:1059 gift-wrap) to the kernel's
/// flat [`RawEvent`]. The signature and id are carried through verbatim — the
/// signed-event publish path verifies them and forwards the event unchanged.
fn nostr_event_to_raw(event: &nostr::Event) -> RawEvent {
    RawEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: event.kind.as_u16() as u32,
        tags: event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::commands::identity::sign_in_nsec;
    use crate::actor::commands::new_bunker_handshake_slot;
    use crate::actor::ActorCommand;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const RECIPIENT: &str =
        "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

    fn fresh() -> (IdentityRuntime, Kernel) {
        (
            IdentityRuntime::new(new_bunker_handshake_slot()),
            Kernel::new(DEFAULT_VISIBLE_LIMIT),
        )
    }

    /// A kind:14 rumor with a `created_at: 0` sentinel — what
    /// `nmp_nip17::build_dm_rumor` produces.
    fn sample_rumor(sender_pubkey: &str) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: sender_pubkey.to_string(),
            kind: 14,
            tags: vec![vec!["p".to_string(), RECIPIENT.to_string()]],
            content: "hello over NIP-17".to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn send_gift_wrapped_dm_without_account_toasts_and_emits_nothing() {
        let (identity, mut kernel) = fresh();
        let rumor = sample_rumor(
            "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
        );
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, RECIPIENT);
        assert!(
            outbound.is_empty(),
            "no active account → no envelopes published"
        );
        assert!(
            kernel.last_error_toast_snapshot().is_some(),
            "D6: the failure is surfaced as a toast, never silent"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_rejects_malformed_recipient_pubkey() {
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");
        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, "not-a-pubkey");
        assert!(outbound.is_empty(), "malformed recipient → nothing published");
        assert!(
            kernel
                .last_error_toast_snapshot()
                .map(|t| t.contains("recipient pubkey"))
                .unwrap_or(false),
            "D6: malformed recipient pubkey is surfaced as a toast"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_with_local_key_gift_wraps_recipient_and_self() {
        // With a local nsec the handler must seal+wrap the rumor twice (one
        // envelope per recipient, one self-copy) and publish both — no toast.
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");
        kernel.seed_kind10002_for_test(&sender, &["wss://dm-relay.test"]);

        // NIP-59 gift-wrap performs a NIP-44 ECDH against the recipient key, so
        // the recipient pubkey MUST be a real secp256k1 curve point. Derive one
        // from a freshly generated keypair rather than a hand-typed hex string.
        let recipient_pk = nostr::Keys::generate().public_key().to_hex();

        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk);

        assert!(
            kernel.last_error_toast_snapshot().is_none(),
            "a local-key gift-wrap send must not toast an error: {:?}",
            kernel.last_error_toast_snapshot()
        );
        // Two kind:1059 envelopes (recipient + self-copy) were published; each
        // produces at least one outbound EVENT frame to the configured relay.
        assert!(
            !outbound.is_empty(),
            "both gift-wrap envelopes should produce outbound frames"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_variant_is_matched_in_dispatch() {
        // Compile-time guard: the `ActorCommand::SendGiftWrappedDm` variant
        // exists with the documented shape and constructs cleanly. The actual
        // dispatch arm is exercised end-to-end by the actor loop tests; this
        // pins the variant signature so a rename breaks the build here.
        let cmd = ActorCommand::SendGiftWrappedDm {
            rumor: sample_rumor(
                "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
            ),
            recipient_pubkey: RECIPIENT.to_string(),
        };
        match cmd {
            ActorCommand::SendGiftWrappedDm {
                rumor,
                recipient_pubkey,
            } => {
                assert_eq!(rumor.kind, 14, "the carried rumor is a kind:14");
                assert_eq!(recipient_pubkey, RECIPIENT);
            }
            _ => panic!("expected SendGiftWrappedDm variant"),
        }
    }
}
