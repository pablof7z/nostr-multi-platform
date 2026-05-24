//! `SendGiftWrappedDmCommand` — the NIP-17 gift-wrapped DM send handler.
//!
//! V-39 migration: the implementation was lifted from
//! `nmp_core::actor::commands::dm.rs` (deleted at the same revision) and
//! re-shaped as a [`ProtocolCommand`] so it lives in the right crate
//! ([`nmp-nip17`]) and reaches the kernel through the substrate-generic
//! [`ProtocolCommandContext`] rather than a bespoke `ActorCommand`
//! variant. The wire semantics are identical to the pre-migration code:
//!
//! * Two kind:1059 envelopes are produced — one to the recipient, one
//!   self-copy (the sender gift-wraps to their own pubkey so sent
//!   messages stay readable across clients).
//! * Each envelope is pinned to *its receiver's* kind:10050 DM-inbox
//!   relays (NIP-17 § 2). Missing or empty kind:10050 lists fail closed
//!   with a D6 toast — kind:1059 NEVER falls back to generic Content
//!   relays.
//! * The rumor's `created_at` is re-stamped from the kernel clock (D7).
//! * Failure paths set `last_error_toast` AND record a `Failed` terminal
//!   action stage so the host spinner clears.
//!
//! # Signer resolution (V-08 — bunker DM send)
//!
//! `nmp_nip59::gift_wrap_with_signer` accepts an `Arc<dyn SignerForSeal>`.
//! The signer for the active account is resolved through
//! `ProtocolCommandContext::signer_for_seal()`, which transparently
//! handles BOTH:
//!
//! * **Local-nsec accounts** — the blanket impl on `nostr::Keys` makes
//!   every chain step `Ready`, so the seal runs synchronously on the
//!   actor thread.
//! * **NIP-46 bunker accounts** — `RemoteSignerForSeal` adapts the
//!   active `RemoteSignerHandle`; `gift_wrap_with_signer` spawns a
//!   per-invocation driver thread so the actor itself never blocks on
//!   bunker RPCs (the actor waits below via
//!   `op.wait(GIFT_WRAP_TOTAL_TIMEOUT)`; the 12s budget covers both the
//!   `nip44_encrypt` + `sign_seal` round-trips plus in-process wrap
//!   assembly).
//!
//! `None` from `signer_for_seal()` means either no active account OR a
//! remote signer that reported a malformed pubkey; both surface a D6
//! toast and a `Failed` terminal stage so the host spinner clears.
//!
//! V-39 shipped a local-only MVP that read `ctx.nip17_local_keys()` and
//! refused bunker accounts; V-08 closes that regression by routing
//! through the substrate-generic `SignerForSeal` seam.
//!
//! # D doctrine
//!
//! * **D0** — the substrate (`nmp-core`) holds no NIP-17 nouns; this
//!   crate owns the kind:1059 wire shape, the kind:10050 cache, and the
//!   gift-wrap orchestration.
//! * **D6** — every failure path sets a toast AND records an action
//!   failure when a `correlation_id` was supplied; no silent drops.
//! * **D7** — the kernel-owned wall clock stamps `created_at`; the host
//!   sends `0` as the sentinel.
//! * **D10** — the publish path uses `PublishTarget::Explicit { relays }`
//!   with a non-empty slice (the `required_dm_relays` gate rejects the
//!   empty / missing branches before any envelope is constructed).
//! * **D15** — all closures the context exposes wrap in `catch_unwind`
//!   internally; the command body itself does not need to.

use nmp_core::publish::PublishTarget;
use nmp_core::substrate::{
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError, UnsignedEvent,
};
use nmp_core::ActorCommand;
use nostr::{
    nips::nip59::RANGE_RANDOM_TIMESTAMP_TWEAK, EventBuilder, Kind, PublicKey, Tag, Timestamp,
};

