//! Non-blocking signing helpers — local-key and remote-signer paths.

use nmp_signer_iface::SignerOp;
use nostr::{EventBuilder, Kind, Tag, Timestamp};

use nmp_signer_iface::{SignedEvent, UnsignedEvent};

use super::runtime::IdentityRuntime;

/// Build a signed event over a fixed `Keys`. Mirrors the
/// `nmp-signers::LocalKeySigner::sign_now` recipe (same `nostr` primitives) —
/// kept here because D0 forbids importing `nmp-signers`. Two D6 correctness gates
/// (errors-as-state, never silent truncation), detailed at the call sites below:
/// out-of-`u16`-range kind, and any malformed tag — both hard-fail with a toast.
pub(crate) fn sign_with(
    keys: &nostr::Keys,
    unsigned: &UnsignedEvent,
) -> Result<SignedEvent, String> {
    // Finding 1: validate kind is within the Nostr-defined u16 range before
    // casting. kind:65559 → kind:23 would be a silent correctness violation.
    if unsigned.kind > u32::from(u16::MAX) {
        return Err(format!(
            "invalid kind {}: must be in range [0, 65535]",
            unsigned.kind
        ));
    }
    let kind = Kind::from_u16(unsigned.kind as u16);

    // Finding 2: hard-fail on any malformed tag rather than silently dropping
    // it. The caller is responsible for building well-formed tags; silent
    // drops would produce a signed event that differs from the caller's intent
    // (D6 — correctness hazard for kind-agnostic publish pass-through).
    let mut tags = Vec::with_capacity(unsigned.tags.len());
    let mut malformed = 0usize;
    for t in &unsigned.tags {
        match Tag::parse(t) {
            Ok(tag) => tags.push(tag),
            Err(_) => malformed += 1,
        }
    }
    if malformed > 0 {
        return Err(format!("Dropped {malformed} malformed tag(s)"));
    }

    let event = EventBuilder::new(kind, &unsigned.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(unsigned.created_at))
        .sign_with_keys(keys)
        .map_err(|e| format!("sign failed: {e}"))?;
    Ok(SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    })
}

/// Non-blocking sign with the active account. Local keys resolve inline;
/// remote signers return the port's `Pending` op for the caller to park.
pub(crate) fn sign_active_nonblocking(
    identity: &IdentityRuntime,
    unsigned: &UnsignedEvent,
) -> Result<SignerOp<SignedEvent>, String> {
    if let Some(handle) = identity.active_remote() {
        return Ok(handle.sign(unsigned));
    }
    let keys = identity
        .active_keys()
        .ok_or_else(|| "no active account — sign in first".to_string())?;
    match sign_with(keys, unsigned) {
        Ok(signed) => Ok(SignerOp::ok(signed)),
        Err(e) => Ok(SignerOp::err(nmp_signer_iface::SignerError::Backend(
            format!("local sign failed: {e}"),
        ))),
    }
}

/// Non-blocking sign with a specific registered pubkey, independent of which
/// account is active. This is the `signer_pubkey: Some(_)` publish path.
pub(crate) fn sign_with_account_nonblocking(
    identity: &IdentityRuntime,
    pubkey: &str,
    unsigned: &UnsignedEvent,
) -> Result<SignerOp<SignedEvent>, String> {
    if let Some(handle) = identity.remote_signers.get(pubkey) {
        return Ok(handle.sign(unsigned));
    }
    let keys = identity
        .keys
        .get(pubkey)
        .ok_or_else(|| format!("no signer for account {pubkey} — add it first"))?;
    match sign_with(keys, unsigned) {
        Ok(signed) => Ok(SignerOp::ok(signed)),
        Err(e) => Ok(SignerOp::err(nmp_signer_iface::SignerError::Backend(
            format!("local sign failed: {e}"),
        ))),
    }
}
