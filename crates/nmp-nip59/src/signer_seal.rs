//! NIP-59 seal (kind:13) + gift-wrap (kind:1059) — **pure functions**.
//!
//! # ADR-0072 §D5 — the `SignerForSeal` execution model is gone
//!
//! ADR-0072 once routed the seal step through a `SignerForSeal` trait so a
//! remote signer (NIP-46 bunker, NIP-07, hardware) could produce the kind:13
//! NIP-44 ciphertext out of process. That design carried a per-invocation
//! **driver thread** and two wall-clock timeout constants
//! (`DRIVER_STEP_TIMEOUT`, `GIFT_WRAP_TOTAL_TIMEOUT`) inside this crate — a
//! second, parallel waiting mechanism on top of the actor's own signer port.
//!
//! ADR-0072 §D5 deletes that whole execution model. The seal step is now a
//! continuation chain through the actor's three-verb signer port
//! (`Nip44EncryptForAccount` → `SignEventForAccount` → `PublishSignedEvent`,
//! composed in `nmp-nip17::SendGiftWrappedDmCommand`). This crate keeps only the
//! **pure, synchronous building blocks** that chain composes from:
//!
//! - [`build_seal_unsigned`] — assemble the kind:13 seal `UnsignedEvent` from an
//!   already-produced NIP-44 ciphertext (whoever produced it — the port for the
//!   DM chain, local `Keys` for Marmot). No key material.
//! - [`wrap_signed_seal`] — locally wrap a signed kind:13 seal in a kind:1059
//!   envelope under a freshly-minted ephemeral key (the NIP-59 unlinkability
//!   guarantee — the ephemeral key never escapes this function).
//! - [`gift_wrap_local`] — the **local-keys-only** convenience that composes the
//!   four pure steps end-to-end (NIP-44 encrypt with the caller's own `Keys`,
//!   [`build_seal_unsigned`], sign in process, [`wrap_signed_seal`]). Used by
//!   `nmp-marmot` (local-key-only by construction) and the integration tests.
//!   Synchronous; no trait object, no `Arc`, no thread, no `SignerOp`.
//!
//! Remote-signer DM sends never call [`gift_wrap_local`] — they drive the seal
//! through the port one verb at a time and assemble the wrap with
//! [`build_seal_unsigned`] + [`wrap_signed_seal`] on the actor thread.

use nostr::{
    nips::nip44::{self, Version as Nip44Version},
    Event, EventBuilder, JsonUtil, Keys, Kind, PublicKey, SecretKey, Timestamp, UnsignedEvent,
};

use crate::error::Nip59Error;

/// Build the kind:13 seal `UnsignedEvent` from an already-encrypted payload.
///
/// Mirrors `nostr::nips::nip59::make_seal` but takes the NIP-44 ciphertext as
/// **input** (instead of re-encrypting), so the function is agnostic to which
/// path produced it: the actor's `Nip44EncryptForAccount` port for the
/// remote/local DM chain (§D5), or a local `Keys` NIP-44 encrypt for Marmot.
/// No key material crosses this boundary (D13).
///
/// `created_at` is stamped on the seal; callers pass a NIP-59-tweaked value
/// (`Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK)`). The kind:1059 outer
/// wrap draws its OWN independent timestamp in [`wrap_signed_seal`] (NIP-59 §1).
#[must_use]
pub fn build_seal_unsigned(
    sender_pubkey: PublicKey,
    encrypted_content: String,
    created_at: Timestamp,
) -> UnsignedEvent {
    EventBuilder::new(Kind::Seal, encrypted_content)
        .custom_created_at(created_at)
        .build(sender_pubkey)
}

