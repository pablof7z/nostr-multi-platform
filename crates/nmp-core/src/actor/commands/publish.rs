//! Publish handlers — generic unsigned events, kind:1 (note/reply), kind:7
//! (reaction), kind:3 (follow-edit), and timeline (re)open.
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

    // T144: a kind:1 reply needs full NIP-10 structure (root forwarding,
    // parent-author re-notification, dedup) not just a minimal reply marker.
    // We can't depend on `nmp-nip01` here (it depends on `nmp-core`, so the
    // edge would cycle), but we *can* use the same `crate::tags` primitives
    // its `Note::reply_to` builder is composed of — byte-identical output.
    //
    // See PD-024 (`docs/perf/pending-user-decisions.md`) for the rationale.
    let mut tags: Vec<Vec<String>> = Vec::new();
    let mut hydration_kick: Option<String> = None;
    if let Some(reply) = reply_to_id.filter(|r| crate::kernel::is_hex_id(r)) {
        match kernel.reply_tags_for_parent(reply) {
            Some(reply_tags) => tags = reply_tags,
            None => {
                // Cold reply — parent not in `kernel.events`. Emit a minimal
                // reply marker so the event is at least thread-discoverable,
                // and enqueue a one-shot hydration REQ (T121) so the next
                // reply on this id can be built with full NIP-10 structure
                // once the parent lands.
                tags.push(crate::tags::e_tag(reply, None, Some("reply")));
                hydration_kick = Some(reply.to_string());
            }
        }
    }

    let unsigned = UnsignedEvent {
        pubkey,
        kind: 1,
        tags,
        content: content.to_string(),
        created_at: now_secs(),
    };
    let mut outbound = match sign_active(identity, &unsigned) {
        Ok(signed) => kernel.publish_signed(&signed, &[]),
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            return Vec::new();
        }
    };

    if let Some(id) = hydration_kick {
        outbound.extend(kernel.kick_thread_hydration(id));
    }

    outbound
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
        Some(pk) => {
            // T140 Step A: register M2 follow-feed interests so drain_lifecycle_tick
            // emits REQ frames for the follow set on the next idle tick.
            // This complements ingest_contacts (which registers on kind:3 arrival);
            // open_timeline covers re-opens (screen re-entry) before a new kind:3
            // arrives.
            kernel.register_follow_feed_for_active_account();

            // M1 path: keep profile open (open_author) during the T140 transition
            // window. Step C will evaluate whether open_author is still needed
            // post-M2 or can be removed.
            kernel.open_author(pk, relays_ready)
        }
        None => toast_no_account(kernel, "open timeline"),
    }
}
