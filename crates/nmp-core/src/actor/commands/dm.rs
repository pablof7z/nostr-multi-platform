//! NIP-17 gift-wrapped DM send handler.
//!
//! # D13: signer-only seal path
//!
//! This file is in the D13 Part-A default scope (see
//! `crates/nmp-testing/bin/doctrine-lint/rules/d13.rs`). The DM seal path
//! MUST NOT dereference raw key material directly — every gift-wrap call
//! routes through `nmp_nip59::gift_wrap_with_signer` over the
//! `SignerForSeal` abstraction. The marker comment above keeps the rule
//! attached even if this file moves.
//!
//! # Account-type branching
//!
//! Sealing a NIP-59 rumor needs a NIP-44 encrypt (kind:13 seal) followed by
//! a signature, both keyed by the sender's account. ADR-0026 unifies the
//! local-keys and remote-signer flavours through the `SignerForSeal` trait
//! (`nmp_nip59::SignerForSeal`).
//!
//! The handler resolves a `SignerForSeal` for the active account via
//! `IdentityRuntime::active_signer_for_seal` and hands it to
//! `nmp_nip59::gift_wrap_with_signer`. The accessor returns:
//! - `Some(Arc<dyn SignerForSeal>)` for a local account — the blanket impl
//!   on `nostr::Keys` resolves both the encrypt and sign steps
//!   synchronously, so `gift_wrap_with_signer` runs the whole chain on the
//!   actor thread and returns the kind:1059 envelope immediately.
//! - `None` for a remote account — `gift_wrap_with_signer` itself can drive
//!   a remote chain (it spawns a per-invocation driver thread that owns the
//!   multi-step `SignerOp::Pending` round-trips), but wiring the remote
//!   `RemoteSignerHandle → SignerForSeal` adapter (with the substrate
//!   `SignedEvent ↔ nostr::Event` bridge) is the Phase 2 follow-up to
//!   ADR-0026. Until then `None` is the graceful-degrade signal: surface
//!   a toast (D6), publish nothing, never panic.
//!
//! It deliberately does NOT read the `NmpApp::marmot_local_nsec` FFI field
//! to bypass the actor: that slot is the ADR-0025 Marmot exception and must
//! not be read for NIP-17 (D13 Part A makes this structural).
//!
//! `ActorCommand::SendGiftWrappedDm` arrives carrying an **unsigned**
//! kind:14 chat-message rumor (built host-side by
//! `nmp_nip17::build_dm_rumor`). This handler:
//!
//! 1. Resolves the active account's `SignerForSeal` (see above).
//! 2. Re-stamps `rumor.created_at` from `kernel.now_secs()` (D7 — the kernel
//!    owns the wall clock; the host sends `0` as a sentinel).
//! 3. Gift-wraps the rumor TWICE via `nmp_nip59::gift_wrap_with_signer`:
//!    once to the recipient, once to the sender's own pubkey (the
//!    self-copy, so sent messages stay readable). Each call mints a fresh
//!    ephemeral key for the outer kind:1059 envelope — the unlinkability
//!    guarantee.
//! 4. Publishes each kind:1059 envelope to its receiver's kind:10050 DM-inbox
//!    relays via the explicit-target publish path. The envelopes are already
//!    signed (by their ephemeral keys); they MUST NOT be re-signed with the
//!    account key, which would destroy unlinkability — so they route through
//!    `publish_signed_event`, not the unsigned publish path.

