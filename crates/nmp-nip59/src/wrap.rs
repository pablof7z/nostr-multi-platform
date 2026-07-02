//! NIP-59 unwrap: the kind:1059 → seal → rumor decode path, split into **pure
//! parse halves** so ADR-0072 Stage 4 can route the two NIP-44 decrypts through
//! the actor's signer port (a NIP-46 bunker decrypts the wrap/seal out of
//! process; the DM inbox no longer holds raw `Keys`).
//!
//! Unsealing ONE kind:1059 envelope needs TWO sequential NIP-44 decrypts:
//!
//! 1. **outer** — decrypt `gift_wrap.content` against the ephemeral wrap pubkey
//!    (`gift_wrap.pubkey`) → the kind:13 seal JSON.
//! 2. **inner** — decrypt `seal.content` against the seal author
//!    (`seal.pubkey`) → the rumor JSON.
//!
//! The decrypt step is the only part that needs key material. Everything else —
//! the kind check, seal-event parse + signature verify, rumor parse, and the
//! rumor-author-matches-seal check — is pure and lives in the three functions
//! below. Stage 4's port-driven inbox calls these around two
//! `Nip44DecryptForAccount` continuations; [`unwrap_gift_wrap`] composes them
//! around two LOCAL `nostr::nips::nip44::decrypt` calls for the local-keys path
//! (and the crate's tests).

use alloc::format;
use alloc::string::String;
use nostr::nips::nip44;
use nostr::{Event, JsonUtil, Keys, Kind, PublicKey, UnsignedEvent};

use crate::error::Nip59Error;

/// Unwrapped NIP-59 gift-wrap: the sender's public key and the inner rumor.
///
/// This mirrors `nostr::nips::nip59::UnwrappedGift` but is re-exported from
/// this crate's public surface so callers do not need to depend directly on
/// the `nostr` crate's internal NIP module paths.
#[derive(Debug, Clone)]
pub struct UnwrappedGift {
    /// Public key of the sender, extracted from the verified seal (kind:13).
    pub sender: PublicKey,
    /// The inner rumor (`UnsignedEvent`) extracted from the seal.
    pub rumor: UnsignedEvent,
}

/// Pure half 1 — validate a kind:1059 envelope and extract the
/// `(outer_ciphertext, ephemeral_peer)` pair the **outer** NIP-44 decrypt needs.
///
/// `peer` is the gift-wrap's own (ephemeral) pubkey — the key the receiver
/// decrypts the outer content against. No key material is read here; the caller
/// (Stage 4 port, or [`unwrap_gift_wrap`] locally) performs the decrypt.
///
/// `recipient` is the active account this envelope is being unwrapped for. As a
/// cheap defense-in-depth (issue #1265), an envelope whose `#p` tag does not
/// address `recipient` is rejected up front so we never burn a NIP-44 decrypt —
/// nor, on a bunker signer, an out-of-process round-trip — on a kind:1059 that
/// was never addressed to us. The authoritative recipient check still happens
/// when the outer decrypt itself fails, but rejecting on the public `#p` tag is
/// both free and fail-closed.
///
/// # Errors
///
/// [`Nip59Error::NotGiftWrap`] if `gift_wrap.kind != 1059`, or if the envelope's
/// `#p` tag does not address `recipient`.
pub fn parse_outer_for_decrypt(
    gift_wrap: &Event,
    recipient: &PublicKey,
) -> Result<(String, PublicKey), Nip59Error> {
    if gift_wrap.kind != Kind::GiftWrap {
        return Err(Nip59Error::NotGiftWrap);
    }
    // Defense-in-depth: a kind:1059 must carry a `#p` tag addressing the active
    // account. NIP-59 wraps are public; the `#p` is the cleartext routing hint.
    // A wrap not addressed to us is not ours to decrypt — reject before the port.
    if !addresses_recipient(gift_wrap, recipient) {
        return Err(Nip59Error::NotGiftWrap);
    }
    Ok((gift_wrap.content.clone(), gift_wrap.pubkey))
}

/// Whether `gift_wrap` carries a `#p` tag whose value is `recipient` (hex).
fn addresses_recipient(gift_wrap: &Event, recipient: &PublicKey) -> bool {
    let recipient_hex = recipient.to_hex();
    gift_wrap.tags.iter().any(|tag| {
        let slice = tag.as_slice();
        slice.first().map(String::as_str) == Some("p")
            && slice.get(1).map(String::as_str) == Some(recipient_hex.as_str())
    })
}

