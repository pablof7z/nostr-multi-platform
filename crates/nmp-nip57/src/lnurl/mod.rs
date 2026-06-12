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
//! - [`ProtocolCommandContext::now_secs`] — D7 — kernel owns the wall clock.
//!   The executor passes `created_at = 0` as a sentinel; this command
//!   re-stamps before signing (mirrors the
//!   `PublishUnsignedEventToRelays` precedent).
//! - [`ProtocolCommandContext::recipient_publish_relays`] — V-07: the
//!   substrate seam (Debt-C-follow-up) the kernel-side adapter wires
//!   through its injected `outbox_router` slot to resolve the recipient's
//!   NIP-65 write set (with router lane-7 / lane-6 cold-start fallback).
//!   `inject_recipient_relays` consumes this to populate the kind:9734
//!   `relays` tag so the LN provider knows where to publish the kind:9735
//!   zap receipt (NIP-57 § "Appendix F").
//! - [`ProtocolCommandContext::sign_event_for_account`] — V-78 reconcile: signs
//!   the kind:9734 with the active account through the unified
//!   `ActorCommand::SignEventForAccount` port (ADR-0043 Decision 2). The actor
//!   dispatch arm resolves BOTH backends behind the port — a local nsec signs
//!   inline (`SignerOp::Ready`); a NIP-46 bunker parks (`SignerOp::Pending`) and
//!   the idle-loop drain resolves it — then invokes the continuation with the
//!   resolved `SignedEvent` (or an error string). This command's worker never
//!   sees a `SignerOp` and never branches on backend; only a genuinely absent
//!   account surfaces an `Err` to the continuation, which fails closed.
//! - [`ProtocolCommandContext::record_action_stage_requested`] — track the
//!   `Requested` stage against the host's `correlation_id` (when supplied)
//!   so the stage observer sees the transition before the worker thread
//!   posts the terminal.
//! - [`ProtocolCommandContext::send`] — re-enter the actor loop with the
//!   follow-up `ActorCommand`s (`Protocol(WalletPayInvoiceCommand)`,
//!   `ShowToast`, `RecordActionFailure`).
//!
//! # D8 — no blocking on the actor thread
//!
//! The actor thread DISPATCHES the kind:9734 sign through the
//! `SignEventForAccount` port and returns immediately. The continuation runs
//! on the actor thread too (inline for a local nsec, from the idle-loop drain
//! for a bunker) and MUST NOT block — its sole job is to SPAWN the HTTP worker
//! `std::thread` carrying the already-signed kind:9734. The worker thread:
//!
//! 1. Decodes the LNURL (bech32) or lightning-address (`user@domain`) input
//!    into a `https://…/.well-known/lnurlp/<user>` URL via
//!    [`pay::lnurl_to_well_known_url`].
//! 2. HTTP GET that URL → parse `{ "callback": "…", "minSendable": …,
//!    "maxSendable": …, "allowsNostr": …, "nostrPubkey": … }`.
//! 3. HTTP GET `{callback}?amount=<msats>&nostr=<urlencoded-signed-9734>` →
//!    parse `{ "pr": "lnbc…" }`.
//! 4. Send the follow-up [`ActorCommand`]s back through the cloned
//!    [`Sender<ActorCommand>`]: `Protocol(WalletPayInvoiceCommand)` on a
//!    fetched invoice, or `ShowToast` + `RecordActionFailure` for LNURL
//!    failures and missing-wallet failures.
//!
//! Because the port resolves the sign before the worker spawns, the worker
//! never holds a `SignerOp` and never waits on the signer — it receives the
//! serialized signed kind:9734 JSON ready for the callback's `nostr=` param.
//!
//! # NWC payment handoff
//!
//! After the bolt11 is fetched, the worker checks `nmp_nip47::active_wallet_runtime()`.
//! If a wallet runtime is installed, it dispatches `WalletPayInvoiceCommand`
//! carrying the bolt11 and the zap's `correlation_id`. The kind:23195 NWC
//! response handler then closes the action stage — success or failure — so
//! the host's spinner resolves only when the payment is confirmed by the
//! wallet, not merely when the invoice is fetched. If no wallet is installed
//! the action records a `Failed` terminal immediately with a descriptive reason.

