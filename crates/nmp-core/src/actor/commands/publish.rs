//! Publish handlers — generic unsigned events, kind:0 (profile), kind:3
//! (follow-edit), and timeline (re)open.
//!
//! Every handler builds an `UnsignedEvent`, signs it with the active
//! account's key (D6: a missing active account is surfaced as a toast, never
//! an exception across FFI), then routes through `Kernel::publish_signed`
//! which resolves the NIP-65 outbox (D3) and emits the wire `EVENT` frame.

use crate::actor::commands::identity::{
    sign_active_nonblocking, sign_with_account_nonblocking, IdentityRuntime,
};
use crate::actor::commands::publish_failures::{
    fail_invalid_target, fail_ownership, fail_publish, toast_no_account,
};
use crate::actor::commands::publish_finalize::finalize_before_sign;
use crate::actor::pending_sign::{ParkedOp, ParkedSignerOps};
use crate::kernel::Kernel;
use crate::publish::{validate_explicit_relays, PublishRouteClass, PublishTarget};
use crate::relay::OutboundMessage;
use nmp_ownership::EventOwnershipProvenance;
use nmp_signer_iface::UnsignedEvent;

/// Generic, kind-agnostic publish path.
///
/// Takes an `UnsignedEvent` already built by any protocol-crate builder
/// (`nmp_nip23::Article`, `nmp_nip01::Note`, `nmp_nip25::ReactAction`, …),
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
pub(crate) fn publish_unsigned_event(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    mut unsigned: UnsignedEvent,
    ownership: Option<EventOwnershipProvenance>,
    correlation_id: Option<String>,
    signer_pubkey: Option<String>,
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    if let Err(err) =
        nmp_ownership::validate_publish_ownership(unsigned.kind, &unsigned.tags, ownership, false)
    {
        return fail_ownership(kernel, err.to_string(), correlation_id);
    }
    // `signer_pubkey: Some(_)` publishes under a SPECIFIC (possibly non-active)
    // account — the active-account guard is skipped (a non-active signer
    // publish must succeed even with no active account). `None` keeps the
    // legacy active-account requirement.
    if signer_pubkey.is_none() && identity.active_pubkey().is_none() {
        // Broken-promise fix: a dispatched action handed the host a
        // `correlation_id`; `toast_no_account` records the matching
        // `Failed` terminal so the spinner clears, and is a no-op for `None`.
        return toast_no_account(kernel, "publish", correlation_id);
    }
    finalize_before_sign(kernel, &mut unsigned);
    // Non-blocking sign: a local key resolves now; a remote (NIP-46) signer
    // returns a `Pending` op that is parked in `parked_ops` and `poll()`ed
    // by the actor's idle section — the actor thread never blocks (D8).
    let sign_result = match &signer_pubkey {
        Some(pubkey) => sign_with_account_nonblocking(identity, pubkey, &unsigned),
        None => sign_active_nonblocking(identity, &unsigned),
    };
    let mut op = match sign_result {
        Ok(op) => op,
        Err(reason) => {
            // Broken-promise fix: a sign-setup failure happens on the actor
            // thread AFTER `dispatch_action` already returned the
            // correlation_id — `fail_publish` records the terminal failure.
            return fail_publish(kernel, reason, correlation_id);
        }
    };
    match op.poll() {
        // Local key resolved on the spot. When the publish was action-dispatched
        // (`correlation_id.is_some()`) the engine must report THAT id in
        // `action_results` — route through `publish_signed_with_correlation`.
        // Non-dispatch callers (`correlation_id == None` — `NmpApp::` Rust API,
        // tests) keep the prior `publish_signed` shape: the engine reports the
        // event id (== publish handle), which is the documented `None` fallback.
        // The two paths are run_publish_engine-equivalent (both `PublishTarget::Auto`,
        // identical p_tags); preserving the named entrypoints documents intent
        // and keeps `publish_signed` from drifting into dead-code in this lib.
        Some(Ok(signed)) => match correlation_id {
            Some(cid) => kernel.publish_signed_with_correlation(&signed, &[], Some(cid)),
            None => kernel.publish_signed(&signed, &[]),
        },
        Some(Err(e)) => {
            // Broken-promise fix: a local-key sign error happens after
            // `dispatch_action` returned the correlation_id — `fail_publish`
            // records the terminal failure under that id.
            fail_publish(kernel, format!("sign failed: {e}"), correlation_id)
        }
        None => {
            // Remote signer pending. Action-dispatched calls park WITH their
            // correlation_id so the broker turn-around settles under the id
            // the host is waiting on. The deadline is the SIGNING account's
            // per-op budget (ADR-0048 D3 — NIP-46 = 5s, NIP-55 = 90s).
            let deadline = identity.sign_deadline_for(signer_pubkey.as_deref());
            parked_ops.push(ParkedOp::publish(
                op,
                Vec::new(),
                PublishTarget::Auto,
                correlation_id,
                deadline,
            ));
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
/// `Kernel::publish_signed_to` with an explicit route.
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
/// **Empty / invalid `relays`.** Fail closed. Callers that want NIP-65 outbox
/// routing must use [`publish_unsigned_event`] / `ActorCommand::PublishUnsignedEvent`;
/// an empty explicit target is a caller bug, not a request to widen to `Auto`.
///
/// **Remote (NIP-46) signers.** The explicit target is carried through the
/// remote-sign park via [`ParkedOp::publish`] — without it a bunker
/// user's group event would resolve through the NIP-65 outbox once the broker
/// responds, defeating the pin (D8: the actor still never blocks).
pub(crate) fn publish_unsigned_event_to_relays(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    mut unsigned: UnsignedEvent,
    ownership: Option<EventOwnershipProvenance>,
    relays: Vec<crate::publish::RelayUrl>,
    route_class: crate::publish::PublishRouteClass,
    correlation_id: Option<String>,
    signer_pubkey: Option<String>,
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    let is_group_host_pin = route_class == PublishRouteClass::GroupHostPin;
    if let Err(err) = nmp_ownership::validate_publish_ownership(
        unsigned.kind,
        &unsigned.tags,
        ownership,
        is_group_host_pin,
    ) {
        return fail_ownership(kernel, err.to_string(), correlation_id);
    }
    // `signer_pubkey: Some(_)` publishes under a SPECIFIC (possibly non-active)
    // account — skip the active-account guard. `None` keeps the legacy
    // active-account requirement.
    if signer_pubkey.is_none() && identity.active_pubkey().is_none() {
        // Broken-promise fix: dispatched callers (NIP-29 group-message
        // executor — the only live consumer today) receive a correlation_id
        // from `nmp_app_dispatch_action`; without recording the terminal
        // failure here the host's spinner hangs forever. `toast_no_account`
        // is a no-op for `None` callers.
        return toast_no_account(kernel, "publish", correlation_id);
    }
    if let Err(reason) = validate_explicit_relays(&relays) {
        return fail_invalid_target(kernel, reason, correlation_id);
    }
    finalize_before_sign(kernel, &mut unsigned);
    let target = PublishTarget::explicit(relays, route_class);
    // Non-blocking sign: a local key resolves now; a remote (NIP-46) signer
    // returns a `Pending` op parked in `parked_ops` with the explicit
    // target + correlation_id attached — the actor thread never blocks (D8).
    let sign_result = match &signer_pubkey {
        Some(pubkey) => sign_with_account_nonblocking(identity, pubkey, &unsigned),
        None => sign_active_nonblocking(identity, &unsigned),
    };
    let mut op = match sign_result {
        Ok(op) => op,
        Err(reason) => {
            // Broken-promise fix: dispatched callers are waiting on
            // `action_results`; `fail_publish` records the terminal failure
            // under the correlation_id so the spinner clears.
            return fail_publish(kernel, reason, correlation_id);
        }
    };
    match op.poll() {
        Some(Ok(signed)) => {
            kernel.publish_signed_to_with_correlation(&signed, &[], target, correlation_id)
        }
        Some(Err(e)) => {
            // Broken-promise fix: a local-key sign error happens after
            // `dispatch_action` returned the correlation_id — `fail_publish`
            // records the terminal failure under that id.
            fail_publish(kernel, format!("sign failed: {e}"), correlation_id)
        }
        None => {
            // Remote signer not yet responded — park the op WITH its target
            // and correlation_id so pinned routing + spinner round-trip both
            // survive the broker round-trip. The deadline is the SIGNING
            // account's per-op budget (ADR-0048 D3).
            let deadline = identity.sign_deadline_for(signer_pubkey.as_deref());
            parked_ops.push(ParkedOp::publish(
                op,
                Vec::new(),
                target,
                correlation_id,
                deadline,
            ));
            Vec::new()
        }
    }
}

/// Generic, kind-agnostic publish of an **already-signed** event.
///
/// Sibling to [`publish_unsigned_event`], with one decisive difference: the
/// signer is **never** consulted. The caller supplies a fully-formed Nostr
/// event (`id`, `pubkey`, `created_at`, `kind`, `tags`, `content`, `sig`)
/// that was signed elsewhere — by an external group-message signer, a
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
/// supported. The capability is generic (D0 —
/// no app-layer nouns in the kernel).
///
/// **Relay targeting.** `target` preserves the caller's intent:
/// - `PublishTarget::Auto` routes via the author's NIP-65 kind:10002 outbox.
/// - `PublishTarget::Explicit` dispatches to exactly those relays, bypassing the
///   outbox resolver. Empty or malformed sets fail closed rather than degrading to Auto.
///
/// D6 — well-formedness verification (id-hash + Schnorr sig) runs through the
/// shared `Kernel::verify_externally_signed_event` chokepoint; a failure
/// surfaces a categorized toast, drops the forged event, and emits no frames.
///
/// `correlation_id` is an optional internal/protocol operation id (never the
/// event id — #1748); threading it makes the engine report THAT id in
/// `action_results`. `None` for callers that want the engine to fall back to the
/// publish handle (== event id).
///
/// **D10 defensive guard.** A kind:1059 gift-wrap with `PublishTarget::Auto` is
/// REFUSED — Auto would resolve through the author's public-relay outbox and
/// leak the encrypted envelope. Defense in depth at every entry into the
/// verified-publish path; callers of kind:1059 MUST supply a `VerifiedPrivateInbox` pin.
pub(crate) fn publish_signed_event(
    kernel: &mut Kernel,
    raw: crate::store::RawEvent,
    target: PublishTarget,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    // Delegates to the shared Kernel::publish_externally_signed helper
    // (#2045 PR-A): target-validate → verify-sig → D10 routing gate → publish.
    // Zero native behavior change — the full pipeline logic now lives in the
    // kernel method so the wasm/headless paths share it (forged-event fix).
    kernel.publish_externally_signed(raw, target, correlation_id)
}

/// Sign and publish a kind:0 profile metadata event for the active account.
///
/// `fields` is the flat string map the host supplied via
/// `PublishAction::PublishProfile`; this serializes it into the kind:0
/// `content`, stamps `created_at` from `kernel.now_secs()` (the host never
/// hand-rolls the timestamp — D7: the kernel owns the wall clock), signs with
/// the active account, and routes through the NIP-65 outbox (D3).
///
/// Sibling of the other action-dispatched publish helpers — same non-blocking
/// sign + `correlation_id` threading, kind:0 instead of kind:1.
/// `correlation_id` is the
/// registry-minted action id; threading it through makes the publish engine
/// report it in `action_results` so the host spinner keyed on the dispatch
/// return value can be cleared. `None` for non-dispatch callers.
pub(crate) fn publish_profile(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    fields: serde_json::Map<String, serde_json::Value>,
    correlation_id: Option<String>,
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    let Some(pubkey) = identity.active_pubkey() else {
        // Broken-promise fix: `toast_no_account` records `Failed` against the
        // dispatch correlation_id (no-op for `None` callers).
        return toast_no_account(kernel, "publish profile", correlation_id);
    };

    // kind:0 `content` is the JSON-serialized metadata object (NIP-01).
    // Preserve cached third-party fields so a profile edit from the one-door
    // action path does not turn the host into a second kind:0 writer.
    let fields = merged_profile_fields(kernel, &pubkey, fields);
    let content = match serde_json::to_string(&fields) {
        Ok(json) => json,
        Err(e) => {
            // Broken-promise fix: surface the rejection under the dispatch
            // correlation_id.
            return fail_publish(
                kernel,
                format!("profile serialisation: {e}"),
                correlation_id,
            );
        }
    };

    let mut unsigned = UnsignedEvent {
        pubkey,
        kind: 0,
        tags: Vec::new(),
        content,
        created_at: kernel.now_secs(),
    };
    finalize_before_sign(kernel, &mut unsigned);
    // Non-blocking sign: remote (NIP-46) signers return a `Pending` op parked
    // for the actor's idle-tick poll loop instead of blocking here.
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            // Broken-promise fix: report the failure under the dispatch
            // correlation_id so the host spinner clears.
            return fail_publish(kernel, reason, correlation_id);
        }
    };
    match op.poll() {
        // Local key resolved on the spot — publish through the engine with the
        // dispatch correlation_id so the terminal verdict reports it.
        Some(Ok(signed)) => kernel.publish_signed_with_correlation(&signed, &[], correlation_id),
        Some(Err(e)) => {
            // Broken-promise fix: a local-key sign error happens after
            // `dispatch_action` returned the correlation_id — record it.
            fail_publish(kernel, format!("sign failed: {e}"), correlation_id)
        }
        None => {
            // Remote signer pending — park the op WITH its correlation_id so
            // the dispatched profile still settles under the id the host is
            // waiting on once the broker turns the sign request around.
            parked_ops.push(ParkedOp::publish(
                op,
                Vec::new(),
                PublishTarget::Auto,
                correlation_id,
                identity.active_sign_deadline(),
            ));
            Vec::new()
        }
    }
}

fn merged_profile_fields(
    kernel: &Kernel,
    pubkey: &str,
    fields: serde_json::Map<String, serde_json::Value>,
) -> serde_json::Map<String, serde_json::Value> {
    let mut merged = kernel
        .profile_for_pubkey(pubkey)
        .map(|profile| profile.raw_fields)
        .unwrap_or_default();
    for (key, value) in fields {
        merged.insert(key, value);
    }
    merged
}