/// NIP-17 § 2 gift-wrap publish — the [`ProtocolCommand`] equivalent of
/// the legacy `ActorCommand::SendGiftWrappedDm` variant.
///
/// Construct one of these in the action executor (`SendDmAction::execute`)
/// and dispatch via `ActorCommand::Protocol(Box::new(cmd))`. The actor
/// runs it on the actor thread; the body resolves the active signer,
/// reads the recipient + sender DM-inbox relays, gift-wraps the rumor
/// twice (recipient + self-copy), and dispatches each kind:1059 envelope
/// back through the substrate via [`ProtocolCommandContext::send`]
/// (`ActorCommand::PublishSignedEvent` with `PublishTarget::Explicit`).
#[derive(Clone, Debug)]
pub struct SendGiftWrappedDmCommand {
    /// The kind:14 chat-message rumor (unsigned) the host built via
    /// [`crate::build_dm_rumor`]. `created_at == 0` is the kernel-stamp
    /// sentinel; the executor re-stamps from `ctx.now_secs()`.
    pub rumor: UnsignedEvent,
    /// Recipient's Nostr public key (lowercase hex). Used as the
    /// recipient envelope's `p`-tag receiver AND the kind:10050 lookup
    /// key for the recipient's DM-inbox relays.
    pub recipient_pubkey: String,
    /// Registry-minted action id when this send originates from
    /// `dispatch_action` (`nmp.nip17.send`). The publish engine threads
    /// the id through each kind:1059 `publish_signed_event` call so the
    /// host's spinner resolves on the per-envelope terminal verdict. A
    /// pre-publish early-exit failure records `Failed` directly via
    /// [`ProtocolCommandContext::record_action_failure`]. Non-dispatch
    /// callers (test harnesses) pass `None`.
    pub correlation_id: Option<String>,
}

impl ProtocolCommand for SendGiftWrappedDmCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let SendGiftWrappedDmCommand {
            mut rumor,
            recipient_pubkey,
            correlation_id,
        } = *self;

        // 1. Resolve a `SignerForSeal` for the active account. V-08:
        // `ProtocolCommandContext::signer_for_seal()` covers BOTH local
        // (nsec → blanket impl on `nostr::Keys`, every `SignerOp::Ready`)
        // AND remote-signer (NIP-46 bunker → `RemoteSignerForSeal`
        // adapter, `nip44_encrypt` + `sign_seal` are `Pending` so
        // `gift_wrap_with_signer` runs the chain on a per-invocation
        // driver thread). `None` only when there is no active account
        // OR a remote signer reported a malformed pubkey.
        let Some(signer) = ctx.signer_for_seal() else {
            let reason = "cannot send DM: no active account".to_string();
            ctx.set_last_error_toast(Some(reason.clone()));
            if let Some(id) = correlation_id.clone() {
                ctx.record_action_failure(id, reason);
            }
            return Ok(());
        };

        // 2. D7: re-stamp the rumor timestamp from the kernel clock. The
        // host sends `created_at: 0` as the sentinel; the kernel owns
        // the wall clock.
        if rumor.created_at == 0 {
            rumor.created_at = ctx.now_secs();
        }

        // 3. The signer carries the sender's pubkey; centralising the
        // access here keeps the body D13-clean (no `.secret_key()`).
        let sender = signer.pubkey();
        let sender_hex = sender.to_hex();

        // 4. Convert the substrate rumor → `nostr::UnsignedEvent`. The
        // rumor is NEVER signed; `EventBuilder::build` produces the
        // unsigned form `gift_wrap_with_signer` seals.
        let nostr_rumor = match build_nostr_rumor(&rumor, sender) {
            Ok(r) => r,
            Err(reason) => {
                let toast = format!("cannot send DM: {reason}");
                ctx.set_last_error_toast(Some(toast.clone()));
                if let Some(id) = correlation_id.clone() {
                    ctx.record_action_failure(id, toast);
                }
                return Ok(());
            }
        };

        // 5. Recipient pubkey must parse — a malformed hex pubkey is a
        // caller bug; refuse the send rather than wrap to garbage (D6).
        let recipient = match PublicKey::parse(&recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                let toast = format!("cannot send DM: malformed recipient pubkey: {e}");
                ctx.set_last_error_toast(Some(toast.clone()));
                if let Some(id) = correlation_id.clone() {
                    ctx.record_action_failure(id, toast);
                }
                return Ok(());
            }
        };

        // 6. D10 fail-closed gate — resolve BOTH receivers' kind:10050
        // DM-inbox relays BEFORE constructing any envelope. The gate
        // rejects the missing / empty cases up front, so we never reach
        // `PublishSignedEvent` with an empty relay slice.
        let recipient_relays = match required_dm_relays(ctx, "recipient", &recipient_pubkey) {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(
                    envelope = err.envelope,
                    receiver_pubkey = err.receiver_pubkey.as_str(),
                    "NIP-17 DM send blocked: missing or empty kind:10050 \
                     DM-relay list; refusing Content relay fallback"
                );
                let toast = err.toast();
                ctx.set_last_error_toast(Some(toast.clone()));
                if let Some(id) = correlation_id.clone() {
                    ctx.record_action_failure(id, toast);
                }
                return Ok(());
            }
        };
        let self_relays = match required_dm_relays(ctx, "self-copy", sender_hex.as_str()) {
            Ok(r) => r,
            Err(err) => {
                tracing::warn!(
                    envelope = err.envelope,
                    receiver_pubkey = err.receiver_pubkey.as_str(),
                    "NIP-17 DM send blocked: missing or empty kind:10050 \
                     DM-relay list; refusing Content relay fallback"
                );
                let toast = err.toast();
                ctx.set_last_error_toast(Some(toast.clone()));
                if let Some(id) = correlation_id.clone() {
                    ctx.record_action_failure(id, toast);
                }
                return Ok(());
            }
        };

        // 7. Gift-wrap TWICE — fresh ephemeral outer key per call
        // (NIP-59 unlinkability). Each envelope routes to *its
        // receiver's* kind:10050 list.
        for (label, receiver_pk, relays) in [
            ("recipient", &recipient, recipient_relays),
            ("self-copy", &sender, self_relays),
        ] {
            // NIP-59 randomises the kind:13 + kind:1059 timestamps in a
            // 2-day window so an observer cannot correlate envelope
            // timestamps with the underlying rumor.
            let tweaked = Timestamp::tweaked(RANGE_RANDOM_TIMESTAMP_TWEAK);
            let op = nmp_nip59::gift_wrap_with_signer(&signer, receiver_pk, &nostr_rumor, tweaked);
            let envelope = match op.wait(nmp_nip59::GIFT_WRAP_TOTAL_TIMEOUT) {
                Ok(ev) => ev,
                Err(e) => {
                    let toast = format!("cannot send DM: gift-wrap ({label}) failed: {e}");
                    ctx.set_last_error_toast(Some(toast.clone()));
                    if let Some(id) = correlation_id.clone() {
                        ctx.record_action_failure(id, toast);
                    }
                    return Ok(());
                }
            };

            // The kind:1059 envelope is already signed by its ephemeral
            // key. Route via the signed-event publish path so the kernel
            // forwards it verbatim — re-signing with the account key
            // would destroy the unlinkability gift-wrap exists to
            // provide.
            let raw = nostr_event_to_raw(&envelope);
            ctx.send(ActorCommand::PublishSignedEvent {
                raw,
                target: PublishTarget::Explicit { relays },
                correlation_id: correlation_id.clone(),
            });
        }

        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────────────
