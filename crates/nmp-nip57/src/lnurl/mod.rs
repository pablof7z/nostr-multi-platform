//! NIP-57 LNURL-pay fetcher — `FetchLnurlInvoiceCommand` `ProtocolCommand`.
//!
//! V-41 / step 4 of the crate-boundary migration
//! (`docs/architecture/crate-boundaries.md` §5). Replaces the old
//! `nmp-core::actor::commands::zap::handle_fetch_lnurl_invoice` +
//! `ActorCommand::FetchLnurlInvoice` variant: the entire LNURL-pay
//! orchestration now lives in `nmp-nip57` and dispatches through the
//! generic [`nmp_core::substrate::ProtocolCommand`] seam.
//!
//! # Wire routing — kind:9734 NEVER reaches relays
//!
//! NIP-57 § "Appendix C": the signed kind:9734 zap request is delivered to
//! the LN provider's LNURL **callback URL** as a `nostr=<urlencoded>` query
//! parameter — NOT broadcast to Nostr relays. The kind:9735 receipt is what
//! relays receive, and the LN provider mints it after the invoice settles.
//! This command therefore emits NO relay-bound frames. On a fetched invoice it
//! dispatches the actor-local NWC pay-invoice command; on LNURL failure or a
//! missing wallet it sends `ShowToast` + `RecordActionFailure` so the host's
//! spinner resolves without scraping toast text.
//!
//! # Surfaces threaded through `ProtocolCommandContext`
//!
//! - `now_secs` — D7 (kernel owns the clock): the executor passes
//!   `created_at = 0`; this command re-stamps before signing.
//! - `recipient_publish_relays` — V-07: resolves the recipient's NIP-65 write
//!   set (via the kernel-side `outbox_router` slot, router cold-start
//!   fallback); `inject_recipient_relays` populates the kind:9734 `relays` tag
//!   (NIP-57 § "Appendix F").
//! - `sign_event_for_account` — V-78: signs the kind:9734 via the unified
//!   `SignEventForAccount` port (ADR-0043), which resolves local-nsec (inline)
//!   and bunker (idle-loop drain) behind one seam; the worker never sees a
//!   `SignerOp`. Only a genuinely absent account fails closed.
//! - `record_action_stage_requested` — tracks the `Requested` stage against the
//!   host `correlation_id` before the worker posts the terminal.
//! - `send` — re-enters the actor loop with the follow-up `ActorCommand`s.
//!
//! # D8 — no blocking on the actor thread
//!
//! The actor thread dispatches the kind:9734 sign through the
//! `SignEventForAccount` port and returns immediately. The continuation (inline
//! for a local nsec, idle-loop drain for a bunker) MUST NOT block — it only
//! SPAWNs the HTTP worker
//! `std::thread` carrying the already-signed kind:9734. The worker thread:
//!
//! 1. Decode the LNURL / lightning-address input → `.well-known/lnurlp` URL
//!    ([`pay::lnurl_to_well_known_url`]).
//! 2. GET that URL → `{ callback, minSendable, maxSendable, nostrPubkey, … }`.
//! 3. GET `{callback}?amount=<msats>&nostr=<urlencoded-signed-9734>` → `{ pr }`.
//! 4. Send follow-up [`ActorCommand`]s: `Protocol(WalletPayInvoiceCommand)` on
//!    a fetched invoice, else `ShowToast` + `RecordActionFailure`.
//!
//! Because the port resolves the sign before the worker spawns, the worker
//! never holds a `SignerOp` and never waits on the signer — it receives the
//! serialized signed kind:9734 JSON ready for the callback's `nostr=` param.
//!
//! # NWC payment handoff
//!
//! After the bolt11 is fetched, the worker uses the per-app
//! `WalletRuntimeHandle` that `ZapAction` captured at composition time
//! (ADR-0052 rung 5.2 — no process-global): with a handle it dispatches
//! `WalletPayInvoiceCommand` carrying the bolt11 + the zap's `correlation_id`;
//! with none it records a "no wallet connected" failure. The kind:23195 NWC
//! response handler then closes the action stage, so the host spinner resolves
//! only on wallet confirmation, not on invoice fetch.

