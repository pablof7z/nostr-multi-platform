//! Publish handlers — generic unsigned events, kind:0 (profile), kind:7
//! (reaction), kind:3 (follow-edit), and timeline (re)open.
//!
//! Every handler builds an `UnsignedEvent`, signs it with the active
//! account's key (D6: a missing active account is surfaced as a toast, never
//! an exception across FFI), then routes through `Kernel::publish_signed`
//! which resolves the NIP-65 outbox (D3) and emits the wire `EVENT` frame.

use crate::actor::commands::identity::{
    sign_active_nonblocking, sign_with_account_nonblocking, IdentityRuntime,
};
use crate::actor::pending_sign::ParkedOp;
use crate::kernel::Kernel;
use crate::kinds::KIND_GIFT_WRAP;
use crate::publish::{validate_explicit_relays, validate_publish_target, PublishTarget};
use crate::relay::OutboundMessage;
use crate::substrate::UnsignedEvent;

// V-57 P2 (2026-05-27) — the gift-wrap kind constant lives in the
// workspace-canonical [`crate::kinds`] registry. The D10 guard predicate
// below still keys off the integer; centralising it removes the
// duplication between this file and `nmp-nip59::kinds::KIND_GIFT_WRAP`
// (and the other private duplicates in `nmp-nip17` / `nmp-marmot`) so the
// wire integer is declared once across the workspace.

/// Set a "no active account" toast and — when a dispatched action is waiting
/// on a `correlation_id` — record the matching `Failed` terminal so the host
/// spinner clears.
///
/// Every publish handler in this module guards on `identity.active_pubkey()`
/// and exits early when no account is signed in. Threading the `correlation_id`
/// through that exit is the broken-promise fix the per-handler arms already
/// honour ad-hoc; centralising it here keeps the pattern uniform and removes
/// the risk of a new handler forgetting the second leg.
///
/// The `action_failure` reason is the bare `"no active account"` string the
/// per-handler sites used historically — matching across handlers so the host
/// can pattern-match consistently regardless of which verb dispatched.
fn toast_no_account(
    kernel: &mut Kernel,
    action: &str,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    kernel.set_last_error_toast(Some(format!(
        "cannot {action}: no active account — sign in first"
    )));
    if let Some(id) = correlation_id {
        kernel.record_action_failure(id, "no active account".to_string());
    }
    Vec::new()
}

/// Set `reason` as the last-error toast and — when a dispatched action is
/// waiting on a `correlation_id` — record the matching `Failed` terminal so
/// the host spinner clears. Returns an empty outbound vec so call sites stay
/// `return fail_publish(...);` one-liners.
///
/// This is the generic twin of [`fail_invalid_target`] — same dual-write
/// contract, but the toast text is supplied verbatim by the caller rather
/// than templated with the `"explicit publish target rejected:"` prefix.
/// Used by sign-setup and sign-error branches across every publish handler;
/// previously these were ~3-line `set_last_error_toast` + `if let Some(id)`
/// copy-pastes (with one branch in `publish_unsigned_event_to_relays`
/// silently DROPPING the `correlation_id`, which orphaned the host spinner on
/// a dispatched NIP-29 group-message sign failure — fixed by this consolidation).
fn fail_publish(
    kernel: &mut Kernel,
    reason: String,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    kernel.set_last_error_toast(Some(reason.clone()));
    if let Some(id) = correlation_id {
        kernel.record_action_failure(id, reason);
    }
    Vec::new()
}