// D13: signer-only seal path
//!
//! # Relay routing — NIP-17 § 2
//!
//! NIP-17 requires each kind:1059 envelope to be published to the **receiver's**
//! kind:10050 DM-relay list — the recipient envelope to the recipient's list,
//! the self-copy envelope to the *sender's* own list (so the sender's other
//! clients can read sent messages). Routing both envelopes to the sender's
//! Content relays — as an earlier draft did — silently loses the message when
//! the recipient reads a different relay set: the send "succeeds" with no toast
//! but nothing is delivered.
//!
//! `Kernel::recipient_dm_relays` reads a **live** kind:10050 cache
//! (`dm_relay_lists`, populated by `ingest_dm_relay_list`). It returns
//! `Some(relays)` for any receiver whose kind:10050 DM-relay list has been
//! ingested, and `None` for a receiver who has never published one. The `None`
//! branch here is the correct fallback for that genuinely-missing case: route
//! to the actor's configured Content relays AND emit a `tracing::warn!` so the
//! routing gap is visible in logs. The warn is no longer a stub-marker for an
//! unbuilt feature — it is the documented diagnostic for a recipient without a
//! kind:10050 list.
//!
//! ## D10 empty-relay guard — kind:1059 NEVER Auto-routes
//!
//! `bootstrap_urls_for_role(Content)` may return empty in production builds
//! (the operator has not configured any Content relays yet). Without a guard,
//! `publish_signed_event` then maps the empty slice → `PublishTarget::Auto`,
//! which leaks the encrypted envelope to the author's NIP-65 outbox — the
//! exact D10 violation gift-wrap exists to prevent. The runtime guard in
//! [`is_empty_relays_kind1059_block`] refuses any kind:1059 publish whose
//! resolved relay list is empty (no kind:10050 cache AND no configured
//! Content fallback). The refusal surfaces as a D6 toast and skips the
//! `publish_signed_event` call entirely — the in-memory event is dropped.
//!
//! This is the call-site (NIP-17) twin of the
//! `nmp-marmot::projection::publish::publish_to` Marmot guard PR-K added at
//! the Marmot bridge — defence in depth at every kind:1059 publish surface.

use nostr::{nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK, EventBuilder, Kind, PublicKey, Tag, Timestamp};

use crate::actor::commands::identity::IdentityRuntime;
use crate::kernel::Kernel;
use crate::relay::OutboundMessage;
use crate::store::RawEvent;
use crate::substrate::UnsignedEvent;