mod metadata;
mod pay;
mod roundtrip;
mod validation;

pub(crate) use validation::{validate_bolt11_amount, validate_description_hash};

use std::str::FromStr;
use std::sync::Arc;

use nmp_core::substrate::{
    build_record_action_failure, PaymentIntent, PaymentPort, ProtocolCommand,
    ProtocolCommandContext, ProtocolCommandError,
};
use nmp_signer_iface::{SignedEvent, UnsignedEvent};
use nmp_core::ActorCommand;
use nmp_kinds::KIND_ZAP_RECEIPT;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

pub use metadata::LnurlInvoice;
pub use pay::{lnurl_to_well_known_url, looks_like_bolt11, url_encode_query, url_to_bech32_lnurl};
use roundtrip::fetch_lnurl_invoice_blocking;

/// The substrate-level [`ProtocolCommand`] that drives the LNURL-pay
/// round-trip. Dispatched as `ActorCommand::Protocol(Box::new(...))` by
/// `ZapAction::execute` (see `crate::action`). When `lnurl_or_address` is
/// `None` the command resolves the recipient's lightning address from the
/// kernel's cached kind:0 profile via the zap-only `ZapProfileLookup`
/// capability (`ctx.zap_profiles().lnurl_for_pubkey(..)`; ADR-0052 §D5).
///
/// The fields mirror the legacy `ActorCommand::FetchLnurlInvoice` variant
/// payload one-for-one — every field is consumed inside [`Self::run`].
#[derive(Debug)]
pub struct FetchLnurlInvoiceCommand {
    /// Unsigned kind:9734 zap request — built by `ZapAction::execute`. The
    /// `created_at` field is the D7 sentinel `0`; this command re-stamps
    /// from the kernel clock before signing.
    pub unsigned: UnsignedEvent,
    /// Recipient's Nostr pubkey (hex). Used as the fallback key for
    /// kernel-side lnurl resolution when `lnurl_or_address` is `None`.
    pub recipient_pubkey: String,
    /// LN-side destination. One of three shapes (LUD-01 / LUD-06 / LUD-16):
    /// a lightning address (`user@domain`), a bech32 LNURL (`lnurl1…`), or
    /// a bare `https://` URL. `None` means the kernel should resolve it from
    /// the recipient's cached kind:0 profile. Decoded by
    /// [`pay::lnurl_to_well_known_url`].
    pub lnurl_or_address: Option<String>,
    /// Zap amount in millisatoshis. Bounded against the LN provider's
    /// `minSendable` / `maxSendable` on leg 1.
    pub amount_msats: u64,
    /// Registry-minted correlation id when this command originates from
    /// `dispatch_action` (`nmp.nip57.zap`). When `Some`, terminal stages
    /// (`Accepted` / `Failed`) are recorded against this id so the host
    /// spinner clears. `None` means a direct caller with no spinner —
    /// only the `ShowToast` follow-up is sent.
    pub correlation_id: Option<String>,
    /// ADR-0052 rung 5.2: the per-app [`PaymentPort`] the zap auto-chain pays
    /// through — captured by `ZapAction` at composition time, not read from a
    /// process-global (so two `NmpApp`s zap independently). `None` when no
    /// wallet was wired; the worker then records a "no wallet connected"
    /// failure. The port is the substrate seam: NIP-57 emits a typed
    /// [`PaymentIntent`] through it and never names a NIP-47 wallet runtime.
    pub payment_port: Option<Arc<dyn PaymentPort>>,
}