mod pay;

use std::io::Read;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use nmp_core::substrate::{
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError, SignedEvent, UnsignedEvent,
};
use nmp_core::ActorCommand;
use nmp_kinds::KIND_ZAP_RECEIPT;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

pub use pay::{looks_like_bolt11, lnurl_to_well_known_url, url_encode_query, url_to_bech32_lnurl};

/// LNURL-pay total budget for the two-leg HTTP round-trip
/// (well-known fetch + callback fetch). Conservative — keeps a stuck
/// LN provider from accumulating worker threads even though each thread
/// is independent of the actor loop.
const LNURL_HTTP_TIMEOUT_SECS: u64 = 10;

/// Maximum response body the worker will accept from either LNURL hop.
/// LNURL-pay responses are tiny JSON objects (a few hundred bytes); 64 KiB
/// is several orders of magnitude over the spec. The cap exists to make a
/// hostile / runaway endpoint a bounded error, not an OOM event.
const LNURL_MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// The substrate-level [`ProtocolCommand`] that drives the LNURL-pay
/// round-trip. Dispatched as `ActorCommand::Protocol(Box::new(...))` by
/// `ZapAction::execute` (see `crate::action`). When `lnurl_or_address` is
/// `None` the command resolves the recipient's lightning address from the
/// kernel's cached kind:0 profile via
/// [`ProtocolCommandContext::lnurl_for_pubkey`].
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
        } = *self;

        // Resolve the LN destination. Shells may omit `lnurl_or_address`
        // (pass `None`) — when they do, the kernel looks up the recipient's
        // lightning address from its cached kind:0 profile. Shells that DO
        // provide an explicit value (e.g. the `:zap` power-user command) use
        // it verbatim to allow overriding the on-profile address.
        let lnurl_or_address = match lnurl_or_address {
            Some(v) if !v.trim().is_empty() => v,
            _ => match ctx.lnurl_for_pubkey(&recipient_pubkey) {
                Some(v) => v,
                None => {
                    let reason = "this user has no lightning address in their profile";
                    ctx.send(ActorCommand::ShowToast { message: reason.to_string() });
                    if let Some(cid) = correlation_id {
                        ctx.send(ActorCommand::RecordActionFailure {
                            correlation_id: cid,
                            reason: reason.to_string(),
                        });
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

        // NIP-57 SHOULD — the `lnurl` tag carries the bech32-encoded
        // well-known URL so the LN provider (e.g. Primal) can associate
        // the payment with the right Nostr account and mint the kind:9735
        // receipt. Pure computation — no I/O, safe on the actor thread.
        inject_lnurl_tag(&lnurl_or_address, &mut unsigned);

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
            let signed_json = match outcome.and_then(|signed| signed_event_to_nostr_json(&signed)) {
                Ok(json) => json,
                Err(reason) => {
                    // Sign failed (genuinely-no-active-account, broker
                    // rejection, or a malformed signer response) — fail closed
                    // with a D6 toast + `RecordActionFailure`.
                    let msg = format!("Zap failed: {reason}");
                    let _ = worker_tx.send(ActorCommand::ShowToast { message: msg });
                    if let Some(cid) = correlation_id {
                        let _ = worker_tx.send(ActorCommand::RecordActionFailure {
                            correlation_id: cid,
                            reason,
                        });
                    }
                    return;
                }
            };
            spawn_lnurl_worker(worker_tx, lnurl_or_address, amount_msats, signed_json, correlation_id);
        });

        Ok(())
    }
}