/// Seal + gift-wrap a NIP-17 kind:14 rumor and publish the kind:1059 envelopes.
///
/// Returns the outbound wire frames for both envelopes (recipient + self-copy),
/// or an empty vec when the send could not proceed — in which case a toast has
/// been set on the kernel (D6: the error is observable state, never silent).
pub(crate) fn send_gift_wrapped_dm(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    mut rumor: UnsignedEvent,
    recipient_pubkey: &str,
) -> Vec<OutboundMessage> {
    // D10: private-kind publish
    // Every publish call below MUST route via an Explicit pin (the
    // recipient's kind:10050 DM-inbox relays or, on a cache miss, the
    // configured Content relays). The doctrine-lint D10 rule enforces this
    // structurally: any Auto-routing seam (`PublishTarget::Auto`,
    // `publish_signed(...)`, `publish_unsigned_event(...)`) below this
    // marker fires a lint finding.
    //
    // 1. Resolve a `SignerForSeal` for the active account (ADR-0026 seam).
    //    `None` here means the active signer is a remote (NIP-46 / NIP-07)
    //    handle and the Phase-2 adapter is not wired yet; surface a toast
    //    and publish nothing (D6 — graceful degrade, never a panic).
    let Some(signer) = identity.active_signer_for_seal() else {
        let reason = match identity.active_signer_kind() {
            Some("local") => {
                // Unreachable in practice — a local signer always produces
                // Some — but report it as a D6 error rather than a panic
                // if the invariant ever breaks.
                "internal error: local account did not produce a SignerForSeal"
                    .to_string()
            }
            Some(other) => format!(
                "cannot send DM: remote signer ({}) not yet supported for NIP-17 \
                 (ADR-0026 Phase 2)",
                other
            ),
            None => "cannot send DM: no active account".to_string(),
        };
        kernel.set_last_error_toast(Some(reason));
        return Vec::new();
    };

    // 2. D7: re-stamp the rumor timestamp from the kernel clock. The host sends
    //    `created_at: 0` as the sentinel; the kernel owns the wall clock.
    if rumor.created_at == 0 {
        rumor.created_at = kernel.now_secs();
    }

    // 3. The signer carries the sender's pubkey; centralising the access
    //    here keeps dm.rs D13-clean (no `active_local_keys` / `.secret_key()`
    //    calls on the seal path).
    let sender = signer.pubkey();

    // 4. Convert the substrate rumor → `nostr::UnsignedEvent`. The rumor is
    //    NEVER signed; `EventBuilder::build` produces the unsigned form that
    //    `gift_wrap_with_signer` seals.
    let nostr_rumor = match build_nostr_rumor(&rumor, sender) {
        Ok(r) => r,
        Err(reason) => {
            kernel.set_last_error_toast(Some(format!("cannot send DM: {reason}")));
            return Vec::new();
        }
    };

    // Recipient pubkey must parse — a malformed hex pubkey is a caller bug;
    // refuse the send rather than wrap to a garbage key (D6).
    let recipient = match PublicKey::parse(recipient_pubkey) {
        Ok(pk) => pk,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!(
                "cannot send DM: malformed recipient pubkey: {e}"
            )));
            return Vec::new();
        }
    };

    // 5. Gift-wrap TWICE — fresh ephemeral outer key per call (NIP-59).
    //    Envelope A: wrapped to the recipient.
    //    Envelope B: the self-copy, wrapped to the sender's own pubkey.
    //
    //    Each envelope is routed to *its receiver's* kind:10050 DM-inbox
    //    relays (NIP-17 § 2): the recipient envelope to the recipient's list,
    //    the self-copy to the sender's own list. The receiver's pubkey hex is
    //    carried alongside so `recipient_dm_relays` can be keyed correctly.
    //
    //    Each call to `gift_wrap_with_signer` runs the seal+wrap chain for
    //    one envelope. On the LOCAL fast path (every `SignerOp` from a
    //    `Keys`-backed `SignerForSeal` is `Ready`), the chain completes on
    //    the actor thread and `wait()` resolves synchronously. The Phase-2
    //    REMOTE path returns `SignerOp::Pending` — wiring that into the
    //    actor's existing `pending_signs` queue (so the actor never blocks
    //    on a bunker round-trip) is the next follow-up; right now `None`
    //    above short-circuits the remote case to a toast.
    let sender_hex = sender.to_hex();
    let mut outbound = Vec::new();
    let mut empty_relay_skips: Vec<&'static str> = Vec::new();
    for (label, receiver, receiver_hex) in [
        ("recipient", &recipient, recipient_pubkey),
        ("self-copy", &sender, sender_hex.as_str()),
    ] {
        // NIP-59 randomises the kind:13 + kind:1059 timestamps in a 2-day
        // window so an observer cannot correlate the envelope timestamp
        // with the underlying rumor; mirrors the behaviour of
        // `nostr::nips::nip59::make_seal`.
        let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
        let op = nmp_nip59::gift_wrap_with_signer(
            std::sync::Arc::clone(&signer),
            receiver,
            nostr_rumor.clone(),
            tweaked,
        );
        // LOCAL fast path: `wait(0)` resolves a `Ready` op immediately.
        // For the REMOTE path this `wait` would block the actor thread —
        // but `active_signer_for_seal` returns `None` for remote accounts
        // today, so this branch is unreachable for a Pending op. The
        // generous timeout here (matching the per-step driver budget)
        // is purely defensive: if a future signer impl violates the
        // contract by returning Pending on a synchronous-looking path,
        // we surface a toast rather than wedge.
        let envelope = match op.wait(nmp_nip59::DRIVER_STEP_TIMEOUT) {
            Ok(ev) => ev,
            Err(e) => {
                kernel.set_last_error_toast(Some(format!(
                    "cannot send DM: gift-wrap ({label}) failed: {e}"
                )));
                return Vec::new();
            }
        };
        // The kind:1059 envelope is already signed by its ephemeral key. Route
        // it through the signed-event publish path so the kernel verifies and
        // forwards it VERBATIM — re-signing with the account key would destroy
        // the unlinkability gift-wrap exists to provide.
        let raw = nostr_event_to_raw(&envelope);
        // NIP-17 § 2: pin the envelope to the receiver's kind:10050 DM-inbox
        // relays. `recipient_dm_relays` reads the live kind:10050 cache; it
        // returns `None` only when the receiver has never published a
        // kind:10050 list. In that genuinely-missing case fall back to the
        // configured Content relays AND warn, so the routing gap is visible in
        // logs rather than a silent delivery failure (the recipient simply
        // never receiving the message).
        let relays = kernel.recipient_dm_relays(receiver_hex).unwrap_or_else(|| {
            tracing::warn!(
                envelope = label,
                "NIP-17 DM: no cached kind:10050 DM-relay list for receiver; \
                 falling back to configured Content relays — delivery may be \
                 lost if the receiver reads a different relay set"
            );
            kernel.bootstrap_urls_for_role(crate::relay::RelayRole::Content)
        });
        // D10 empty-relay guard — `bootstrap_urls_for_role` may return empty
        // in production when the operator has configured no Content relays.
        // `publish_signed_event` would then map the empty slice → Auto and
        // leak the kind:1059 envelope through the author's NIP-65 outbox.
        // Refuse the publish on the empty branch; the envelope is dropped.
        // This is the call-site twin of the `nmp-marmot::publish_to` D10
        // guard PR-K added at the Marmot bridge — every kind:1059 publish
        // surface now refuses an empty relay set.
        if is_empty_relays_kind1059_block(&relays) {
            tracing::warn!(
                envelope = label,
                "NIP-17 DM: kind:1059 publish refused — no DM-inbox relays for \
                 receiver and no configured Content relays to fall back to; \
                 D10 forbids Auto-routing the encrypted envelope to the \
                 author's NIP-65 outbox"
            );
            empty_relay_skips.push(label);
            continue;
        }
        // NIP-17 gift-wrap is not a `dispatch_action` path — no host
        // correlation_id to thread; the engine falls back to the publish
        // handle (== gift-wrap envelope id) as before.
        //
        // The `doctrine-allow: D10 — …` annotation MUST be a trailing
        // comment on the offending line itself (the lint parser is line-
        // scoped); the prose reason here covers why it is safe:
        // `is_empty_relays_kind1059_block` above proved `relays` non-empty
        // before this call, so `publish_signed_event` cannot fall through
        // to its `relays.is_empty()` → `PublishTarget::Auto` branch and
        // cannot leak the kind:1059 envelope to the author's NIP-65 outbox.
        outbound.extend(super::publish::publish_signed_event( // doctrine-allow: D10 — empty-relay kind:1059 Auto-route guarded above by is_empty_relays_kind1059_block
            kernel, raw, &relays, None,
        ));
    }

    // If any envelope was skipped because its relay set resolved empty,
    // surface a single toast — D6: the failure is observable state, never a
    // silent drop. The toast names the affected envelope so the user can
    // tell whether the recipient or the self-copy (or both) failed.
    if !empty_relay_skips.is_empty() {
        let which = empty_relay_skips.join(", ");
        kernel.set_last_error_toast(Some(format!(
            "Cannot send DM: no DM-inbox relays configured for {which} \
             (publish skipped to avoid leaking encrypted envelope to public relays)"
        )));
    }

    outbound
}

