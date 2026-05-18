//! Core NIP-59 free functions: `gift_wrap` and `unwrap_gift_wrap`.
//!
//! Both functions are synchronous wrappers over the async `nostr` 0.44 API.
//! The `Keys` NIP-44 operations are CPU-only with no real suspension points;
//! `futures::executor::block_on` bridges to a synchronous call-site without
//! pulling in tokio.

use nostr::{Event, EventBuilder, Keys, PublicKey, Tag, UnsignedEvent};

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

/// Seal (kind:13, NIP-44 from sender) + gift-wrap (kind:1059, NIP-44 from
/// ephemeral key). Thin wrapper over `nostr::EventBuilder::gift_wrap`.
///
/// # Seam note
///
/// This function requires the caller to hold sender `Keys`. In the post-v1
/// Marmot flow the actor's signer-bridge will hold keys via
/// `KeyringCapability`; for this milestone callers invoke this free function
/// directly.
pub fn gift_wrap(
    sender: &Keys,
    receiver: &PublicKey,
    rumor: UnsignedEvent,
    expiration: Option<nostr::Timestamp>,
) -> Result<Event, Nip59Error> {
    let extra_tags: Vec<Tag> = expiration
        .map(|ts| vec![Tag::expiration(ts)])
        .unwrap_or_default();

    futures::executor::block_on(
        EventBuilder::gift_wrap(sender, receiver, rumor, extra_tags),
    )
    .map_err(Nip59Error::from)
}

/// Unwrap an incoming kind:1059 gift-wrap event: verify the seal → extract
/// the rumor. Thin wrapper over
/// `nostr::nips::nip59::UnwrappedGift::from_gift_wrap`.
pub fn unwrap_gift_wrap(receiver: &Keys, gift_wrap: &Event) -> Result<UnwrappedGift, Nip59Error> {
    let inner = futures::executor::block_on(
        nostr::nips::nip59::UnwrappedGift::from_gift_wrap(receiver, gift_wrap),
    )
    .map_err(Nip59Error::from)?;

    Ok(UnwrappedGift {
        sender: inner.sender,
        rumor: inner.rumor,
    })
}
