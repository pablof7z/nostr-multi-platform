//! Publish handlers — generic unsigned events, kind:1 (note/reply), kind:7
//! (reaction), kind:3 (follow-edit), and timeline (re)open.
//!
//! Every handler builds an `UnsignedEvent`, signs it with the active
//! account's key (D6: a missing active account is surfaced as a toast, never
//! an exception across FFI), then routes through `Kernel::publish_signed`
//! which resolves the NIP-65 outbox (D3) and emits the wire `EVENT` frame.

use crate::actor::commands::identity::{sign_active_nonblocking, IdentityRuntime};
use crate::actor::pending_sign::PendingSign;
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
/// (`nmp_nip23::Article`, `nmp_nip01::Note`, `nmp_relations::Reaction`, …),
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
    pending_signs: &mut Vec<PendingSign>,
) -> Vec<OutboundMessage> {
    if identity.active_pubkey().is_none() {
        return toast_no_account(kernel, "publish");
    }
    // Non-blocking sign: a local key resolves now; a remote (NIP-46) signer
    // returns a `Pending` op that is parked in `pending_signs` and `poll()`ed
    // by the actor's idle section — the actor thread never blocks (D8).
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            return Vec::new();
        }
    };
    match op.poll() {
        Some(Ok(signed)) => kernel.publish_signed(&signed, &[]),
        Some(Err(e)) => {
            kernel.set_last_error_toast(Some(format!("sign failed: {e}")));
            Vec::new()
        }
        None => {
            // Remote signer not yet responded — park the op for polling.
            pending_signs.push(PendingSign::new(op, Vec::new()));
            Vec::new()
        }
    }
}

/// Sign an unsigned event with the active account and publish it to an
/// EXPLICIT relay set, bypassing the NIP-65 outbox resolver.
///
/// This is the host-pinned twin of [`publish_unsigned_event`]: it shares the
/// "build → sign with the active account" half but replaces the routing half.
/// Where `publish_unsigned_event` routes through `Kernel::publish_signed`
/// (`PublishTarget::Auto`, the NIP-65 outbox), this routes through
/// `Kernel::publish_signed_to` with `PublishTarget::Explicit { relays }`.
///
/// The driving consumer is the NIP-29 group-action executor: a join request
/// (`kind:9021`) MUST land on the group's own host relay — the author's
/// kind:10002 outbox is the wrong target. The caller supplies that relay pin;
/// the kernel never inspects the event's `h` tag to derive it (routing.md §5
/// — typed pin, not tag-sniffing).
///
/// **Pubkey provenance.** Identical to `publish_unsigned_event`: the caller's
/// `unsigned.pubkey` is ignored; signing derives the pubkey from the active
/// identity and writes it onto the `SignedEvent`.
///
/// **Empty `relays`.** A defensive degrade: falls back to
/// `PublishTarget::Auto` so the publish is not silently dropped. Callers that
/// reach this path always supply the pin; an empty set is a caller bug, not a
/// supported mode.
///
/// **Remote (NIP-46) signers.** The explicit target is carried through the
/// remote-sign park via [`PendingSign::with_target`] — without it a bunker
/// user's group event would resolve through the NIP-65 outbox once the broker
/// responds, defeating the pin (D8: the actor still never blocks).
pub(crate) fn publish_unsigned_event_to_relays(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    unsigned: UnsignedEvent,
    relays: Vec<crate::publish::RelayUrl>,
    pending_signs: &mut Vec<PendingSign>,
) -> Vec<OutboundMessage> {
    if identity.active_pubkey().is_none() {
        return toast_no_account(kernel, "publish");
    }
    // Empty `relays` → Auto (NIP-65 outbox, defensive degrade); non-empty →
    // the named D3 Explicit opt-out routed to exactly those relays.
    let target = if relays.is_empty() {
        crate::publish::PublishTarget::Auto
    } else {
        crate::publish::PublishTarget::Explicit { relays }
    };
    // Non-blocking sign: a local key resolves now; a remote (NIP-46) signer
    // returns a `Pending` op parked in `pending_signs` with the explicit
    // target attached — the actor thread never blocks (D8).
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            return Vec::new();
        }
    };
    match op.poll() {
        Some(Ok(signed)) => kernel.publish_signed_to(&signed, &[], target),
        Some(Err(e)) => {
            kernel.set_last_error_toast(Some(format!("sign failed: {e}")));
            Vec::new()
        }
        None => {
            // Remote signer not yet responded — park the op WITH its target
            // so the pinned routing survives the broker round-trip.
            pending_signs.push(PendingSign::with_target(op, Vec::new(), target));
            Vec::new()
        }
    }
}

