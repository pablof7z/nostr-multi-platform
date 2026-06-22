//! `SendGiftWrappedDmCommand` — the NIP-17 gift-wrapped DM send handler.
//!
//! # ADR-0050 §D5 — a continuation chain through the signer port
//!
//! The send is a chain of port requests composed via the cloned actor command
//! sender ([`ProtocolCommandContext::command_sender_clone`]). There is no
//! `SignerForSeal` trait, no per-invocation driver thread, no per-DM worker
//! thread, and no inline local-keys bypass — local and bunker accounts ride the
//! SAME mechanism (one mechanism per concern; the mailbox hops are the
//! honestly-stated cost). Each envelope is sealed + wrapped by:
//!
//! 1. `Nip44EncryptForAccount(receiver, rumor_json)` → the seal-content
//!    ciphertext (local: `nostr::nips::nip44` inside the runtime; bunker: the
//!    handle's `nip44_encrypt`, parked + drained — invisible to this code).
//! 2. continuation builds the kind:13 seal `UnsignedEvent` (pure, on-actor, via
//!    `nmp_nip59::build_seal_unsigned`) and sends `SignEventForAccount(seal)`.
//! 3. continuation assembles the kind:1059 wrap locally (fresh ephemeral key,
//!    in-process via `nmp_nip59::wrap_signed_seal` — NIP-59 unlinkability
//!    untouched) and sends `PublishSignedEvent`.
//!
//! ## Envelope order — sequential and failure-preserving
//!
//! The recipient chain runs FIRST; its success continuation launches the
//! self-copy chain. The action verdict is the recipient envelope's
//! (single-terminal contract): only the recipient envelope carries the
//! `correlation_id`. A self-copy failure surfaces a D6 toast only — never a
//! `RecordActionFailure` (the recipient already got the message, so the action
//! promise is satisfied). This preserves today's semantics exactly.
//!
//! ## Account pinning (§D5)
//!
//! A chain of port requests would re-resolve "active" at every step, so a
//! mid-chain account switch could sign the seal with a different key than the
//! one that encrypted it. The chain therefore resolves the active account's
//! pubkey ONCE, up front (`ctx.active_account_pubkey()`), and every port step
//! passes `signer_pubkey: Some(hex)` — never `None`. Oracle: a DM-send chain
//! whose active account switches mid-flight signs the seal with the originating
//! account.
//!
//! ## Wire semantics (unchanged from the pre-§D5 code)
//!
//! * Two kind:1059 envelopes — one to the recipient, one self-copy (so sent
//!   messages stay readable across clients). Each routes to *its receiver's*
//!   kind:10050 DM-inbox relays (NIP-17 § 2); missing/empty lists fail closed
//!   with a D6 toast (never a generic Content-relay fallback).
//! * The rumor's `created_at` is re-stamped from the kernel clock (D7).
//! * Per-step deadlines are the port's (§D4 op_timeout budgets) — the old
//!   dual `DRIVER_STEP_TIMEOUT` / `GIFT_WRAP_TOTAL_TIMEOUT` constants are gone.
//!
//! # D doctrine
//!
//! * **D0** — the substrate (`nmp-core`) holds no NIP-17 nouns; this crate owns
//!   the kind:1059 wire shape, the kind:10050 cache, and the gift-wrap chain.
//! * **D6** — every failure path sets a toast AND (recipient envelope only)
//!   records an action failure; no silent drops.
//! * **D7** — the kernel-owned wall clock stamps `created_at`.
//! * **D8** — zero blocking on the actor thread: each continuation only enqueues
//!   the next port command; the seal/wrap steps are pure CPU work.
//! * **D10** — the publish path uses `PublishTarget::Explicit { relays }` with a
//!   non-empty slice (the `required_dm_relays` gate rejects empty/missing first).
//! * **D13** — no raw `Keys` cross this crate; the seal is signed through the
//!   port and only a `SignedEvent` / ciphertext is ever observed.

use nmp_core::substrate::{ProtocolCommand, ProtocolCommandContext, ProtocolCommandError};
use nmp_signer_iface::UnsignedEvent;
use nostr::{EventBuilder, JsonUtil, Kind, PublicKey, Tag, Timestamp};

/// NIP-17 § 2 gift-wrap publish — the [`ProtocolCommand`] equivalent of the
/// legacy `ActorCommand::SendGiftWrappedDm` variant.
///
/// Construct one of these in the action executor (`SendDmAction::execute`) and
/// dispatch via `ActorCommand::Protocol(Box::new(cmd))`. The actor runs `run`
/// on the actor thread; it pins the active account, validates inputs, resolves
/// both receivers' DM-inbox relays, then launches the recipient gift-wrap chain
/// (whose success continuation launches the self-copy chain) — all through the
/// signer port (ADR-0050 §D5).
#[derive(Clone, Debug)]
pub struct SendGiftWrappedDmCommand {
    /// The kind:14 chat-message rumor (unsigned) the host built via
    /// [`crate::build_dm_rumor`]. `created_at == 0` is the kernel-stamp
    /// sentinel; the executor re-stamps from `ctx.now_secs()`.
    pub rumor: UnsignedEvent,
    /// Recipient's Nostr public key (lowercase hex). Used as the recipient
    /// envelope's `p`-tag receiver AND the kind:10050 lookup key for the
    /// recipient's DM-inbox relays.
    pub recipient_pubkey: String,
    /// Registry-minted action id when this send originates from `dispatch_action`
    /// (`nmp.nip17.send`). Threaded through the recipient envelope's
    /// `PublishSignedEvent` so the host spinner resolves on that terminal
    /// verdict. A pre-publish early-exit failure records `Failed` directly.
    /// Non-dispatch callers (tests) pass `None`.
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