fn fail_invalid_target(
    kernel: &mut Kernel,
    reason: String,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    let toast = format!("explicit publish target rejected: {reason}");
    kernel.set_last_error_toast(Some(toast.clone()));
    if let Some(id) = correlation_id {
        kernel.record_action_failure(id, toast);
    }
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
    correlation_id: Option<String>,
    signer_pubkey: Option<String>,
    parked_ops: &mut Vec<ParkedOp>,
) -> Vec<OutboundMessage> {
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
    unsigned: UnsignedEvent,
    relays: Vec<crate::publish::RelayUrl>,
    correlation_id: Option<String>,
    signer_pubkey: Option<String>,
    parked_ops: &mut Vec<ParkedOp>,
) -> Vec<OutboundMessage> {
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
    let target = PublishTarget::Explicit { relays };
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
/// - `PublishTarget::Explicit { relays }` dispatches to exactly those relays,
///   bypassing the outbox resolver. Empty or malformed explicit relay sets
///   fail closed rather than degrading to Auto.
///
/// D6 — a signature/id verification failure is surfaced as a toast (error
/// becomes kernel state, never a silent no-op) and produces no outbound
/// frames and no publish-queue entry. The forged event is dropped.
///
/// `correlation_id` is the registry-minted action id when this publish
/// originates from `nmp_app_dispatch_action`'s pre-signed `PublishAction::Publish`
/// path. Threading it makes the publish engine report THAT id in
/// `action_results` via `correlation_id_override` — explicit symmetry with
/// `publish_profile`. `None` for non-dispatch callers (the kernel-internal
/// `NmpApp::publish_signed_explicit` workspace-internal seam + conformance
/// harnesses land on this `None` path; the deleted
/// `nmp_app_publish_signed_event*` C-ABI symbols used to land here too,
/// always with `None`); the engine then falls back to the publish handle
/// (== event id), preserving the prior behaviour.
///
/// **D10 defensive guard.** A kind:1059 gift-wrap envelope with
/// `PublishTarget::Auto` is REFUSED — the Auto branch below would
/// otherwise resolve through the author's public-relay outbox and leak
/// the encrypted envelope. The refusal sets a D6 toast and emits a
/// `tracing::warn!`. This is the kernel-level twin of the per-protocol
/// call-site guards — defense in depth at every entry into the
/// verified-publish path. Callers of kind:1059 MUST supply an explicit
/// relay pin (`PublishTarget::Explicit { relays }`).
pub(crate) fn publish_signed_event(
    kernel: &mut Kernel,
    raw: crate::store::RawEvent,
    target: PublishTarget,
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    if let Err(reason) = validate_publish_target(&target) {
        return fail_invalid_target(kernel, reason, correlation_id);
    }
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
            // than substring-matching the English reason. The categorized
            // toast surface is deliberately preserved here (NOT
            // `fail_publish`'s uncategorized path), because the FFI error
            // contract pins the `ERR_MALFORMED_EVENT` discriminant.
            let toast = format!("signed event rejected: {reason}");
            kernel.set_error_toast_with_category(
                toast.clone(),
                crate::kernel::closed_reason::ERR_MALFORMED_EVENT,
            );
            // Broken-promise fix: dispatched callers (the generic
            // `dispatch_action("nmp.publish")` → `PublishAction::Publish`
            // path) carry a correlation_id; record the terminal failure
            // under it so the host's spinner clears. No-op for `None`.
            if let Some(id) = correlation_id {
                kernel.record_action_failure(id, toast);
            }
            return Vec::new();
        }
    };
    let raw = verified.into_raw();
    // ── D10 defensive guard ─────────────────────────────────────────────────
    //
    // Belt-and-suspenders for kind:1059 gift-wraps: refuse to publish the
    // envelope when the caller did not supply an explicit relay pin. The
    // per-protocol call-site guards close their own send paths; this is the
    // kernel-level twin that closes EVERY path that reaches
    // `publish_signed_event`. In particular:
    //
    //   1. The generic `dispatch_action("nmp.publish")` → `PublishAction::Publish`
    //      arm: a `PublishTarget::Auto` carries no relays and is routed
    //      verbatim into `ActorCommand::PublishSignedEvent { target: Auto }`,
    //      which lands here. Without this guard a host that dispatches a
    //      kind:1059 envelope with `target: Auto` would silently leak through
    //      the Auto branch below.
    //
    //   2. Workspace-internal seams that always build
    //      `PublishTarget::Explicit { relays }` (with `validate_publish_target`
    //      rejecting an empty `Explicit` relay set at the top of this
    //      function) do not hit this Auto leg today — the guard is the
    //      defence in depth that keeps the invariant when a future caller is
    //      added.
    //
    // Structural invariant: kind:1059 + `Auto` is NEVER routed to the
    // author's public-relay outbox. The refusal sets a D6 toast, drops the
    // event before any outbound frames or publish-queue entries are
    // produced, and emits a `tracing::warn!` so the leak attempt is visible
    // in logs. This is policy, not malformed data — `set_last_error_toast`
    // (the legacy uncategorized path) is the right surface (a routing-leak
    // policy refusal is not in the closed `error_category` key set defined
    // by `kernel::closed_reason`).
    if raw.kind == KIND_GIFT_WRAP && matches!(target, PublishTarget::Auto) {
        let reason = "cannot publish kind:1059 gift-wrap: no explicit relay pin \
             (D10 would leak the encrypted envelope to the author's public relays)"
            .to_string();
        tracing::warn!(
            kind = raw.kind,
            "publish_signed_event refused: kind:1059 envelope with PublishTarget::Auto \
             would route through the author's public-relay outbox, leaking the \
             existence of the encrypted gift-wrap (D10 violation). Caller must \
             supply an explicit relay pin.",
        );
        kernel.set_last_error_toast(Some(reason.clone()));
        // Broken-promise fix: if this publish came in via `dispatch_action`'s
        // `PublishAction::Publish` path, the host received a `correlation_id`
        // and the dispatch arm already recorded `ActionStage::Requested`. The
        // refusal here must reach `action_results` under that id so the
        // host's spinner clears with a terminal failure verdict — the same
        // pattern the per-verb publishers (`publish_profile`) apply on their
        // sign-step early-exits. No-op for `None`
        // (non-dispatch callers — `NmpApp::publish_signed_explicit`,
        // conformance harnesses — have nothing waiting on an id).
        if let Some(id) = correlation_id {
            kernel.record_action_failure(id, reason);
        }
        return Vec::new();
    }
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
    // `correlation_id` threads through to the publish engine's
    // `correlation_id_override` — `None` preserves the prior fallback to the
    // publish handle (== event id) for every non-dispatch caller.
    kernel.publish_signed_to_with_correlation(&signed, &[], target, correlation_id)
}