/// Generic, kind-agnostic publish of an **already-signed** event.
///
/// Sibling to [`publish_unsigned_event`], with one decisive difference: the
/// signer is **never** consulted. The caller supplies a fully-formed Nostr
/// event (`id`, `pubkey`, `created_at`, `kind`, `tags`, `content`, `sig`)
/// that was signed elsewhere — by an MDK/Marmot group-message signer, a
/// hardware signer, a relayed NIP-46 broker, anything. The kernel verifies
/// the Schnorr signature + event-id hash (forged/garbled events are rejected,
/// never published) and then routes the event verbatim through the **same**
/// publish planner / NIP-65 outbox resolver / relay-pin path the unsigned
/// command uses (D3). Only the signing step is skipped.
///
/// **Behavioral asymmetry vs. the unsigned sibling.** The unsigned path
/// requires an active account because it must sign. This path does **not** —
/// the signature already exists, and routing keys off the event's *own*
/// `pubkey` (its kind:10002 outbox), not the active account. Publishing a
/// signed event with no active account signed in is therefore valid and
/// supported. Marmot is the first consumer; the capability is generic (D0 —
/// no MLS/Marmot nouns in the kernel).
///
/// **Relay targeting.** `relays` selects the D3 routing mode:
/// - empty slice → `PublishTarget::Auto`: route via the author's NIP-65
///   kind:10002 outbox (the existing back-compat behavior — `kind:30443/443`
///   key-packages take this path).
/// - non-empty → `PublishTarget::Explicit { relays }`: the named D3 opt-out.
///   The verbatim signed event is dispatched to **exactly** these relays,
///   bypassing the outbox resolver. Marmot uses this for kind:445 group
///   messages (pinned GROUP relay) and kind:1059 gift-wraps (recipient inbox
///   relays) — relays the author's own kind:10002 does not cover.
///
/// D6 — a signature/id verification failure is surfaced as a toast (error
/// becomes kernel state, never a silent no-op) and produces no outbound
/// frames and no publish-queue entry. The forged event is dropped.
pub(crate) fn publish_signed_event(
    kernel: &mut Kernel,
    raw: crate::store::RawEvent,
    relays: &[crate::publish::RelayUrl],
) -> Vec<OutboundMessage> {
    // Reuse the store's verification gate: serializes to NIP-01 canonical
    // JSON, parses with the `nostr` crate, and checks BOTH the event-id hash
    // and the Schnorr signature. This is the exact primitive `kernel::ingest`
    // uses on inbound events, so a published signed event is held to the same
    // cryptographic bar as a received one.
    let verified = match crate::store::VerifiedEvent::try_from_raw(raw) {
        Ok(v) => v,
        Err(reason) => {
            // Typed FFI error contract: a verification failure (bad id hash
            // or Schnorr sig) means the caller handed us a structurally
            // malformed event — iOS branches on `malformed_event` rather
            // than substring-matching the English reason.
            kernel.set_error_toast_with_category(
                format!("signed event rejected: {reason}"),
                crate::kernel::closed_reason::ERR_MALFORMED_EVENT,
            );
            return Vec::new();
        }
    };
    let raw = verified.into_raw();
    // RawEvent (flat NIP-01) → SignedEvent (the kernel's publish-engine input).
    // No re-signing: `id` and `sig` are carried through verbatim — the wire
    // frame the engine builds (`build_event_frame`) reproduces these bytes
    // exactly.
    let signed = crate::substrate::SignedEvent {
        id: raw.id,
        sig: raw.sig,
        unsigned: UnsignedEvent {
            pubkey: raw.pubkey,
            kind: raw.kind,
            tags: raw.tags,
            content: raw.content,
            created_at: raw.created_at,
        },
    };
    // Empty `relays` → Auto (NIP-65 outbox, the back-compat path). Non-empty
    // → the named D3 Explicit opt-out, routed to exactly those relays.
    if relays.is_empty() {
        kernel.publish_signed(&signed, &[])
    } else {
        kernel.publish_signed_to(
            &signed,
            &[],
            crate::publish::PublishTarget::Explicit {
                relays: relays.to_vec(),
            },
        )
    }
}

