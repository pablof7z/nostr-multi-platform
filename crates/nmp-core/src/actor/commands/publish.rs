//! Publish handlers — kind:1 (note/reply), kind:7 (reaction), kind:3
//! (follow-edit), and timeline (re)open.
//!
//! Every handler builds an `UnsignedEvent`, signs it with the active
//! account's key (D6: a missing active account is surfaced as a toast, never
//! an exception across FFI), then routes through `Kernel::publish_signed`
//! which resolves the NIP-65 outbox (D3) and emits the wire `EVENT` frame.

use crate::actor::commands::identity::{now_secs, sign_active, IdentityRuntime};
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::substrate::UnsignedEvent;

fn toast_no_account(kernel: &mut Kernel, action: &str) -> Vec<OutboundMessage> {
    kernel.set_last_error_toast(Some(format!(
        "cannot {action}: no active account — sign in first"
    )));
    Vec::new()
}

/// Generic, kind-agnostic publish path.
///
/// Takes an `UnsignedEvent` already built by any protocol-crate builder
/// (`nmp_nip23::Article`, `nmp_nip01::Note`, `nmp_reactions::Reaction`, …),
/// signs it with the active account's keys, and routes the signed event
/// through the existing NIP-65 outbox resolver (D3 automatic routing).
///
/// This is the **kernel-side dispatcher** for the per-NIP builders — it
/// doesn't know the kind, doesn't decode tags, doesn't construct any wire
/// shape. The kernel signs + publishes; the per-NIP crates own the wire
/// form. That keeps `nmp-core` D0-clean (no app nouns, no protocol decoders)
/// while unblocking every builder we've landed.
///
/// **Pubkey provenance.** The caller's `unsigned.pubkey` is **ignored** —
/// signing derives the pubkey from the active identity's keys and writes it
/// onto the returned `SignedEvent`. There is no path for an app to publish
/// under another author's identity through this command.
///
/// Stepping stone, not destination. The doctrine path is per-protocol-crate
/// `ActionModule` impls that own the full Build → Sign → Publish pipeline
/// (`kind-wrappers.md` §8 Phase 1). Once those land kind-by-kind, this
/// generic command deprecates gracefully — typed `AppAction::NmpNipNN(...)`
/// dispatches replace it.
pub(crate) fn publish_unsigned_event(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    unsigned: UnsignedEvent,
) -> Vec<OutboundMessage> {
    if identity.active_pubkey().is_none() {
        return toast_no_account(kernel, "publish");
    }
    match sign_active(identity, &unsigned) {
        Ok(signed) => kernel.publish_signed(&signed, &[]),
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            Vec::new()
        }
    }
}

pub(crate) fn publish_note(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    content: &str,
    reply_to_id: Option<&str>,
) -> Vec<OutboundMessage> {
    let Some(pubkey) = identity.active_pubkey() else {
        return toast_no_account(kernel, "publish");
    };
    let mut tags: Vec<Vec<String>> = Vec::new();
    if let Some(reply) = reply_to_id.filter(|r| crate::kernel::is_hex_id(r)) {
        // NIP-10 minimal reply marker.
        tags.push(vec![
            "e".to_string(),
            reply.to_string(),
            String::new(),
            "reply".to_string(),
        ]);
    }
    let unsigned = UnsignedEvent {
        pubkey,
        kind: 1,
        tags,
        content: content.to_string(),
        created_at: now_secs(),
    };
    match sign_active(identity, &unsigned) {
        Ok(signed) => kernel.publish_signed(&signed, &[]),
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            Vec::new()
        }
    }
}

pub(crate) fn react(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    target_event_id: &str,
    reaction: &str,
) -> Vec<OutboundMessage> {
    let Some(pubkey) = identity.active_pubkey() else {
        return toast_no_account(kernel, "react");
    };
    if !crate::kernel::is_hex_id(target_event_id) {
        kernel.set_last_error_toast(Some("react: malformed target event id".to_string()));
        return Vec::new();
    }
    let content = if reaction.trim().is_empty() {
        "+".to_string()
    } else {
        reaction.to_string()
    };
    let unsigned = UnsignedEvent {
        pubkey,
        kind: 7,
        tags: vec![vec!["e".to_string(), target_event_id.to_string()]],
        content,
        created_at: now_secs(),
    };
    match sign_active(identity, &unsigned) {
        Ok(signed) => kernel.publish_signed(&signed, &[]),
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            Vec::new()
        }
    }
}

/// Add (`add == true`) or remove a follow from the active account's kind:3
/// set and re-publish the full list (NIP-02 replaceable).
pub(crate) fn follow(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    pubkey: &str,
    add: bool,
) -> Vec<OutboundMessage> {
    let Some(author) = identity.active_pubkey() else {
        return toast_no_account(kernel, if add { "follow" } else { "unfollow" });
    };
    if !crate::kernel::is_hex_pubkey(pubkey) {
        kernel.set_last_error_toast(Some("follow: expected 64-hex pubkey".to_string()));
        return Vec::new();
    }
    let mut follows = kernel.current_follows(&author);
    if add {
        if !follows.iter().any(|p| p == pubkey) {
            follows.push(pubkey.to_string());
        }
    } else {
        follows.retain(|p| p != pubkey);
    }
    let tags = follows
        .iter()
        .map(|p| vec!["p".to_string(), p.clone()])
        .collect::<Vec<_>>();
    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags,
        content: String::new(),
        created_at: now_secs(),
    };
    match sign_active(identity, &unsigned) {
        Ok(signed) => kernel.publish_signed(&signed, &[]),
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            Vec::new()
        }
    }
}

pub(crate) fn open_timeline(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    match identity.active_pubkey() {
        Some(pk) => kernel.open_author(pk, relays_ready),
        None => toast_no_account(kernel, "open timeline"),
    }
}
