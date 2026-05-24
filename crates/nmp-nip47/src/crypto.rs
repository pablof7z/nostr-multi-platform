//! Wallet-side signing + event-shape helpers extracted from `runtime.rs` to
//! keep that file under the 500-LOC ceiling. Pure crypto / JSON shaping —
//! no kernel state.

use nmp_core::substrate::{SignedEvent, UnsignedEvent};
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};

/// Sign a kind:23194 NWC request event with the NWC client secret key.
///
/// `created_at_secs` is supplied by the caller (read from the kernel-owned
/// clock — `Kernel::now_secs()`). Keeping this helper free of a `Kernel`
/// dependency means the timestamp source stays `FixedClock`-testable.
pub(crate) fn sign_nwc_request(
    client_secret_hex: &str,
    wallet_pubkey_hex: &str,
    encrypted_content: &str,
    created_at_secs: u64,
) -> Result<SignedEvent, String> {
    let sk = SecretKey::from_hex(client_secret_hex).map_err(|e| format!("client secret: {e}"))?;
    let wallet_pk =
        PublicKey::from_hex(wallet_pubkey_hex).map_err(|e| format!("wallet pubkey: {e}"))?;
    let keys = Keys::new(sk);
    let p_tag = Tag::public_key(wallet_pk);
    let created_at = Timestamp::from(created_at_secs);
    let event = EventBuilder::new(Kind::from_u16(23194), encrypted_content)
        .tags([p_tag])
        .custom_created_at(created_at)
        .sign_with_keys(&keys)
        .map_err(|e| format!("sign: {e}"))?;
    Ok(SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: 23194u32,
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    })
}

/// Sign an arbitrary [`UnsignedEvent`] with a fixed `Keys` — mirrors
/// `nmp-core::actor::commands::identity::sign_with`. Used to build the
/// wallet-lane NIP-42 [`AuthSignerFn`](nmp_core::AuthSignerFn) closure.
pub(crate) fn sign_with(
    keys: &Keys,
    unsigned: &UnsignedEvent,
) -> Result<SignedEvent, String> {
    // NWC + NIP-42 frames are all <= u16; truncate explicitly so a future
    // u32-only kind never silently signs with a clipped value.
    let kind_u16 = u16::try_from(unsigned.kind).map_err(|_| {
        format!("sign_with: kind {} exceeds u16 range", unsigned.kind)
    })?;
    let kind = Kind::from_u16(kind_u16);
    let mut builder = EventBuilder::new(kind, &unsigned.content)
        .custom_created_at(Timestamp::from(unsigned.created_at));
    for tag_vec in &unsigned.tags {
        if let Ok(tag) = Tag::parse(tag_vec.clone()) {
            builder = builder.tag(tag);
        }
    }
    let event = builder
        .sign_with_keys(keys)
        .map_err(|e| format!("sign: {e}"))?;
    Ok(SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: unsigned.kind,
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    })
}

/// Serialize a [`SignedEvent`] into the NIP-01 EVENT JSON object.
pub(crate) fn build_event_json(signed: &SignedEvent) -> serde_json::Value {
    serde_json::json!({
        "id": signed.id,
        "pubkey": signed.unsigned.pubkey,
        "created_at": signed.unsigned.created_at,
        "kind": signed.unsigned.kind,
        "tags": signed.unsigned.tags,
        "content": signed.unsigned.content,
        "sig": signed.sig,
    })
}