impl ProtocolCommand for FetchLnurlInvoiceCommand {
    fn run(
        self: Box<Self>,
        ctx: &mut ProtocolCommandContext<'_>,
    ) -> Result<(), ProtocolCommandError> {
        let Self {
            mut unsigned,
            recipient_pubkey,
            lnurl_or_address,
            amount_msats,
            correlation_id,
            payment_port,
        } = *self;

        // Resolve the LN destination. Shells may omit `lnurl_or_address`
        // (pass `None`) — when they do, the kernel looks up the recipient's
        // lightning address from its cached kind:0 profile. Shells that DO
        // provide an explicit value (e.g. the `:zap` power-user command) use
        // it verbatim to allow overriding the on-profile address.
        let lnurl_or_address = match lnurl_or_address {
            Some(v) if !v.trim().is_empty() => v,
            _ => match ctx.zap_profiles().lnurl_for_pubkey(&recipient_pubkey) {
                Some(v) => v,
                None => {
                    let reason = "this user has no lightning address in their profile";
                    ctx.send(ActorCommand::ShowToast {
                        message: reason.to_string(),
                    });
                    if let Some(cid) = correlation_id {
                        ctx.record_action_failure(cid, reason.to_string());
                    }
                    return Ok(());
                }
            },
        };

        // Track the `Requested` stage against the host's correlation id
        // (mirrors the legacy `FetchLnurlInvoice` dispatch arm). The
        // terminal `Accepted` / `Failed` follow from the worker thread
        // (success) or the sync-path fail branches below (sign/bunker
        // failure).
        if let Some(ref cid) = correlation_id {
            ctx.record_action_stage_requested(cid);
        }

        // D7 — kernel owns the wall clock. Executors pass `0` as a sentinel;
        // re-stamp here. Matches the `PublishUnsignedEventToRelays` arm.
        if unsigned.created_at == 0 {
            unsigned.created_at = ctx.now_secs();
        }

        // V-07 — relay selection is kernel policy, never shell policy. The
        // kind:9734 `relays` tag tells the LN provider where to publish the
        // kind:9735 receipt (NIP-57 § "Appendix F"): the correct answer is
        // the RECIPIENT's NIP-65 write list, which only the kernel knows.
        inject_recipient_relays(ctx, &mut unsigned);

        // D6 fail-closed: abort the zap if lnurl tag encoding fails (most
        // providers including Primal won't mint a receipt without it).
        if let Err(reason) = inject_lnurl_tag(&lnurl_or_address, &mut unsigned) {
            ctx.send(ActorCommand::ShowToast {
                message: format!("Zap failed: {reason}"),
            });
            if let Some(cid) = correlation_id {
                ctx.record_action_failure(cid, reason);
            }
            return Ok(());
        }

        // V-78 reconcile — sign the kind:9734 through the unified
        // `ActorCommand::SignEventForAccount` port (ADR-0043 Decision 2). The
        // active account signs, so `signer_pubkey = None`. The actor's dispatch
        // arm resolves BOTH backends behind the port — a local nsec inline
        // (`SignerOp::Ready`); a NIP-46 bunker parked and resolved from the
        // idle-loop drain (`SignerOp::Pending`) — and invokes `continuation`
        // with the resolved `SignedEvent` (or an `Err` string). This worker
        // never sees a `SignerOp` and never branches on backend; one seam, both
        // signer kinds. This closes V-78 onto the same port the Blossom
        // sign-and-return path uses (D13 — only a `SignedEvent` ever crosses).
        //
        // [`ProtocolCommandContext::command_sender_clone`] hands us an owned
        // `Sender<ActorCommand>` (a cheap atomic ref-count bump) for the
        // continuation + worker to post follow-up commands (`ShowToast`,
        // `Protocol(WalletPayInvoiceCommand)`, `RecordActionFailure`) back into
        // the actor loop after the dispatch arm (and its
        // `ProtocolCommandContext`) have returned.
        let worker_tx = ctx.command_sender_clone();
        ctx.sign_event_for_account(unsigned, None, move |outcome| {
            // Runs on the actor thread (inline for local, idle-drain for
            // bunker). D8: it MUST NOT block — its only job is to serialize the
            // signed kind:9734 and SPAWN the HTTP worker (or, on a sign error /
            // absent account, post the fail-closed terminal).
            let signed = match outcome {
                Ok(signed) => signed,
                Err(reason) => {
                    // Sign failed (genuinely-no-active-account, broker
                    // rejection, or a malformed signer response) — fail closed
                    // with a D6 toast + `RecordActionFailure`.
                    let msg = format!("Zap failed: {reason}");
                    let _ = worker_tx.send(ActorCommand::ShowToast { message: msg });
                    if let Some(cid) = correlation_id {
                        let _ = worker_tx.send(build_record_action_failure(cid, reason));
                    }
                    return;
                }
            };
            let zap_request_id = signed.id.clone();
            let signed_json = match signed_event_to_nostr_json(&signed) {
                Ok(json) => json,
                Err(reason) => {
                    let msg = format!("Zap failed: {reason}");
                    let _ = worker_tx.send(ActorCommand::ShowToast { message: msg });
                    if let Some(cid) = correlation_id {
                        let _ = worker_tx.send(build_record_action_failure(cid, reason));
                    }
                    return;
                }
            };
            spawn_lnurl_worker(
                worker_tx,
                lnurl_or_address,
                amount_msats,
                signed_json,
                zap_request_id,
                correlation_id,
                payment_port,
            );
        });

        Ok(())
    }
}