/// Sign and publish a kind:0 profile metadata event for the active account.
///
/// `fields` is the flat string map the host supplied via
/// `PublishAction::PublishProfile`; this serializes it into the kind:0
/// `content`, stamps `created_at` from `kernel.now_secs()` (the host never
/// hand-rolls the timestamp — D7: the kernel owns the wall clock), signs with
/// the active account, and routes through the NIP-65 outbox (D3).
///
/// Sibling of [`react`] — same non-blocking sign + `correlation_id`
/// threading, kind:0 instead of kind:1. `correlation_id` is the
/// registry-minted action id; threading it through makes the publish engine
/// report it in `action_results` so the host spinner keyed on the dispatch
/// return value can be cleared. `None` for non-dispatch callers.
pub(crate) fn publish_profile(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    fields: serde_json::Map<String, serde_json::Value>,
    correlation_id: Option<String>,
    parked_ops: &mut Vec<ParkedOp>,
) -> Vec<OutboundMessage> {
    let Some(pubkey) = identity.active_pubkey() else {
        // Broken-promise fix: `toast_no_account` records `Failed` against the
        // dispatch correlation_id (no-op for `None` callers).
        return toast_no_account(kernel, "publish profile", correlation_id);
    };

    // kind:0 `content` is the JSON-serialized metadata object (NIP-01).
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

    let unsigned = UnsignedEvent {
        pubkey,
        kind: 0,
        tags: Vec::new(),
        content,
        created_at: kernel.now_secs(),
    };
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

pub(crate) fn react(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    target_event_id: &str,
    reaction: &str,
    correlation_id: Option<String>,
    parked_ops: &mut Vec<ParkedOp>,
) -> Vec<OutboundMessage> {
    let Some(pubkey) = identity.active_pubkey() else {
        // Broken-promise fix: `toast_no_account` records `Failed` against the
        // dispatch correlation_id (no-op for `None` callers).
        return toast_no_account(kernel, "react", correlation_id);
    };
    // Build NIP-25 tags + normalised content via the shared builder.
    // `reaction_tags` returns None for invalid hex — surface as fail_publish
    // so the broken-promise fix records the terminal under the dispatch id.
    let author = kernel.event_author(target_event_id);
    let (tags, content) =
        match crate::tags::reaction_tags(target_event_id, author.as_deref(), reaction) {
            Some(t) => t,
            None => {
                return fail_publish(
                    kernel,
                    "react: malformed target event id".to_string(),
                    correlation_id,
                );
            }
        };
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
            // Broken-promise fix: a sign-setup failure happens on the actor
            // thread AFTER `dispatch_action` already returned the
            // correlation_id — `fail_publish` records the terminal failure.
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
            // the dispatched reaction still settles under the id the host is
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

/// Add (`add == true`) or remove a follow from the active account's kind:3
/// set and re-publish the full list (NIP-02 replaceable).
pub(crate) fn follow(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    pubkey: &str,
    add: bool,
    correlation_id: Option<String>,
    parked_ops: &mut Vec<ParkedOp>,
) -> Vec<OutboundMessage> {
    let Some(author) = identity.active_pubkey() else {
        // Broken-promise fix: `toast_no_account` records `Failed` against the
        // dispatch correlation_id (no-op for `None` callers).
        return toast_no_account(
            kernel,
            if add { "follow" } else { "unfollow" },
            correlation_id,
        );
    };
    if !crate::kernel::is_hex_pubkey(pubkey) {
        // Broken-promise fix: surface the rejection under the dispatch
        // correlation_id so the host spinner does not hang.
        return fail_publish(
            kernel,
            "follow: expected 64-hex pubkey".to_string(),
            correlation_id,
        );
    }
    // Fail-closed gate (issue #1246b): read the active account's FULL existing
    // kind:3 raw event — every tag verbatim plus its content. `None` means the
    // kind:3 has not been ingested yet; publishing an edit built from an empty
    // list would silently wipe the user's contacts, so surface the failure
    // under the dispatch correlation_id (matching the wasm path's
    // `follow_list_not_loaded` CapabilityFailure after PR #1244). This replaces
    // the old gate-less `current_follows`, which returned `[]` on a not-loaded
    // list.
    let Some((current_tags, current_content)) = kernel.try_current_kind3_event() else {
        return fail_publish(
            kernel,
            "follow_list_not_loaded".to_string(),
            correlation_id,
        );
    };
    // Splice ONLY the `p` section — preserve relay hints, petnames, every
    // non-`p` tag, and the original content verbatim (issue #1246a).
    let tags = if add {
        crate::tags::kind3_tags_after_add(&current_tags, pubkey)
    } else {
        crate::tags::kind3_tags_after_remove(&current_tags, pubkey)
    };
    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags,
        content: current_content,
        created_at: kernel.now_secs(),
    };
    // Non-blocking sign: a remote signer's `Pending` op is parked for the
    // actor's idle-tick poll loop rather than blocking the actor thread.
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => {
            // Broken-promise fix: a sign-setup failure happens on the actor
            // thread AFTER `dispatch_action` already returned the
            // correlation_id — record it.
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
            // the dispatched follow/unfollow still settles under the id the
            // host is waiting on once the broker turns the sign request around.
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

/// (Re)open the contact-feed subscription for the active account.
///
/// `kinds` is the host-declared event-kind set the follow-set REQ should
/// carry. D0: `nmp-core` does not know which kinds belong to the host's app
/// concept (Chirp's home feed declares {1, 6}; a long-form app might declare
/// {30023}); the host supplies the set so the substrate carries no
/// app-specific social knowledge. `Kernel::set_follow_feed_kinds` stores the
/// set and re-registers the active account's M2 follow-feed interests under it,
/// so `drain_lifecycle_tick` emits REQ frames for the follow set on the next
/// idle tick. This complements `ingest_contacts` (which registers on kind:3
/// arrival, reading the same stored kinds); this command covers re-opens
/// (screen re-entry) before a new kind:3 arrives, and is also where the host
/// first activates the subscription by declaring its kinds. An empty `kinds`
/// set clears the subscription (equivalent to `close_contact_feed`).
pub(crate) fn open_contact_feed(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    kinds: std::collections::BTreeSet<u32>,
) -> Vec<OutboundMessage> {
    match identity.active_pubkey() {
        Some(_pk) => {
            // Store the host-declared kinds and re-register M2 follow-feed
            // interests so drain_lifecycle_tick emits REQ frames for the follow
            // set on the next idle tick. `set_follow_feed_kinds` already calls
            // `register_follow_feed_for_active_account` internally. An empty
            // set clears all follow-feed interests (D5 CLEAR branch).
            kernel.set_follow_feed_kinds(kinds);
            Vec::new()
        }
        None => toast_no_account(kernel, "open_contact_feed", None),
    }
}

/// Tear down the contact-feed subscription opened by `open_contact_feed`.
///
/// Passes an empty kinds set to the kernel so it withdraws all follow-feed
/// M2 interests from the lifecycle registry. The `FollowListChanged` trigger
/// propagates and `drain_lifecycle_tick` emits CLOSE frames for any live REQs.
///
/// D6: no active account (or no prior open) is a silent no-op — the kernel's
/// `follow_feed_kinds` is already empty for a not-signed-in session.
pub(crate) fn close_contact_feed(
    _identity: &IdentityRuntime,
    kernel: &mut Kernel,
) -> Vec<OutboundMessage> {
    kernel.set_follow_feed_kinds(std::collections::BTreeSet::new());
    Vec::new()
}