/// Spawn the off-actor HTTP worker that runs the two-leg LNURL-pay round-trip
/// for an already-signed kind:9734 (serialized as `signed_json`). `std::thread`
/// (not tokio) — `nmp-nip57` has no async runtime and the actor itself is
/// `std::thread`-based. The worker owns clones of everything it needs; nothing
/// references the actor's mutable state. D8: zero blocking on the actor thread —
/// this function only *spawns*; the blocking HTTP work happens on the new
/// thread.
///
/// On a fetched invoice the worker hands the bolt11 to the NWC wallet so the
/// kind:23195 response handler closes the `nmp.nip57.zap` action stage on wallet
/// confirmation; if no wallet runtime is installed (or the LNURL legs fail) it
/// posts a `ShowToast` + `RecordActionFailure` so the host spinner resolves with
/// a clear reason instead of hanging.
fn spawn_lnurl_worker(
    worker_tx: std::sync::mpsc::Sender<ActorCommand>,
    lnurl_or_address: String,
    amount_msats: u64,
    signed_json: String,
    correlation_id: Option<String>,
) {
    std::thread::spawn(move || {
        match fetch_lnurl_invoice_blocking(&lnurl_or_address, amount_msats, &signed_json) {
            Ok(bolt11) => match nmp_nip47::active_wallet_runtime() {
                Some(runtime) => {
                    let _ = worker_tx.send(ActorCommand::Protocol(Box::new(
                        nmp_nip47::WalletPayInvoiceCommand {
                            bolt11,
                            amount_msats: None, // bolt11 carries the amount
                            correlation_id,
                            runtime,
                        },
                    )));
                }
                None => {
                    let reason =
                        "zap: no wallet connected — connect a NWC wallet first".to_string();
                    let _ = worker_tx.send(ActorCommand::ShowToast {
                        message: reason.clone(),
                    });
                    if let Some(cid) = correlation_id {
                        let _ = worker_tx.send(ActorCommand::RecordActionFailure {
                            correlation_id: cid,
                            reason,
                        });
                    }
                }
            },
            Err(reason) => {
                let _ = worker_tx.send(ActorCommand::ShowToast {
                    message: format!("Zap failed: {reason}"),
                });
                if let Some(cid) = correlation_id {
                    let _ = worker_tx.send(ActorCommand::RecordActionFailure {
                        correlation_id: cid,
                        reason,
                    });
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

/// NIP-57 SHOULD — inject the `lnurl` tag (bech32-encoded well-known URL)
/// into the kind:9734 zap request when no `lnurl` tag is already present.
///
/// Primal and most LNURL servers require this tag to associate the LN
/// payment with the right Nostr account and mint the kind:9735 zap receipt.
/// Without it the bolt11 is paid but no receipt is published.
///
/// Pure computation (no I/O): safe to call on the actor thread.
pub(crate) fn inject_lnurl_tag(lnurl_or_address: &str, unsigned: &mut UnsignedEvent) {
    // Skip if the caller already provided an lnurl tag (non-empty value).
    if unsigned
        .tags
        .iter()
        .any(|t| t.first().is_some_and(|k| k == "lnurl") && t.len() > 1)
    {
        return;
    }
    // Resolve to the https well-known URL, then encode as bech32.
    // Errors are silently ignored — the zap proceeds without the tag
    // (degraded: receipt may not be published by some providers).
    let Ok(well_known) = lnurl_to_well_known_url(lnurl_or_address) else {
        return;
    };
    let Ok(bech32_lnurl) = pay::url_to_bech32_lnurl(&well_known) else {
        return;
    };
    unsigned.tags.push(vec!["lnurl".to_string(), bech32_lnurl]);
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
            Tag::parse(t.iter().map(std::string::String::as_str).collect::<Vec<_>>())
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
    let pubkey = nostr::PublicKey::from_hex(&unsigned.pubkey)
        .map_err(|e| format!("zap pubkey: {e}"))?;
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

/// Two-leg LNURL-pay HTTP round-trip. Runs on the spawned worker thread —
/// blocking I/O is acceptable here precisely because we are NOT on the
/// actor thread. Also usable from standalone tools (see `fetch_bolt11_for_zap`).
pub(crate) fn fetch_lnurl_invoice_blocking(
    lnurl_or_address: &str,
    amount_msats: u64,
    signed_zap_request_json: &str,
) -> Result<String, String> {
    let well_known_url = lnurl_to_well_known_url(lnurl_or_address)?;

    // Leg 1: well-known fetch. Pull the LNURL-pay metadata. We care about
    // `callback`, `minSendable`, `maxSendable`, and `allowsNostr` (must be
    // truthy for NIP-57).
    let well_known = http_get_json(&well_known_url)?;
    let callback = well_known
        .get("callback")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "LNURL well-known response missing `callback` URL — receiver is not LNURL-pay enabled".to_string()
        })?;
    if let Some(min) = well_known
        .get("minSendable")
        .and_then(serde_json::Value::as_u64)
    {
        if amount_msats < min {
            return Err(format!(
                "amount {amount_msats} msats below receiver minSendable {min} msats"
            ));
        }
    }
    if let Some(max) = well_known
        .get("maxSendable")
        .and_then(serde_json::Value::as_u64)
    {
        if amount_msats > max {
            return Err(format!(
                "amount {amount_msats} msats above receiver maxSendable {max} msats"
            ));
        }
    }
    let allows_nostr = well_known
        .get("allowsNostr")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    if !allows_nostr {
        return Err(
            "receiver's LNURL-pay endpoint does not advertise NIP-57 support (`allowsNostr` is false or missing)"
                .to_string(),
        );
    }

    // Leg 2: callback fetch. NIP-57 § "Appendix C" — append `amount` (msats)
    // and the URL-encoded signed kind:9734 as `nostr`. The response carries
    // the bolt11 in the `pr` field.
    //
    // NIP-57 Appendix B also specifies a `lnurl=<bech32>` query parameter
    // so the LN provider can associate the payment with the right Nostr
    // account and publish the kind:9735 receipt. Primal requires this.
    if !callback.starts_with("https://") {
        return Err(format!(
            "LNURL callback URL is not https:// (got: {callback})"
        ));
    }
    let separator = if callback.contains('?') { '&' } else { '?' };
    // Encode the well-known URL as bech32 for the `lnurl=` callback param.
    // If encoding fails we still attempt the request — some providers omit
    // the check, and failing silently here is preferable to aborting the zap.
    let lnurl_param = pay::url_to_bech32_lnurl(&well_known_url)
        .map(|b| format!("&lnurl={}", url_encode_query(&b)))
        .unwrap_or_default();
    let callback_url = format!(
        "{callback}{separator}amount={amount_msats}&nostr={}{lnurl_param}",
        url_encode_query(signed_zap_request_json),
    );
    let callback_response = http_get_json(&callback_url)?;

    // LUD-06 says a successful response is `{ "pr": "lnbc…" }`; an error
    // shape is `{ "status": "ERROR", "reason": "…" }`. Handle the error
    // shape so the user sees the provider's reason rather than a generic
    // "missing pr field".
    if let Some(status) = callback_response.get("status").and_then(serde_json::Value::as_str) {
        if status.eq_ignore_ascii_case("ERROR") {
            let reason = callback_response
                .get("reason")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("LNURL provider returned ERROR without a reason");
            return Err(format!("LNURL provider error: {reason}"));
        }
    }
    let bolt11 = callback_response
        .get("pr")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| {
            "LNURL callback response missing `pr` (bolt11 invoice) field".to_string()
        })?;
    if !looks_like_bolt11(bolt11) {
        return Err(format!(
            "LNURL callback returned a `pr` value that does not look like a bolt11 invoice: {bolt11}"
        ));
    }
    // NIP-57 recommendation — verify the bolt11 encodes exactly the amount the
    // user requested before handing it to the wallet for automatic payment.
    // Fail closed: an amountless invoice or one with a different amount is
    // never forwarded to the wallet (D6 — errors as state, not panic).
    validate_bolt11_amount(bolt11, amount_msats)?;
    Ok(bolt11.to_string())
}

/// Validate that a bolt11 invoice encodes exactly `requested_msats`.
///
/// Parses the BOLT-11 HRP with [`crate::bolt11::amount_msats`] and compares
/// the result against the user-chosen amount.  Returns `Err` when:
///
/// - the invoice is **amountless** (parser returns `None`) — fail closed,
///   because an unverifiable invoice must not be auto-paid (a malicious
///   provider could charge any amount); or
/// - the encoded amount **does not match** the requested amount — a mismatch
///   means a buggy or hostile LNURL provider; the error message names both
///   values so the user can diagnose it.
///
/// Returns `Ok(())` only when the parsed amount equals `requested_msats`
/// exactly.
pub(crate) fn validate_bolt11_amount(bolt11: &str, requested_msats: u64) -> Result<(), String> {
    match crate::bolt11::amount_msats(bolt11) {
        None => Err(format!(
            "LNURL provider returned an amountless bolt11 invoice — \
             refusing automatic payment of an unverifiable amount \
             (requested {requested_msats} msats)"
        )),
        Some(actual) if actual != requested_msats => Err(format!(
            "LNURL provider invoice amount mismatch: requested {requested_msats} msats \
             but bolt11 encodes {actual} msats — refusing automatic payment"
        )),
        Some(_) => Ok(()),
    }
}

/// One-shot HTTP GET → JSON. Bounded by `LNURL_HTTP_TIMEOUT_SECS` and
/// `LNURL_MAX_RESPONSE_BYTES`. The result is a `serde_json::Value` rather
/// than a typed shape because LNURL-pay returns a slightly different schema
/// per leg (well-known has `callback`/`minSendable`/…; callback has
/// `pr`/`status`/…), and the typed-shape boilerplate adds no safety here.
fn http_get_json(url: &str) -> Result<serde_json::Value, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout(std::time::Duration::from_secs(LNURL_HTTP_TIMEOUT_SECS))
        .build();
    let response = agent
        .get(url)
        .call()
        .map_err(|e| format!("HTTP GET {url} failed: {e}"))?;
    if response.status() != 200 {
        return Err(format!(
            "HTTP GET {url} returned status {} {}",
            response.status(),
            response.status_text()
        ));
    }
    // Bound the response so a runaway/hostile endpoint can't OOM us.
    let mut body = Vec::with_capacity(1024);
    response
        .into_reader()
        .take(LNURL_MAX_RESPONSE_BYTES as u64)
        .read_to_end(&mut body)
        .map_err(|e| format!("read response body from {url}: {e}"))?;
    serde_json::from_slice::<serde_json::Value>(&body)
        .map_err(|e| format!("parse JSON from {url}: {e}"))
}

/// Standalone blocking entry point for one-shot tools and integration tests.
///
/// Unlike [`FetchLnurlInvoiceCommand`] (which runs inside the NMP actor
/// pipeline), this function blocks the calling thread directly. Use it from
/// CLI binaries and integration tests where the actor stack is not available.
///
/// Signs the kind:9734 zap request with `keys`, does the two-leg LNURL-pay
/// round-trip, and returns the bolt11 invoice string on success.
#[allow(dead_code)] // Reference impl retained for the zap-smoke tool's docs.
pub(crate) fn fetch_bolt11_for_zap(
    keys: &Keys,
    lnurl_or_address: &str,
    amount_msats: u64,
    recipient_pubkey: &str,
    relays: &[String],
    comment: Option<&str>,
) -> Result<String, String> {
    // NIP-57: the `lnurl` tag must be bech32-encoded (LUD-01), NOT the raw
    // lightning address. Resolve → well-known URL → bech32 before building.
    let well_known_url = lnurl_to_well_known_url(lnurl_or_address)?;
    let bech32_lnurl = pay::url_to_bech32_lnurl(&well_known_url)?;
    let mut builder = crate::build::ZapRequest::to_pubkey(recipient_pubkey)
        .amount_msats(amount_msats)
        .relays(relays.to_vec())
        .lnurl(&bech32_lnurl);
    if let Some(c) = comment {
        builder = builder.comment(c);
    }
    let mut unsigned = builder
        .build()
        .map_err(|e| format!("build kind:9734: {e}"))?;
    // D7 — this standalone path owns the wall clock directly (no actor context).
    // Re-stamp `created_at` the same way `FetchLnurlInvoiceCommand::run` does
    // for the actor path. `pubkey` is re-derived from `keys` inside
    // `sign_zap_request` via `EventBuilder::sign_with_keys`.
    unsigned.created_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let signed_json = sign_zap_request(keys, &unsigned)?;
    fetch_lnurl_invoice_blocking(lnurl_or_address, amount_msats, &signed_json)
}

#[cfg(test)]
mod tests;