/// Spawn the off-actor HTTP worker that runs the two-leg LNURL-pay round-trip
/// for an already-signed kind:9734 (serialized as `signed_json`). `std::thread`
/// (not tokio); the worker owns clones of everything it needs and never touches
/// the actor's mutable state. D8: zero blocking on the actor thread —
/// this function only *spawns*; the blocking HTTP work happens on the new
/// thread.
///
/// On a fetched invoice the worker hands the bolt11 to the NWC wallet (the
/// kind:23195 response closes the `nmp.nip57.zap` stage on confirmation); on a
/// missing wallet or LNURL failure it posts `ShowToast` + `RecordActionFailure`
/// so the host spinner resolves with a clear reason.
fn spawn_lnurl_worker(
    worker_tx: nmp_core::CommandSender,
    lnurl_or_address: String,
    amount_msats: u64,
    signed_json: String,
    zap_request_id: String,
    correlation_id: Option<String>,
    payment_port: Option<Arc<dyn PaymentPort>>,
) {
    std::thread::spawn(move || {
        match fetch_lnurl_invoice_blocking(&lnurl_or_address, amount_msats, &signed_json) {
            // ADR-0052 rung 5.2: pay through the per-app `PaymentPort`
            // `ZapAction` captured (no process-global). `Some` → emit the
            // port's pay-invoice command (its own no-connection branch reports a
            // disconnected wallet); `None` → no wallet wired, record the
            // "no wallet connected" failure so the host spinner resolves.
            Ok(invoice) => match payment_port {
                Some(port) => {
                    if let Err(reason) = crate::pending::active_pending_zap_registry()
                        .remember_expected_provider(&zap_request_id, &invoice.provider_pubkey)
                    {
                        let _ = worker_tx.send(ActorCommand::ShowToast {
                            message: format!("Zap failed: {reason}"),
                        });
                        if let Some(cid) = correlation_id {
                            let _ = worker_tx.send(build_record_action_failure(cid, reason));
                        }
                        return;
                    }
                    let _ = worker_tx.send(port.pay_invoice(PaymentIntent {
                        bolt11: invoice.bolt11,
                        amount_msats: None, // bolt11 carries the amount
                        correlation_id,
                    }));
                }
                None => {
                    let reason =
                        "zap: no wallet connected — connect a NWC wallet first".to_string();
                    let _ = worker_tx.send(ActorCommand::ShowToast {
                        message: reason.clone(),
                    });
                    if let Some(cid) = correlation_id {
                        let _ = worker_tx.send(build_record_action_failure(cid, reason));
                    }
                }
            },
            Err(reason) => {
                let _ = worker_tx.send(ActorCommand::ShowToast {
                    message: format!("Zap failed: {reason}"),
                });
                if let Some(cid) = correlation_id {
                    let _ = worker_tx.send(build_record_action_failure(cid, reason));
                }
            }
        }
    });
}