/// Sign and publish a kind:1 note (optionally a NIP-10 reply).
///
/// `correlation_id` is the registry-minted action id when this publish
/// originates from `nmp_app_dispatch_action`'s `PublishAction::PublishNote`
/// path. The actor signs the event here, so its `id` is unknown to the host
/// at dispatch time; threading the minted id through makes the publish engine
/// report it in `last_action_result` (instead of the signed event's `id`) so
/// the host spinner keyed on the dispatch return value can be cleared. `None`
/// for non-dispatch callers (conformance harness, tests) — the engine then
/// reports the event id, the prior behaviour.
pub(crate) fn publish_note(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    content: &str,
    reply_to_id: Option<&str>,
    correlation_id: Option<String>,
    pending_signs: &mut Vec<PendingSign>,
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
    if let Some(reply) = reply_to_id {
        // D6: a malformed reply id is a user-visible error, not a silent
        // degrade. Without this guard the note would still publish — but as a
        // top-level note instead of a reply — losing the user's intent with no
        // feedback. Mirrors the explicit id/pubkey validation in `react` and
        // `follow`: refuse the publish and surface a toast.
        if !crate::kernel::is_hex_id(reply) {
            kernel.set_last_error_toast(Some("reply: malformed target event id".to_string()));
            return Vec::new();
        }
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
        created_at: kernel.now_secs(),
    };
    // Non-blocking sign: remote (NIP-46) signers return a `Pending` op that is
    // parked for the actor's idle-tick poll loop instead of blocking here.
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            return Vec::new();
        }
    };
    let mut outbound = match op.poll() {
        // Local key resolved on the spot — publish through the engine with the
        // dispatch correlation_id so the terminal verdict reports it.
        Some(Ok(signed)) => {
            kernel.publish_signed_with_correlation(&signed, &[], correlation_id)
        }
        Some(Err(e)) => {
            kernel.set_last_error_toast(Some(format!("sign failed: {e}")));
            return Vec::new();
        }
        None => {
            // Remote signer pending — park the op WITH its correlation_id so
            // the dispatched note still settles under the id the host is
            // waiting on once the broker turns the sign request around. The
            // hydration kick (independent of the reply event) still fires
            // below so the parent can be fetched.
            pending_signs.push(PendingSign::with_correlation_id(
                op,
                Vec::new(),
                correlation_id,
            ));
            Vec::new()
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
    pending_signs: &mut Vec<PendingSign>,
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
    // NIP-25 §1: a kind:7 reaction SHOULD carry both an `e` tag (the reacted-to
    // event) and a `p` tag (that event's author) so the author's relays route
    // the reaction to their notification inbox. Without the `p` tag the author
    // never learns the reaction happened.
    //
    // D6: the author pubkey is resolved from the kernel read-cache. If the
    // target event isn't cached (`None`) we still publish the reaction with
    // just the `e` tag — degraded but valid NIP-25 — rather than panicking or
    // refusing the publish.
    let mut tags = vec![vec!["e".to_string(), target_event_id.to_string()]];
    if let Some(author) = kernel.event_author(target_event_id) {
        tags.push(vec!["p".to_string(), author]);
    }
    let unsigned = UnsignedEvent {
        pubkey,
        kind: 7,
        tags,
        content,
        created_at: kernel.now_secs(),
    };
    // Non-blocking sign: a remote signer's `Pending` op is parked for the
    // actor's idle-tick poll loop rather than blocking the actor thread.
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            return Vec::new();
        }
    };
    match op.poll() {
        Some(Ok(signed)) => kernel.publish_signed(&signed, &[]),
        Some(Err(e)) => {
            kernel.set_last_error_toast(Some(format!("sign failed: {e}")));
            Vec::new()
        }
        None => {
            pending_signs.push(PendingSign::new(op, Vec::new()));
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
    pending_signs: &mut Vec<PendingSign>,
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
        created_at: kernel.now_secs(),
    };
    // Non-blocking sign: a remote signer's `Pending` op is parked for the
    // actor's idle-tick poll loop rather than blocking the actor thread.
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            return Vec::new();
        }
    };
    match op.poll() {
        Some(Ok(signed)) => kernel.publish_signed(&signed, &[]),
        Some(Err(e)) => {
            kernel.set_last_error_toast(Some(format!("sign failed: {e}")));
            Vec::new()
        }
        None => {
            pending_signs.push(PendingSign::new(op, Vec::new()));
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