/// D10 empty-relay guard predicate — true iff this kind:1059 publish must
/// be refused because the resolved relay set is empty.
///
/// Centralizes the gate so the unit test in this module can pin its shape
/// directly (mirrors the Marmot `is_d10_blocked` helper next to
/// `publish_to`). Production behaviour: `publish_signed_event` with an empty
/// `relays` slice falls through to `PublishTarget::Auto`, which publishes to
/// the author's NIP-65 outbox — for a kind:1059 envelope that is a D10
/// violation by construction (the envelope's *existence* leaks to every
/// public relay the author advertises). The caller refuses the publish on
/// `true`.
///
/// This predicate is intentionally relay-shape only: the caller has already
/// constructed a kind:1059 envelope by the time it consults the guard, so
/// the kind discrimination the Marmot twin needs (it handles 1059/445/30443
/// in one bridge) is not required here.
pub(crate) fn is_empty_relays_kind1059_block(relays: &[String]) -> bool {
    relays.is_empty()
}

/// Build a `nostr::UnsignedEvent` (the rumor) from the substrate flat repr.
///
/// Mirrors `commands::publish::sign_with`'s tag/kind validation, but stops at
/// `EventBuilder::build` — the rumor is unsigned by design (NIP-59 seals it).
fn build_nostr_rumor(
    rumor: &UnsignedEvent,
    pubkey: PublicKey,
) -> Result<nostr::UnsignedEvent, String> {
    if rumor.kind > u16::MAX as u32 {
        return Err(format!(
            "invalid kind {}: must be in range [0, 65535]",
            rumor.kind
        ));
    }
    let kind = Kind::from_u16(rumor.kind as u16);

    let mut tags = Vec::with_capacity(rumor.tags.len());
    let mut malformed = 0usize;
    for t in &rumor.tags {
        match Tag::parse(t) {
            Ok(tag) => tags.push(tag),
            Err(_) => malformed += 1,
        }
    }
    if malformed > 0 {
        return Err(format!("dropped {malformed} malformed tag(s)"));
    }

    Ok(EventBuilder::new(kind, &rumor.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(rumor.created_at))
        .build(pubkey))
}