// Helpers (private)
// ──────────────────────────────────────────────────────────────────────

/// Receiver-side readiness error for the kind:10050 fail-closed gate.
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

/// D10 fail-closed gate — resolve a receiver's kind:10050 DM-inbox
/// relays or return a [`DmRelayNotReady`] error. By rejecting the `None`
/// branch before any gift-wrap is built, the publish path is never
/// called with an empty relay slice.
fn required_dm_relays(
    ctx: &ProtocolCommandContext<'_>,
    envelope: &'static str,
    receiver_pubkey: &str,
) -> Result<Vec<String>, DmRelayNotReady> {
    ctx.dm_inbox_relays(receiver_pubkey)
        .filter(|relays| !relays.is_empty())
        .ok_or_else(|| DmRelayNotReady {
            envelope,
            receiver_pubkey: receiver_pubkey.to_string(),
        })
}

/// Build a `nostr::UnsignedEvent` (the rumor) from the substrate flat
/// representation. Stops at `EventBuilder::build` — the rumor is
/// unsigned by design (NIP-59 seals it).
fn build_nostr_rumor(
    rumor: &UnsignedEvent,
    pubkey: PublicKey,
) -> Result<nostr::UnsignedEvent, String> {
    if rumor.kind > u32::from(u16::MAX) {
        return Err(format!("invalid kind {}: must be in [0, 65535]", rumor.kind));
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

/// Convert a signed `nostr::Event` (the kind:1059 gift-wrap) to the
/// kernel's flat `RawEvent`. The signature and id are carried through
/// verbatim — the signed-event publish path verifies them and forwards
/// the event unchanged.
fn nostr_event_to_raw(event: &nostr::Event) -> nmp_core::store::RawEvent {
    nmp_core::store::RawEvent {
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
#[path = "dm_send/tests.rs"]
mod tests;