/// V-07 — inject the kind:9734 `relays` tag from the recipient's NIP-65
/// (kind:10002) write list (or the router's cold-start fallback) when the
/// caller produced no filled `relays` row.
///
/// Routes through [`ProtocolCommandContext::recipient_publish_relays`] —
/// the substrate seam the kernel-side adapter wires through its injected
/// `outbox_router` slot (lane 1 = recipient's NIP-65 write set, lane 7 =
/// AppRelay cold-start fallback). NIP-57 § "Appendix F" — the LN provider
/// publishes the kind:9735 zap receipt to the URLs in this tag.
///
/// Algorithm:
/// 1. If a non-empty `relays` row is already present, leave it. A caller
///    that explicitly picked relays overrides this injection.
/// 2. Find the first `p` tag (the zap recipient — NIP-57 § "Appendix A").
///    With no `p` tag we cannot ask the router for anything recipient-
///    specific; the router's lane-7 cold-start seed is the safe fallback
///    (a synthetic publish of kind:9735 from an empty pubkey resolves
///    via the AppRelay seed). With a `p` tag, route via the kind:9735
///    publish-direction (the kind the LN provider will mint).
/// 3. Replace any malformed bare `["relays"]` row (no URLs) with the
///    resolved row.
pub(crate) fn inject_recipient_relays(
    ctx: &ProtocolCommandContext<'_>,
    unsigned: &mut UnsignedEvent,
) {
    if has_filled_relays_row(&unsigned.tags) {
        return;
    }
    let recipient = first_p_tag(&unsigned.tags).unwrap_or_default();
    let urls = ctx.recipient_publish_relays(&recipient, KIND_ZAP_RECEIPT);
    // Drop any pre-existing bare `["relays"]` row (no URLs) — it is
    // malformed per NIP-57 § "Appendix A" and would otherwise survive
    // alongside the injected row.
    unsigned
        .tags
        .retain(|t| !(t.first().is_some_and(|k| k == "relays") && t.len() <= 1));
    let mut row = vec!["relays".to_string()];
    row.extend(urls);
    unsigned.tags.push(row);
}

/// NIP-57 SHOULD — inject the `lnurl` tag (bech32 well-known URL) when absent.
///
/// Returns `Err(reason)` if the LNURL cannot be resolved or encoded so the
/// caller can abort the zap (D6: paid invoice without the tag loses the
/// kind:9735 receipt from most providers). Pure computation; safe on actor
/// thread.
pub(crate) fn inject_lnurl_tag(
    lnurl_or_address: &str,
    unsigned: &mut UnsignedEvent,
) -> Result<(), String> {
    if unsigned
        .tags
        .iter()
        .any(|t| t.first().is_some_and(|k| k == "lnurl") && t.len() > 1)
    {
        return Ok(());
    }
    let well_known = lnurl_to_well_known_url(lnurl_or_address)
        .map_err(|e| format!("zap: lnurl resolve failed ({lnurl_or_address}): {e}"))?;
    let bech32_lnurl = pay::url_to_bech32_lnurl(&well_known)
        .map_err(|e| format!("zap: lnurl bech32 encode failed ({well_known}): {e}"))?;
    unsigned.tags.push(vec!["lnurl".to_string(), bech32_lnurl]);
    Ok(())
}

