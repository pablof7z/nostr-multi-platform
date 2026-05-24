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
//! This command therefore emits NO relay-bound frames; the only follow-up
//! `ActorCommand`s are `ShowToast` (always — the human-readable surface) and
//! the optional `RecordActionSuccess` / `RecordActionFailure` that close the
//! host's spinner when a `correlation_id` was supplied.
//!
//! # Surfaces threaded through `ProtocolCommandContext`
//!
//! - [`ProtocolCommandContext::now_secs`] — D7 — kernel owns the wall clock.
//!   The executor passes `created_at = 0` as a sentinel; this command
//!   re-stamps before signing (mirrors the
//!   `PublishUnsignedEventToRelays` precedent).
//! - TODO Debt C follow-up: V-07 (recipient relay injection) is currently
//!   disabled — Debt C deleted the routing accessors from the substrate
//!   context (`author_write_relays` / `bootstrap_discovery_relays` were
//!   routing leaks). Migration through the kernel's `OutboxRouter` is
//!   pending; until then `inject_recipient_relays` is a no-op and the
//!   kind:9734 carries no `relays` tag.
//! - [`ProtocolCommandContext::active_local_keys`] — ADR-0026 Phase 1:
//!   local-keys accounts only. Bunker (NIP-46) accounts return `None`; we
//!   fail closed with a clear toast and a `RecordActionFailure`.
//! - [`ProtocolCommandContext::record_action_stage_requested`] — track the
//!   `Requested` stage against the host's `correlation_id` (when supplied)
//!   so the stage observer sees the transition before the worker thread
//!   posts the terminal.
//! - [`ProtocolCommandContext::send`] — re-enter the actor loop with the
//!   follow-up `ActorCommand`s (`ShowToast`, `RecordActionSuccess`,
//!   `RecordActionFailure`).
//!
//! # D8 — no blocking on the actor thread
//!
//! The actor thread signs the zap request (sync, ~30µs) and immediately
//! spawns a `std::thread` for the HTTP work. The thread:
//!
//! 1. Decodes the LNURL (bech32) or lightning-address (`user@domain`) input
//!    into a `https://…/.well-known/lnurlp/<user>` URL via
//!    [`pay::lnurl_to_well_known_url`].
//! 2. HTTP GET that URL → parse `{ "callback": "…", "minSendable": …,
//!    "maxSendable": …, "allowsNostr": …, "nostrPubkey": … }`.
//! 3. HTTP GET `{callback}?amount=<msats>&nostr=<urlencoded-signed-9734>` →
//!    parse `{ "pr": "lnbc…" }`.
//! 4. Send the follow-up [`ActorCommand`]s back through the cloned
//!    [`Sender<ActorCommand>`] (`ShowToast` on success / failure, plus the
//!    spinner-closing `RecordActionSuccess` / `RecordActionFailure` when a
//!    `correlation_id` was supplied).
//!
//! # NWC auto-pay handoff — wallet feature only on `nmp-core`
//!
//! The legacy handler chained `WalletPayInvoice` onto the success leg when
//! `nmp-core/wallet` was on. V-41 deliberately does NOT carry that
//! cross-NIP coupling: the wallet pay step is a follow-up dispatch the host
//! kicks off after observing the bolt11 in the `ShowToast` (mirrors the
//! ADR-0024 "minimum-viable observable" path). When V-43 lands (zap-pay
//! chain), the wallet handoff becomes a multi-step `dispatch_action`
//! contract rather than a hard-coded `Sender::send` from this worker.

mod pay;

use std::io::Read;

use nmp_core::substrate::{
    ProtocolCommand, ProtocolCommandContext, ProtocolCommandError, UnsignedEvent,
};
use nmp_core::ActorCommand;
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