/// Locally wrap a signed kind:13 seal in a kind:1059 envelope using a
/// freshly-minted ephemeral key. Always runs in process — the ephemeral key
/// never leaves this function (the unlinkability guarantee per NIP-59 §1).
///
/// The wrap timestamp is drawn INDEPENDENTLY via a fresh
/// `Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK)` call, separate from the
/// seal's `created_at`. This matches the `nostr` 0.44 `EventBuilder::gift_wrap`
/// helper, which calls `Timestamp::tweaked` once inside `make_seal` (for the
/// seal) and again inside `gift_wrap_from_seal` (for the wrap). Two independent
/// draws prevent a relay from correlating the seal and wrap by their timestamps
/// (NIP-59 §1 privacy requirement).
///
/// # Errors
///
/// Returns [`Nip59Error::Nostr`] if the outer NIP-44 encrypt or the ephemeral
/// signature fails (a secp256k1 backend failure — never expected in practice).
pub fn wrap_signed_seal(receiver: &PublicKey, seal_event: &Event) -> Result<Event, Nip59Error> {
    // Draw an independent timestamp for the outer wrap — NOT reusing the
    // seal's created_at. NIP-59 §1 requires independently randomized
    // timestamps on both envelopes.
    let wrap_created_at = Timestamp::tweaked(nostr::nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK);

    // Mint a fresh ephemeral keypair for the outer wrap. NEVER reused —
    // the unlinkability property depends on every kind:1059 envelope
    // carrying a distinct outer pubkey.
    let ephemeral_sk = SecretKey::generate();
    let ephemeral = Keys::new(ephemeral_sk);

    // Encrypt the seal JSON to the receiver, using the EPHEMERAL secret
    // (NOT the sender's). This is what gives the outer envelope its
    // "anyone could have sent this" property.
    let seal_json = seal_event.as_json();
    let outer_content = nip44::encrypt(
        ephemeral.secret_key(),
        receiver,
        &seal_json,
        Nip44Version::V2,
    )
    .map_err(|e| Nip59Error::Nostr(format!("outer wrap nip44_encrypt: {e}")))?;

    // Build + sign the kind:1059 envelope with the ephemeral key.
    let event = EventBuilder::new(Kind::GiftWrap, outer_content)
        .custom_created_at(wrap_created_at)
        .tag(nostr::Tag::public_key(*receiver))
        .sign_with_keys(&ephemeral)
        .map_err(|e| Nip59Error::Nostr(format!("outer wrap sign: {e}")))?;
    Ok(event)
}

/// Seal (kind:13) + gift-wrap (kind:1059) a rumor with the sender's **local
/// keys**, synchronously and end-to-end. The local-keys-only composition of the
/// pure building blocks ([`build_seal_unsigned`] + [`wrap_signed_seal`]) plus an
/// in-process NIP-44 encrypt and seal signature.
///
/// This is the path for callers that are local-key-only **by construction** —
/// `nmp-marmot` (which holds its own MLS identity `Keys`) and the crate's
/// integration tests. Remote-signer DM sends do NOT use this function: they
/// drive the seal step through the actor's `Nip44EncryptForAccount` /
/// `SignEventForAccount` port and call the two pure halves on the actor thread
/// (ADR-0072 §D5). There is no trait object, `Arc`, thread, or `SignerOp` here.
///
/// - `sender` signs the kind:13 seal and provides the NIP-44 ECDH secret for the
///   seal content. The seal's pubkey is `sender.public_key()`.
/// - `receiver` is the gift-wrap recipient; the kind:1059 outer envelope is
///   freshly ephemeral per call (NIP-59 unlinkability). NIP-17 calls this once
///   per receiver (recipient + self-copy).
/// - `seal_created_at` is stamped on the kind:13 seal; pass a NIP-59-tweaked
///   value. The kind:1059 wrap receives its own independent tweak inside
///   [`wrap_signed_seal`].
///
/// # Errors
///
/// Returns [`Nip59Error::Nostr`] if the seal NIP-44 encrypt, the seal signature,
/// or the outer wrap fails.
pub fn gift_wrap_local(
    sender: &Keys,
    receiver: &PublicKey,
    rumor: &UnsignedEvent,
    seal_created_at: Timestamp,
) -> Result<Event, Nip59Error> {
    // Step 1 — seal-content encrypt with the sender's own secret (NIP-44 ECDH
    // sender → receiver). The seal content is `nip44_encrypt(rumor.as_json())`.
    let rumor_json = rumor.as_json();
    let ciphertext = nip44::encrypt(sender.secret_key(), receiver, &rumor_json, Nip44Version::V2)
        .map_err(|e| Nip59Error::Nostr(format!("seal nip44_encrypt: {e}")))?;

    // Step 2 — build the kind:13 seal UnsignedEvent from the ciphertext.
    let seal_unsigned = build_seal_unsigned(sender.public_key(), ciphertext, seal_created_at);

    // Step 3 — sign the seal in process with the sender's keys.
    let seal_event = seal_unsigned
        .sign_with_keys(sender)
        .map_err(|e| Nip59Error::Nostr(format!("seal sign: {e}")))?;

    // Step 4 — wrap with a fresh ephemeral key (in process).
    wrap_signed_seal(receiver, &seal_event)
}

#[cfg(test)]
#[path = "signer_seal/tests.rs"]
mod tests;