fn has_filled_relays_row(tags: &[Vec<String>]) -> bool {
    tags.iter()
        .any(|t| t.first().is_some_and(|k| k == "relays") && t.len() > 1)
}

fn first_p_tag(tags: &[Vec<String>]) -> Option<String> {
    tags.iter()
        .find(|t| t.first().is_some_and(|k| k == "p"))
        .and_then(|t| t.get(1).cloned())
}

/// Sign `unsigned` with `keys` and emit the flat NIP-01 JSON object the
/// LNURL callback expects in its `nostr=<urlencoded>` parameter.
///
/// Mirrors the wallet-runtime `sign_nwc_request` precedent — build a
/// `nostr::Event` via `EventBuilder`, then re-serialize to JSON. The reseat
/// step is the bridge between the substrate's typed `UnsignedEvent` shape
/// (kind / tags / content / `created_at`) and the nostr crate's signer API.
pub fn sign_zap_request(keys: &Keys, unsigned: &UnsignedEvent) -> Result<String, String> {
    let kind = Kind::from_u16(
        u16::try_from(unsigned.kind).map_err(|e| format!("zap kind out of range: {e}"))?,
    );
    let tags: Vec<Tag> = unsigned
        .tags
        .iter()
        .map(|t| {
            Tag::parse(
                t.iter()
                    .map(std::string::String::as_str)
                    .collect::<Vec<_>>(),
            )
            .map_err(|e| format!("tag parse: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let event = EventBuilder::new(kind, unsigned.content.clone())
        .tags(tags)
        .custom_created_at(Timestamp::from(unsigned.created_at))
        .sign_with_keys(keys)
        .map_err(|e| format!("sign: {e}"))?;
    serde_json::to_string(&event).map_err(|e| format!("serialize signed zap request: {e}"))
}

/// V-78 — re-serialize a substrate [`SignedEvent`] into the flat NIP-01 JSON
/// object the LNURL callback expects in its `nostr=<urlencoded>` parameter.
///
/// The substrate [`SignedEvent`] is a nested `{ id, sig, unsigned: { … } }`
/// shape; the LN provider needs the flat `{ id, pubkey, created_at, kind,
/// tags, content, sig }` NIP-01 wire form. We reconstruct a `nostr::Event`
/// from the signed fields and serialize it through the SAME `serde` path
/// [`sign_zap_request`] uses — so a bunker-signed zap request is byte-for-byte
/// the wire shape a local-nsec zap produced, the moment the broker returns the
/// `id`/`sig`. No re-signing: the kind:9734 signature minted by the active
/// account (local OR bunker) is carried through verbatim.
pub fn signed_event_to_nostr_json(signed: &SignedEvent) -> Result<String, String> {
    let SignedEvent { id, sig, unsigned } = signed;

    let event_id = nostr::EventId::from_hex(id).map_err(|e| format!("zap event id: {e}"))?;
    let pubkey =
        nostr::PublicKey::from_hex(&unsigned.pubkey).map_err(|e| format!("zap pubkey: {e}"))?;
    let signature = nostr::secp256k1::schnorr::Signature::from_str(sig)
        .map_err(|e| format!("zap signature: {e}"))?;
    let kind = Kind::from_u16(
        u16::try_from(unsigned.kind).map_err(|e| format!("zap kind out of range: {e}"))?,
    );
    let tags: Vec<Tag> = unsigned
        .tags
        .iter()
        .map(|t| {
            Tag::parse(t.iter().map(String::as_str).collect::<Vec<_>>())
                .map_err(|e| format!("tag parse: {e}"))
        })
        .collect::<Result<Vec<_>, _>>()?;

    let event = nostr::Event::new(
        event_id,
        pubkey,
        Timestamp::from(unsigned.created_at),
        kind,
        tags,
        unsigned.content.clone(),
        signature,
    );
    serde_json::to_string(&event).map_err(|e| format!("serialize signed zap request: {e}"))
}

#[cfg(test)]
mod tests;