        // 1. §D5 account pinning — resolve the active account's pubkey ONCE.
        // Every subsequent port step passes `signer_pubkey: Some(hex)` so a
        // mid-flight account switch cannot sign the seal with a different key
        // than the one that encrypted it. `None` only when no account is active.
        let Some(signer_hex) = ctx.active_account_pubkey() else {
            fail_pre_publish(ctx, &correlation_id, "no active account".to_string());
            return Ok(());
        };
        let sender = match PublicKey::parse(&signer_hex) {
            Ok(pk) => pk,
            Err(e) => {
                fail_pre_publish(
                    ctx,
                    &correlation_id,
                    format!("active account pubkey is malformed: {e}"),
                );
                return Ok(());
            }
        };

        // 2. D7: re-stamp the rumor timestamp from the kernel clock. The host
        // sends `created_at: 0` as the sentinel; the kernel owns the wall clock.
        if rumor.created_at == 0 {
            rumor.created_at = ctx.now_secs();
        }

        // 3. Convert the substrate rumor → `nostr::UnsignedEvent` (NEVER signed;
        // NIP-59 seals it) and serialize once — the seal-content plaintext.
        let nostr_rumor = match build_nostr_rumor(&rumor, sender) {
            Ok(r) => r,
            Err(reason) => {
                fail_pre_publish(ctx, &correlation_id, reason);
                return Ok(());
            }
        };
        let rumor_json = nostr_rumor.as_json();

        // 4. Recipient pubkey must parse.
        let recipient = match PublicKey::parse(&recipient_pubkey) {
            Ok(pk) => pk,
            Err(e) => {
                fail_pre_publish(
                    ctx,
                    &correlation_id,
                    format!("malformed recipient pubkey: {e}"),
                );
                return Ok(());
            }
        };

        // 5. D10 fail-closed gate — resolve BOTH receivers' kind:10050 DM-inbox
        // relays BEFORE launching any chain, so a chain never reaches
        // `PublishSignedEvent` with an empty relay slice.
        let sender_hex = signer_hex.clone();
        let recipient_relays = match required_dm_relays(ctx, "recipient", &recipient_pubkey) {
            Ok(r) => r,
            Err(err) => {
                err.warn();
                fail_pre_publish(ctx, &correlation_id, err.reason());
                return Ok(());
            }
        };
        let self_relays = match required_dm_relays(ctx, "self-copy", sender_hex.as_str()) {
            Ok(r) => r,
            Err(err) => {
                err.warn();
                fail_pre_publish(ctx, &correlation_id, err.reason());
                return Ok(());
            }
        };

        // 6. Launch the RECIPIENT chain. Its success continuation launches the
        // SELF-COPY chain (sequential + failure-preserving). All envelope data
        // is owned; nothing references `ctx` past this point (D8).
        let worker_tx = ctx.command_sender_clone();
        let envelope = EnvelopeChain {
            label: "recipient",
            signer_hex: signer_hex.clone(),
            sender,
            receiver: recipient,
            rumor_json: rumor_json.clone(),
            relays: recipient_relays,
            // The recipient envelope carries the action correlation_id — its
            // relay ack produces the ONE terminal verdict the host waits for.
            correlation_id: correlation_id.clone(),
        };
        // On recipient success, launch the self-copy chain.
        let self_copy = SelfCopyLaunch {
            signer_hex,
            sender,
            rumor_json,
            relays: self_relays,
        };
        envelope.launch(worker_tx, Some(self_copy));

        Ok(())
    }
}

// ──────────────────────────────────────────────────────────────────────
// Failure surface (D6) — pre-chain
// ──────────────────────────────────────────────────────────────────────

/// Pre-publish (on-actor, still holding `ctx`) failure: set the toast AND record
/// the action failure when a `correlation_id` was supplied. Single-terminal:
/// the action verdict is the recipient envelope's. (The in-continuation failure
/// surface lives in [`chain::report_envelope_failure`].)
fn fail_pre_publish(
    ctx: &ProtocolCommandContext<'_>,
    correlation_id: &Option<String>,
    reason: String,
) {
    // issue #1682 — emit a structured token (machine code + English fallback)
    // so the shell renders localized prose; the upstream `reason` is the raw
    // diagnostic detail. The action-failure verdict still carries the fallback
    // prose (the action_lifecycle `reason` channel is English-prose today).
    let token = nmp_core::ui_token::UiToken::error(
        crate::ui_codes::DM_SEND_FAILED,
        format!("cannot send DM: {reason}"),
    )
    .with_detail(reason);
    ctx.set_last_error_token(&token);
    if let Some(id) = correlation_id.clone() {
        ctx.record_action_failure(id, token.fallback_prose().to_string());
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
    fn reason(&self) -> String {
        format!("{} has no kind:10050 DM relay list yet", self.envelope)
    }

    fn warn(&self) {
        tracing::warn!(
            envelope = self.envelope,
            receiver_pubkey = self.receiver_pubkey.as_str(),
            "NIP-17 DM send blocked: missing or empty kind:10050 DM-relay list; \
             refusing Content relay fallback"
        );
    }
}

/// D10 fail-closed gate — resolve a receiver's kind:10050 DM-inbox relays or
/// return a [`DmRelayNotReady`] error. By rejecting the `None` / empty branch
/// before any chain launches, the publish path is never called with an empty
/// relay slice.
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
/// representation. Stops at `EventBuilder::build` — the rumor is unsigned by
/// design (NIP-59 seals it).
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

#[path = "dm_send/chain.rs"]
mod chain;
use chain::{EnvelopeChain, SelfCopyLaunch};

#[cfg(test)]
#[path = "dm_send/tests.rs"]
mod tests;