/// Convert a signed `nostr::Event` (the kind:1059 gift-wrap) to the kernel's
/// flat [`RawEvent`]. The signature and id are carried through verbatim — the
/// signed-event publish path verifies them and forwards the event unchanged.
fn nostr_event_to_raw(event: &nostr::Event) -> RawEvent {
    RawEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: event.kind.as_u16() as u32,
        tags: event
            .tags
            .iter()
            .map(|t| t.as_slice().to_vec())
            .collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::commands::identity::sign_in_nsec;
    use crate::actor::commands::new_bunker_handshake_slot;
    use crate::actor::ActorCommand;
    use crate::relay::DEFAULT_VISIBLE_LIMIT;

    const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";
    const RECIPIENT: &str =
        "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

    fn fresh() -> (IdentityRuntime, Kernel) {
        (
            IdentityRuntime::new(new_bunker_handshake_slot()),
            Kernel::new(DEFAULT_VISIBLE_LIMIT),
        )
    }

    /// A kind:14 rumor with a `created_at: 0` sentinel — what
    /// `nmp_nip17::build_dm_rumor` produces.
    fn sample_rumor(sender_pubkey: &str) -> UnsignedEvent {
        UnsignedEvent {
            pubkey: sender_pubkey.to_string(),
            kind: 14,
            tags: vec![vec!["p".to_string(), RECIPIENT.to_string()]],
            content: "hello over NIP-17".to_string(),
            created_at: 0,
        }
    }

    #[test]
    fn send_gift_wrapped_dm_without_account_toasts_and_emits_nothing() {
        let (identity, mut kernel) = fresh();
        let rumor = sample_rumor(
            "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
        );
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, RECIPIENT);
        assert!(
            outbound.is_empty(),
            "no active account → no envelopes published"
        );
        assert!(
            kernel.last_error_toast_snapshot().is_some(),
            "D6: the failure is surfaced as a toast, never silent"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_rejects_malformed_recipient_pubkey() {
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");
        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, "not-a-pubkey");
        assert!(outbound.is_empty(), "malformed recipient → nothing published");
        assert!(
            kernel
                .last_error_toast_snapshot()
                .map(|t| t.contains("recipient pubkey"))
                .unwrap_or(false),
            "D6: malformed recipient pubkey is surfaced as a toast"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_with_local_key_gift_wraps_recipient_and_self() {
        // With a local nsec the handler must seal+wrap the rumor twice (one
        // envelope per recipient, one self-copy) and publish both — no toast.
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");
        kernel.seed_kind10002_for_test(&sender, &["wss://dm-relay.test"]);

        // NIP-59 gift-wrap performs a NIP-44 ECDH against the recipient key, so
        // the recipient pubkey MUST be a real secp256k1 curve point. Derive one
        // from a freshly generated keypair rather than a hand-typed hex string.
        let recipient_pk = nostr::Keys::generate().public_key().to_hex();

        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk);

        assert!(
            kernel.last_error_toast_snapshot().is_none(),
            "a local-key gift-wrap send must not toast an error: {:?}",
            kernel.last_error_toast_snapshot()
        );
        // Two kind:1059 envelopes (recipient + self-copy) were published; each
        // produces at least one outbound EVENT frame to the configured relay.
        assert!(
            !outbound.is_empty(),
            "both gift-wrap envelopes should produce outbound frames"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_without_kind10050_falls_back_without_toasting() {
        // NIP-17 § 2 routing: each envelope should go to its receiver's
        // kind:10050 DM-inbox relays. kind:10050 ingestion is not yet built,
        // so `recipient_dm_relays` returns `None` and the handler falls back
        // to the configured Content relays. That fallback is a *diagnostic*
        // gap (a `tracing::warn!`), NOT a user-facing failure: the send still
        // succeeds and `last_error_toast_snapshot()` must stay `None` so the
        // D6 toast channel is reserved for genuine errors (no local key,
        // malformed pubkey). This test pins that distinction.
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");

        // No kind:10050 cache exists for anyone — the receiver pubkey is a
        // fresh keypair with no relay data of any kind.
        let recipient_pk = nostr::Keys::generate().public_key().to_hex();

        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk);

        assert!(
            kernel.last_error_toast_snapshot().is_none(),
            "the kind:10050 fallback is a warn-level diagnostic, not a toast: {:?}",
            kernel.last_error_toast_snapshot()
        );
        assert!(
            !outbound.is_empty(),
            "the send still succeeds via the Content-relay fallback"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_routes_recipient_envelope_to_kind10050_relays() {
        // NIP-17 § 2 routing: when the recipient HAS published a kind:10050
        // DM-relay list, the recipient envelope must be pinned to that list —
        // not the sender's Content relays. This is the close of the
        // inert-seam-with-warn bug: `recipient_dm_relays` now reads a live
        // kind:10050 cache, so the recipient envelope routes correctly.
        let (mut identity, mut kernel) = fresh();
        sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
        let sender = identity.active_pubkey().expect("signed in");

        // The recipient publishes a kind:10050 DM-relay list. The URL is given
        // in canonical form (no empty-path trailing slash) so the seeded value
        // and the resolved `recipient_dm_relays` value compare exactly.
        let recipient_keys = nostr::Keys::generate();
        let recipient_pk = recipient_keys.public_key().to_hex();
        kernel.seed_kind10050_for_test(&recipient_pk, &["wss://recipient-dm.relay"]);

        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk);

        assert!(
            kernel.last_error_toast_snapshot().is_none(),
            "a kind:10050-routed send must not toast: {:?}",
            kernel.last_error_toast_snapshot()
        );
        // The recipient envelope must have been published to the recipient's
        // kind:10050 DM-relay list.
        assert!(
            outbound
                .iter()
                .any(|m| m.relay_url == "wss://recipient-dm.relay"),
            "the recipient envelope must route to the recipient's kind:10050 \
             DM-relay list; got: {:?}",
            outbound.iter().map(|m| &m.relay_url).collect::<Vec<_>>()
        );
    }

    // ── D10 empty-relay guard ────────────────────────────────────────────
    //
    // The bootstrap_urls fallback at `kernel::bootstrap_urls_for_role` is
    // back-stopped under `#[cfg(test)]` so it ALWAYS returns a default URL
    // in test builds, which means we cannot directly exercise the empty-
    // bootstrap branch end-to-end here. We pin the guard's shape via the
    // [`is_empty_relays_kind1059_block`] predicate (mirrors the Marmot
    // `is_d10_blocked` test pattern) and assert: (a) the predicate fires on
    // empty relays, (b) it does NOT fire when relays are present. The
    // call-site wiring is exercised end-to-end by the existing
    // `send_gift_wrapped_dm_routes_recipient_envelope_to_kind10050_relays`
    // test (kind:10050 seeded → non-empty → predicate false → publish).

    #[test]
    fn is_empty_relays_kind1059_block_fires_on_empty_slice() {
        // Production scenario: no kind:10050 cache for the receiver AND
        // `bootstrap_urls_for_role(Content)` returns empty (operator has
        // configured no Content relays). The guard MUST refuse the publish
        // so the kind:1059 envelope never reaches `publish_signed_event`'s
        // Auto-fallback path.
        let empty: Vec<String> = Vec::new();
        assert!(
            is_empty_relays_kind1059_block(&empty),
            "empty relays for a kind:1059 publish must be blocked (D10)"
        );
    }

    #[test]
    fn is_empty_relays_kind1059_block_passes_with_kind10050_pin() {
        // The recipient has published a kind:10050 DM-relay list — the
        // explicit pin is non-empty and the guard MUST NOT block.
        let pin = vec!["wss://recipient-dm.relay".to_string()];
        assert!(
            !is_empty_relays_kind1059_block(&pin),
            "an explicit kind:10050 pin must pass the D10 guard"
        );
    }

    #[test]
    fn is_empty_relays_kind1059_block_passes_with_content_fallback() {
        // The bootstrap Content-relay fallback is non-empty (operator has
        // configured at least one Content relay). The guard MUST NOT block —
        // the recipient may read a different relay set so delivery is best-
        // effort, but the envelope is going to a caller-supplied relay, not
        // the Auto outbox.
        let fallback = vec!["wss://relay.damus.io".to_string()];
        assert!(
            !is_empty_relays_kind1059_block(&fallback),
            "a non-empty Content fallback must pass the D10 guard"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_variant_is_matched_in_dispatch() {
        // Compile-time guard: the `ActorCommand::SendGiftWrappedDm` variant
        // exists with the documented shape and constructs cleanly. The actual
        // dispatch arm is exercised end-to-end by the actor loop tests; this
        // pins the variant signature so a rename breaks the build here.
        let cmd = ActorCommand::SendGiftWrappedDm {
            rumor: sample_rumor(
                "aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee",
            ),
            recipient_pubkey: RECIPIENT.to_string(),
        };
        match cmd {
            ActorCommand::SendGiftWrappedDm {
                rumor,
                recipient_pubkey,
            } => {
                assert_eq!(rumor.kind, 14, "the carried rumor is a kind:14");
                assert_eq!(recipient_pubkey, RECIPIENT);
            }
            _ => panic!("expected SendGiftWrappedDm variant"),
        }
    }
}