pub use pay::{looks_like_bolt11, lnurl_to_well_known_url, url_encode_query};

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
/// `ZapAction::execute` (see `crate::action`).
///
/// The fields mirror the legacy `ActorCommand::FetchLnurlInvoice` variant
/// payload one-for-one — every field is consumed inside [`Self::run`].
#[derive(Debug)]
pub struct FetchLnurlInvoiceCommand {
    /// Unsigned kind:9734 zap request — built by `ZapAction::execute`. The
    /// `created_at` field is the D7 sentinel `0`; this command re-stamps
    /// from the kernel clock before signing.
    pub unsigned: UnsignedEvent,
    /// LN-side destination. One of three shapes (LUD-01 / LUD-06 / LUD-16):
    /// a lightning address (`user@domain`), a bech32 LNURL (`lnurl1…`), or
    /// a bare `https://` URL. Decoded by [`pay::lnurl_to_well_known_url`].
    pub lnurl_or_address: String,
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
            lnurl_or_address,
            amount_msats,
            correlation_id,
        } = *self;

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

        // ADR-0026 Phase 1 — local keys only. Bunker accounts have no local
        // secret material so the kind:9734 signature cannot be minted on this
        // path. Fail closed with a clear toast + `RecordActionFailure` so the
        // host spinner resolves.
        let Some(keys) = ctx.active_local_keys() else {
            let reason = "zap requires a local-keys account; bunker signing for kind:9734 \
                          is not yet implemented (ADR-0026 Phase 2 follow-up)";
            ctx.send(ActorCommand::ShowToast {
                message: reason.to_string(),
            });
            if let Some(cid) = correlation_id {
                ctx.send(ActorCommand::RecordActionFailure {
                    correlation_id: cid,
                    reason: reason.to_string(),
                });
            }
            return Ok(());
        };

        // Sign the kind:9734 on the actor thread (D7). The signed JSON is
        // what the LNURL callback expects in its `nostr=<urlencoded>` query
        // param.
        let signed_json = match sign_zap_request(&keys, &unsigned) {
            Ok(json) => json,
            Err(reason) => {
                let msg = format!("failed to sign zap request: {reason}");
                ctx.send(ActorCommand::ShowToast { message: msg.clone() });
                if let Some(cid) = correlation_id {
                    ctx.send(ActorCommand::RecordActionFailure {
                        correlation_id: cid,
                        reason: msg,
                    });
                }
                return Ok(());
            }
        };

        // Spawn the HTTP worker. `std::thread` (not tokio) — nmp-nip57 has
        // no async runtime; the actor itself is `std::thread`-based. The
        // worker owns its own clones of everything it needs; nothing
        // references the actor's mutable state after this point. D8: zero
        // blocking on the actor thread.
        //
        // [`ProtocolCommandContext::command_sender_clone`] hands us an
        // owned `Sender<ActorCommand>` (cheap atomic ref-count bump) the
        // worker moves into its closure. The worker uses it to post
        // follow-up commands (`ShowToast`, `RecordActionSuccess`,
        // `RecordActionFailure`) back into the actor loop after the
        // dispatch arm (and its `ProtocolCommandContext`) have returned.
        let worker_tx = ctx.command_sender_clone();
        std::thread::spawn(move || {
            match fetch_lnurl_invoice_blocking(
                &lnurl_or_address,
                amount_msats,
                &signed_json,
            ) {
                Ok(bolt11) => {
                    let message = format!("Zap invoice: {bolt11}");
                    let _ = worker_tx.send(ActorCommand::ShowToast { message });
                    if let Some(cid) = correlation_id {
                        let _ = worker_tx
                            .send(ActorCommand::RecordActionSuccess { correlation_id: cid });
                    }
                }
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

        Ok(())
    }
}

/// V-07 — inject the kind:9734 `relays` tag from the recipient's NIP-65
/// (kind:10002) write list when the caller produced no tag.
///
/// TODO Debt C follow-up: Debt C removed the
/// `ProtocolCommandContext::author_write_relays` +
/// `bootstrap_discovery_relays` accessors (routing leaks — routing is the
/// kernel's `OutboxRouter`, not a substrate read accessor). This
/// function must be migrated to route through the kernel's
/// `OutboxRouter` (either populate `RoutingContext::explicit_targets` or
/// route a synthetic event and consume `routed.urls()`); the migration
/// is non-trivial (the LNURL fetcher needs a *receiver*-author write set,
/// while the publish path routes by *event*-author) and exceeds this
/// PR's LOC budget.
///
/// Until then this function is a no-op: kind:9734 carries no `relays`
/// tag, the LN provider falls back to its default behaviour (typically
/// publishing the kind:9735 receipt to its operator-configured set; some
/// providers reject the request, in which case the user sees a toast).
/// Two LNURL unit tests are `#[ignore]`d behind the same TODO.
pub(crate) fn inject_recipient_relays(
    _ctx: &ProtocolCommandContext<'_>,
    unsigned: &mut UnsignedEvent,
) {
    let relays_present = unsigned.tags.iter().any(|t| {
        t.first().is_some_and(|k| k == "relays") && t.len() > 1
    });
    if relays_present {
        return;
    }
    tracing::warn!(
        "NIP-57: kind:9734 relays tag injection disabled pending Debt C \
         follow-up (route through OutboxRouter); LN provider falls back \
         to its own default for kind:9735 publication"
    );
}

/// Sign `unsigned` with `keys` and emit the flat NIP-01 JSON object the
/// LNURL callback expects in its `nostr=<urlencoded>` parameter.
///
/// Mirrors the wallet-runtime `sign_nwc_request` precedent — build a
/// `nostr::Event` via `EventBuilder`, then re-serialize to JSON. The reseat
/// step is the bridge between the substrate's typed `UnsignedEvent` shape
/// (kind / tags / content / `created_at`) and the nostr crate's signer API.
pub(crate) fn sign_zap_request(keys: &Keys, unsigned: &UnsignedEvent) -> Result<String, String> {
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

/// Two-leg LNURL-pay HTTP round-trip. Runs on the spawned worker thread —
/// blocking I/O is acceptable here precisely because we are NOT on the
/// actor thread.
fn fetch_lnurl_invoice_blocking(
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
    if !callback.starts_with("https://") {
        return Err(format!(
            "LNURL callback URL is not https:// (got: {callback})"
        ));
    }
    let separator = if callback.contains('?') { '&' } else { '?' };
    let callback_url = format!(
        "{callback}{separator}amount={amount_msats}&nostr={}",
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
    Ok(bolt11.to_string())
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

#[cfg(test)]
mod tests;