/// Pure half 2 — parse + signature-verify the decrypted kind:13 seal, then
/// extract the `(seal_event, inner_ciphertext, seal_author)` the **inner** rumor
/// decrypt needs. `seal_plaintext` is the output of the outer decrypt (half 1).
///
/// The returned [`Event`] is the verified seal; `inner_ciphertext` is its NIP-44
/// content; the [`PublicKey`] is `seal.pubkey` — the peer the rumor is decrypted
/// against. No key material is read here.
///
/// # Errors
///
/// [`Nip59Error::Nostr`] if the seal JSON is malformed or its signature fails to
/// verify.
pub fn parse_seal_for_decrypt(
    seal_plaintext: &str,
) -> Result<(Event, String, PublicKey), Nip59Error> {
    let seal = Event::from_json(seal_plaintext).map_err(Nip59Error::from)?;
    let secp = nostr::secp256k1::Secp256k1::verification_only();
    seal.verify_with_ctx(&secp)
        .map_err(|e| Nip59Error::Nostr(format!("seal verify: {e}")))?;
    let ciphertext = seal.content.clone();
    let author = seal.pubkey;
    Ok((seal, ciphertext, author))
}

/// Pure half 3 — parse the decrypted rumor and enforce the
/// rumor-author-matches-seal invariant (NIP-59 anti-spoofing). `seal` is the
/// verified seal from half 2; `rumor_plaintext` is the output of the inner
/// decrypt.
///
/// # Errors
///
/// [`Nip59Error::Nostr`] if the rumor JSON is malformed; [`Nip59Error::SenderMismatch`]
/// if the rumor author differs from the seal author (a spoofing attempt).
pub fn parse_rumor(seal: &Event, rumor_plaintext: &str) -> Result<UnwrappedGift, Nip59Error> {
    let rumor = UnsignedEvent::from_json(rumor_plaintext).map_err(Nip59Error::from)?;
    if rumor.pubkey != seal.pubkey {
        return Err(Nip59Error::SenderMismatch);
    }
    Ok(UnwrappedGift {
        sender: seal.pubkey,
        rumor,
    })
}

/// Unwrap an incoming kind:1059 gift-wrap with the receiver's **local keys**:
/// verify the seal → extract the rumor. Composes the three pure halves
/// ([`parse_outer_for_decrypt`], [`parse_seal_for_decrypt`], [`parse_rumor`])
/// around two LOCAL `nostr::nips::nip44::decrypt` calls.
///
/// This is the local-keys path (Marmot, the NIP-17 inbox before Stage 4, and
/// this crate's tests). ADR-0072 Stage 4 will route the inbox's two decrypts
/// through `Nip44DecryptForAccount` so a NIP-46 bunker can unwrap without the
/// inbox ever holding raw `Keys`; it reuses the same three pure halves.
///
/// # Errors
///
/// Returns [`Nip59Error`] if the gift-wrap is malformed, a decrypt fails, the
/// seal cannot be verified, or the rumor author does not match the seal.
pub fn unwrap_gift_wrap(receiver: &Keys, gift_wrap: &Event) -> Result<UnwrappedGift, Nip59Error> {
    // Outer: decrypt the wrap content against the ephemeral wrap pubkey → seal.
    let (outer_ciphertext, ephemeral_peer) =
        parse_outer_for_decrypt(gift_wrap, &receiver.public_key())?;
    let seal_plaintext = nip44::decrypt(receiver.secret_key(), &ephemeral_peer, &outer_ciphertext)
        .map_err(|e| Nip59Error::Nostr(format!("outer nip44_decrypt: {e}")))?;

    // Seal: parse + verify → inner ciphertext + seal author.
    let (seal, inner_ciphertext, seal_author) = parse_seal_for_decrypt(&seal_plaintext)?;

    // Inner: decrypt the seal content against the seal author → rumor.
    let rumor_plaintext = nip44::decrypt(receiver.secret_key(), &seal_author, &inner_ciphertext)
        .map_err(|e| Nip59Error::Nostr(format!("inner nip44_decrypt: {e}")))?;

    parse_rumor(&seal, &rumor_plaintext)
}
