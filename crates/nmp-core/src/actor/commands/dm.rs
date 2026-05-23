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
//! - `Some(Arc<dyn SignerForSeal>)` for a remote account — ADR-0026
//!   Phase 2. The `RemoteSignerForSeal` adapter (`commands::
//!   remote_signer_for_seal`) bridges the substrate `SignedEvent` ↔
//!   `nostr::Event` shapes and routes `nip44_encrypt` + `sign` through
//!   the active `RemoteSignerHandle`. `gift_wrap_with_signer` spawns a
//!   per-invocation driver thread for the `SignerOp::Pending` chain so
//!   the actor itself does not block on bunker RPCs (the actor still
//!   waits below via `op.wait(GIFT_WRAP_TOTAL_TIMEOUT)`; the 12s budget
//!   covers both bunker RPCs plus the in-process wrap assembly).
//! - `None` only when no account is active, OR when the active bunker
//!   handle returns a malformed `pubkey_hex()` (graceful-degrade).
//!
//! It deliberately does NOT read the `NmpApp::mls_local_nsec` FFI field
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
//! ingested, and `None` for a receiver who has never published one or who
//! published an empty list. `None` is not a safe substitute for Content relay
//! routing: a kind:1059 envelope must stay pinned to the receiver's DM inbox
//! relays. The send therefore fails closed with a toast and emits no publish
//! frames until both the recipient and self-copy kind:10050 lists are known.
//!
//! ## D10 fail-closed — kind:1059 NEVER Auto-routes
//!
//! `recipient_dm_relays` returns `None` for a missing OR empty kind:10050
//! cache entry (outbox.rs guards `relays.is_empty()` and returns `None`). The
//! [`required_dm_relays`] helper turns that `None` into an early
//! [`DmRelayNotReady`] error: the handler returns before any gift-wrap is
//! built, so `publish_signed_event` is never called with an empty relay slice
//! and can never fall through to `PublishTarget::Auto`. This is the call-site
//! (NIP-17) twin of the `nmp-marmot::projection::publish::publish_to` Marmot
//! guard at the Marmot bridge — every kind:1059 publish surface refuses to
//! substitute generic Content relays for a recipient's DM-inbox pin.

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
    correlation_id: Option<String>,
) -> Vec<OutboundMessage> {
    // D10: private-kind publish
    // Every publish call below MUST route via an Explicit pin (the receiver's
    // kind:10050 DM-inbox relays). Missing or empty DM-inbox lists fail closed
    // before any gift-wrap is built, so this path never substitutes generic
    // Content relays for a kind:1059 envelope. The doctrine-lint D10 rule
    // enforces this structurally: any Auto-routing seam (`PublishTarget::Auto`,
    // `publish_signed(...)`, `publish_unsigned_event(...)`) below this marker
    // fires a lint finding.
    //
    // 1. Resolve a `SignerForSeal` for the active account (ADR-0026 seam).
    //    `None` only when there is no active account at all, or the remote
    //    signer reported a malformed pubkey (RemoteSignerForSeal graceful-
    //    degrade). Both local and remote (NIP-46) signers are supported.
    let Some(signer) = identity.active_signer_for_seal() else {
        // `active_signer_for_seal` only returns `None` when there is no
        // active account at all, OR when the active remote signer
        // reported a malformed `pubkey_hex()` (RemoteSignerForSeal::new
        // graceful-degrade path). Both surface as toasts (D6).
        let reason = match identity.active_signer_kind() {
            Some("local") => {
                // Unreachable in practice — a local signer always produces
                // Some — but report it as a D6 error rather than a panic
                // if the invariant ever breaks.
                "internal error: local account did not produce a SignerForSeal"
                    .to_string()
            }
            Some(other) => format!(
                "cannot send DM: remote signer ({other}) reported a malformed pubkey"
            ),
            None => "cannot send DM: no active account".to_string(),
        };
        kernel.set_last_error_toast(Some(reason.clone()));
        // Broken-promise fix: if the dispatch arm already recorded
        // `Requested` against this correlation_id, an early-exit must
        // record `Failed` or the host's spinner hangs forever. The
        // `record_action_failure` helper writes BOTH the action_results
        // terminal AND the `action_stages` `Failed` mirror entry. No-op
        // for `None` (non-dispatch callers — conformance harnesses have
        // nothing waiting on an id).
        if let Some(id) = correlation_id.clone() {
            kernel.record_action_failure(id, reason);
        }
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
            let toast = format!("cannot send DM: {reason}");
            kernel.set_last_error_toast(Some(toast.clone()));
            // Broken-promise fix (see signer branch above).
            if let Some(id) = correlation_id.clone() {
                kernel.record_action_failure(id, toast);
            }
            return Vec::new();
        }
    };

    // Recipient pubkey must parse — a malformed hex pubkey is a caller bug;
    // refuse the send rather than wrap to a garbage key (D6).
    let recipient = match PublicKey::parse(recipient_pubkey) {
        Ok(pk) => pk,
        Err(e) => {
            let toast = format!("cannot send DM: malformed recipient pubkey: {e}");
            kernel.set_last_error_toast(Some(toast.clone()));
            // Broken-promise fix (see signer branch above).
            if let Some(id) = correlation_id.clone() {
                kernel.record_action_failure(id, toast);
            }
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
    //    the actor thread and `wait()` resolves synchronously. On the
    //    ADR-0026 Phase 2 REMOTE path (`RemoteSignerForSeal` adapter,
    //    `commands::remote_signer_for_seal`), `gift_wrap_with_signer`
    //    spawns a per-invocation driver thread for the encrypt → sign →
    //    wrap chain and `wait()` blocks the actor for up to
    //    `GIFT_WRAP_TOTAL_TIMEOUT` (12s). Routing this into the actor's
    //    `pending_signs` queue (so the actor never blocks at all) is a
    //    documented follow-up — bounded blocking is the deliberate
    //    trade-off chosen for the substrate-thin Phase 2 landing.
    let sender_hex = sender.to_hex();

    // D10 fail-closed gate — resolve BOTH receivers' kind:10050 DM-inbox
    // relays before constructing any envelope. `required_dm_relays` rejects
    // the missing/empty cache cases up-front, so we never reach
    // `publish_signed_event` with an empty relay slice (which would fall
    // through to `PublishTarget::Auto` and leak the kind:1059 envelope to
    // the author's NIP-65 outbox).
    let recipient_relays = match required_dm_relays(kernel, "recipient", recipient_pubkey) {
        Ok(relays) => relays,
        Err(err) => {
            tracing::warn!(
                envelope = err.envelope,
                receiver_pubkey = err.receiver_pubkey.as_str(),
                "NIP-17 DM send blocked: missing or empty kind:10050 \
                 DM-relay list; refusing Content relay fallback"
            );
            let toast = err.toast();
            kernel.set_last_error_toast(Some(toast.clone()));
            // Broken-promise fix (see signer branch above): the dispatch
            // arm already recorded `Requested`; close it with `Failed`.
            if let Some(id) = correlation_id.clone() {
                kernel.record_action_failure(id, toast);
            }
            return Vec::new();
        }
    };
    let self_relays = match required_dm_relays(kernel, "self-copy", sender_hex.as_str()) {
        Ok(relays) => relays,
        Err(err) => {
            tracing::warn!(
                envelope = err.envelope,
                receiver_pubkey = err.receiver_pubkey.as_str(),
                "NIP-17 DM send blocked: missing or empty kind:10050 \
                 DM-relay list; refusing Content relay fallback"
            );
            let toast = err.toast();
            kernel.set_last_error_toast(Some(toast.clone()));
            // Broken-promise fix (see signer branch above).
            if let Some(id) = correlation_id.clone() {
                kernel.record_action_failure(id, toast);
            }
            return Vec::new();
        }
    };

    let mut outbound = Vec::new();
    for (label, receiver, relays) in [
        ("recipient", &recipient, recipient_relays),
        ("self-copy", &sender, self_relays),
    ] {
        // NIP-59 randomises the kind:13 + kind:1059 timestamps in a 2-day
        // window so an observer cannot correlate the envelope timestamp
        // with the underlying rumor; mirrors the behaviour of
        // `nostr::nips::nip59::make_seal`.
        let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
        let op = nmp_nip59::gift_wrap_with_signer(
            &signer,
            receiver,
            &nostr_rumor,
            tweaked,
        );
        // LOCAL fast path: every `SignerOp` is `Ready`, so `wait`
        // resolves immediately regardless of the timeout argument.
        //
        // REMOTE path (ADR-0026 Phase 2): `gift_wrap_with_signer` spawns
        // a driver thread that owns both bunker RPCs (encrypt + sign);
        // the actor blocks here on the resulting `Pending` channel for
        // up to `GIFT_WRAP_TOTAL_TIMEOUT` (12s) — the per-step budget is
        // `DRIVER_STEP_TIMEOUT` (5s), and the total covers both steps
        // plus the in-process wrap assembly with 2s of headroom. Using
        // the per-step budget instead would silently fail a real bunker
        // mid-chain: the driver would still succeed but the actor would
        // already have timed out and reported a misleading toast.
        let envelope = match op.wait(nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT) {
            Ok(ev) => ev,
            Err(e) => {
                let toast = format!("cannot send DM: gift-wrap ({label}) failed: {e}");
                kernel.set_last_error_toast(Some(toast.clone()));
                // Broken-promise fix (see signer branch above). A gift-wrap
                // timeout / sign error is the most likely failure path on the
                // ADR-0026 remote-signer route (bunker RPC stall) — the host
                // spinner MUST clear.
                if let Some(id) = correlation_id.clone() {
                    kernel.record_action_failure(id, toast);
                }
                return Vec::new();
            }
        };
        // The kind:1059 envelope is already signed by its ephemeral key. Route
        // it through the signed-event publish path so the kernel verifies and
        // forwards it VERBATIM — re-signing with the account key would destroy
        // the unlinkability gift-wrap exists to provide.
        let raw = nostr_event_to_raw(&envelope);
        // NIP-17 § 2: `relays` is the receiver's kind:10050 DM-inbox pin
        // resolved up-front by `required_dm_relays`. The helper returns an
        // early error for missing OR empty kind:10050 lists (outbox.rs
        // guarantees `recipient_dm_relays` never returns an empty `Some`), so
        // by the time we reach this call `relays` is provably non-empty. That
        // makes the `relays.is_empty()` → `PublishTarget::Auto` fall-through
        // in `publish_signed_event` structurally unreachable for kind:1059 —
        // the D10 leak the Marmot guard exists to prevent cannot fire here.
        //
        // `correlation_id` (when the send originated from `dispatch_action`)
        // is threaded into the publish engine's `correlation_id_override` for
        // BOTH envelopes (recipient + self-copy). Each kind:1059 envelope has
        // its own ephemeral key and therefore a distinct `event_id` /
        // `PublishHandle`, so `start_publish` cannot collide on
        // `DuplicateHandle`. Both publishes ultimately record terminal
        // verdicts against the same correlation_id — the host's spinner
        // resolves on the first terminal (`action_results` is drained as a
        // `Vec` per tick, so the second verdict is a benign duplicate the UI
        // can treat as idempotent). This is consistent with the doctrine
        // tradeoff: a DM send is "delivered" only when at least one of the
        // two envelopes lands; threading the id into both is the
        // narrowest-blast-radius wiring that closes the spinner round-trip.
        //
        // The `doctrine-allow: D10 — …` annotation MUST be a trailing comment
        // on the offending line itself (the lint parser is line-scoped); the
        // prose reason here covers why it is safe: `required_dm_relays` above
        // rejected the missing/empty branch before any envelope was built, so
        // by the time we call `publish_signed_event` we are guaranteed to be
        // passing `PublishTarget::Explicit { relays }` with a non-empty relay
        // set. The publish command also validates the typed target up front
        // and fails closed for an empty `Explicit`, so kind:1059 can never
        // degrade to `PublishTarget::Auto` and leak the envelope to the
        // author's NIP-65 outbox.
        outbound.extend(super::publish::publish_signed_event( // doctrine-allow: D10 — required_dm_relays rejects missing/empty kind:10050 lists before any kind:1059 is built; publish helper additionally fails closed on an empty PublishTarget::Explicit
            kernel,
            raw,
            crate::publish::PublishTarget::Explicit { relays },
            correlation_id.clone(),
        ));
    }

    outbound
}

/// Receiver-side readiness error for the kind:10050 fail-closed gate.
///
/// `recipient_dm_relays` returns `None` for both the missing (never published
/// a kind:10050) and empty (published one with no `relay` tags, which
/// `ingest_dm_relay_list` collapses to "no entry" — see outbox.rs guard)
/// cases. Both are unsafe substitutes for explicit kind:10050 routing for a
/// kind:1059 envelope, so [`required_dm_relays`] converts that `None` into
/// this error and the send fails closed before any gift-wrap is built. The
/// `envelope` field names which of the two NIP-17 envelopes (`"recipient"`
/// or `"self-copy"`) lacks a list, so the D6 toast can be specific.
#[derive(Clone, Debug, Eq, PartialEq)]
struct DmRelayNotReady {
    envelope: &'static str,
    receiver_pubkey: String,
}

impl DmRelayNotReady {
    fn toast(&self) -> String {
        format!(
            "cannot send DM: {} has no kind:10050 DM relay list yet",
            self.envelope
        )
    }
}

/// D10 fail-closed gate — resolve a receiver's kind:10050 DM-inbox relays or
/// return a [`DmRelayNotReady`] error.
///
/// This is the call-site (NIP-17) twin of the Marmot `publish_to` guard:
/// every kind:1059 publish surface refuses to substitute generic Content
/// relays for a recipient's DM-inbox pin. By rejecting the `None` branch
/// before any gift-wrap is built, `publish_signed_event` is never called with
/// an empty relay slice and cannot fall through to `PublishTarget::Auto`,
/// which would leak the encrypted envelope to the author's NIP-65 outbox.
fn required_dm_relays(
    kernel: &Kernel,
    envelope: &'static str,
    receiver_pubkey: &str,
) -> Result<Vec<String>, DmRelayNotReady> {
    kernel
        .recipient_dm_relays(receiver_pubkey)
        .ok_or_else(|| DmRelayNotReady {
            envelope,
            receiver_pubkey: receiver_pubkey.to_string(),
        })
}

/// Build a `nostr::UnsignedEvent` (the rumor) from the substrate flat repr.
///
/// Mirrors `commands::publish::sign_with`'s tag/kind validation, but stops at
/// `EventBuilder::build` — the rumor is unsigned by design (NIP-59 seals it).
fn build_nostr_rumor(
    rumor: &UnsignedEvent,
    pubkey: PublicKey,
) -> Result<nostr::UnsignedEvent, String> {
    if rumor.kind > u32::from(u16::MAX) {
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
        kind: u32::from(event.kind.as_u16()),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
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
    const RECIPIENT: &str = "bb11223344556677889900aabbccddeeff00112233445566778899aabbccddff";

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
        let rumor =
            sample_rumor("aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee");
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, RECIPIENT, None);
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
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, "not-a-pubkey", None);
        assert!(
            outbound.is_empty(),
            "malformed recipient → nothing published"
        );
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

        // NIP-59 gift-wrap performs a NIP-44 ECDH against the recipient key, so
        // the recipient pubkey MUST be a real secp256k1 curve point. Derive one
        // from a freshly generated keypair rather than a hand-typed hex string.
        let recipient_pk = nostr::Keys::generate().public_key().to_hex();
        kernel.seed_kind10050_for_test(&sender, &["wss://sender-dm.relay"]);
        kernel.seed_kind10050_for_test(&recipient_pk, &["wss://recipient-dm.relay"]);

        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk, None);

        assert!(
            kernel.last_error_toast_snapshot().is_none(),
            "a local-key gift-wrap send must not toast an error: {:?}",
            kernel.last_error_toast_snapshot()
        );
        let mut got: Vec<String> = outbound.iter().map(|m| m.relay_url.clone()).collect();
        got.sort();
        assert_eq!(
            got,
            vec![
                "wss://recipient-dm.relay".to_string(),
                "wss://sender-dm.relay".to_string(),
            ],
            "recipient and self-copy envelopes must use kind:10050 relays"
        );
    }

    #[test]
    fn send_gift_wrapped_dm_without_recipient_kind10050_fails_closed() {
        for empty_kind10050 in [false, true] {
            let (mut identity, mut kernel) = fresh();
            sign_in_nsec(&mut identity, &mut kernel, TEST_NSEC, false);
            let sender = identity.active_pubkey().expect("signed in");
            let recipient_pk = nostr::Keys::generate().public_key().to_hex();
            kernel.seed_kind10050_for_test(&sender, &["wss://sender-dm.relay"]);
            if empty_kind10050 {
                let id = format!("{:064x}", 0x1050);
                let _ = kernel.inject_replaceable_event(
                    &id,
                    &recipient_pk,
                    1_000,
                    10050,
                    Vec::new(),
                    "wss://seed",
                    1_700_000_000_000,
                );
                assert!(kernel.recipient_dm_relays(&recipient_pk).is_none());
            }

            let content_relays = kernel.bootstrap_urls_for_role(crate::relay::RelayRole::Content);
            let rumor = sample_rumor(&sender);
            let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk, None);

            assert!(outbound.is_empty(), "missing/empty kind:10050 must not publish");
            assert!(outbound.iter().all(|m| !content_relays.contains(&m.relay_url)));
            assert!(
                kernel
                    .last_error_toast_snapshot()
                    .is_some_and(|t| t.contains("kind:10050")),
                "fail-closed send must surface a kind:10050 readiness error"
            );
        }
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
        kernel.seed_kind10050_for_test(&sender, &["wss://sender-dm.relay"]);
        kernel.seed_kind10050_for_test(&recipient_pk, &["wss://recipient-dm.relay"]);

        let rumor = sample_rumor(&sender);
        let outbound = send_gift_wrapped_dm(&identity, &mut kernel, rumor, &recipient_pk, None);

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

    // ── D10 fail-closed coverage ─────────────────────────────────────────
    //
    // The fail-closed gate (`required_dm_relays`) is exercised end-to-end by
    // `send_gift_wrapped_dm_without_recipient_kind10050_fails_closed` above,
    // which covers BOTH branches of the kind:10050 cache miss:
    //   1. The receiver has never published a kind:10050 — `dm_relay_lists`
    //      has no entry, `recipient_dm_relays` returns `None`, the helper
    //      converts that to `DmRelayNotReady` and the send aborts with a toast.
    //   2. The receiver published an EMPTY kind:10050 — `ingest_dm_relay_list`
    //      removes the cached entry (outbox.rs guards `relays.is_empty()` →
    //      `None`), so the path is structurally identical to case (1).
    // Both branches are pinned in one parameterised test (the `for empty_kind10050`
    // loop), so the predicate-shape micro-tests the previous design needed are
    // subsumed: there is no relay-shape predicate to assert against because
    // the gate happens before any envelope is constructed.

    #[test]
    fn send_gift_wrapped_dm_variant_is_matched_in_dispatch() {
        // Compile-time guard: the `ActorCommand::SendGiftWrappedDm` variant
        // exists with the documented shape and constructs cleanly. The actual
        // dispatch arm is exercised end-to-end by the actor loop tests; this
        // pins the variant signature so a rename (or a missing
        // `correlation_id` field, which would silently break the
        // dispatch-action spinner round-trip) breaks the build here.
        let cmd = ActorCommand::SendGiftWrappedDm {
            rumor: sample_rumor("aa11223344556677889900aabbccddeeff00112233445566778899aabbccddee"),
            recipient_pubkey: RECIPIENT.to_string(),
            correlation_id: Some("cid-abc".to_string()),
        };
        match cmd {
            ActorCommand::SendGiftWrappedDm {
                rumor,
                recipient_pubkey,
                correlation_id,
            } => {
                assert_eq!(rumor.kind, 14, "the carried rumor is a kind:14");
                assert_eq!(recipient_pubkey, RECIPIENT);
                assert_eq!(
                    correlation_id.as_deref(),
                    Some("cid-abc"),
                    "the variant carries the dispatched correlation_id so the \
                     actor can record `Requested` and the publish engine can \
                     report the terminal verdict against it"
                );
            }
            _ => panic!("expected SendGiftWrappedDm variant"),
        }
    }
}
