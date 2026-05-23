//! NIP-57 zap handler — `ActorCommand::FetchLnurlInvoice` arm.
//!
//! # Why this lives in `nmp-core` (not `nmp-nip57`)
//!
//! Two reasons:
//!
//! 1. The worker needs a `Sender<ActorCommand>` to send the follow-up command
//!    (the bolt11 invoice toast / failure record) back into the actor loop.
//!    That sender is `nmp-core`-private — `nmp-nip57` would gain a circular
//!    dependency by reaching for it.
//! 2. The signing step (kind:9734 zap request) needs the active account's
//!    `nostr::Keys`, which only the actor's `IdentityRuntime` holds. D7 says
//!    the kernel owns key access; an `nmp-nip57` handler cannot reach across
//!    into the actor without violating that.
//!
//! The handler is therefore a thin orchestration layer: NIP-57 nouns
//! (`ZapInput`, kind:9734) stay in `nmp-nip57`; this file only knows how to
//! sign an arbitrary [`UnsignedEvent`], spawn an HTTP worker, and serialize a
//! signed event into the LNURL query parameter. The LNURL-pay decode + URL
//! encode helpers live in the sibling [`zap_lnurl`] module (split for the
//! 500-LOC file-size gate).
//!
//! # D8 — no blocking on the actor thread
//!
//! The actor thread signs the zap request (sync, ~30µs) and immediately
//! spawns a `std::thread` for the HTTP work. The thread:
//!
//! 1. Decodes the LNURL (bech32) or lightning-address (`user@domain`) input
//!    into a `https://…/.well-known/lnurlp/<user>` URL via
//!    [`zap_lnurl::lnurl_to_well_known_url`].
//! 2. HTTP GET that URL → parse `{ "callback": "…", "minSendable": …,
//!    "maxSendable": …, "allowsNostr": …, "nostrPubkey": … }`.
//! 3. HTTP GET `{callback}?amount=<msats>&nostr=<urlencoded-signed-9734>` →
//!    parse `{ "pr": "lnbc…" }`.
//! 4. Send a follow-up [`ActorCommand`] back through the cloned sender:
//!    on success a [`ActorCommand::ShowToast`] (carrying the bolt11) AND
//!    — when a `correlation_id` was supplied — a
//!    [`ActorCommand::RecordActionSuccess`] so the host's `dispatch_action`
//!    spinner closes; on failure a `ShowToast` and a
//!    [`ActorCommand::RecordActionFailure`] (same `correlation_id` guard).
//!
//! # ADR-0026 / bunker accounts — out of scope
//!
//! The handler reads `IdentityRuntime::active_local_keys`. Bunker (NIP-46
//! remote-signer) accounts return `None`; the handler fails closed with a
//! clear toast and records `Failed` against the `correlation_id`. NIP-46
//! signing of kind:9734 zap requests is the ADR-0026 Phase 2 follow-up
//! (parallel to the NIP-17 DM bunker-send path documented in
//! `commands/dm.rs`).
//!
//! # NWC payment — out of scope
//!
//! This handler returns the bolt11 invoice. It does NOT pay the invoice.
//! NIP-47 Nostr Wallet Connect ([`ActorCommand::WalletPayInvoice`], gated
//! behind the `wallet` Cargo feature) is the follow-up path: the host reads
//! the toast / a future `last_action_outcomes` projection, then dispatches
//! the wallet pay.

use std::io::Read;
use std::sync::mpsc::Sender;

use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

use super::zap_lnurl::{looks_like_bolt11, lnurl_to_well_known_url, url_encode_query};
use crate::actor::commands::identity::IdentityRuntime;
use crate::actor::ActorCommand;
use crate::kernel::Kernel;
use crate::substrate::UnsignedEvent;

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

/// Drive the `FetchLnurlInvoice` arm.
///
/// Returns no outbound relay frames — the kind:9734 zap request is delivered
/// over HTTP to the LNURL callback, NOT broadcast to Nostr relays (NIP-57 §
/// "Appendix C"). Publishing kind:9734 to relays would violate the spec; the
/// receipt (kind:9735) is what relays see, and it is minted by the LN
/// provider after the invoice settles.
///
/// # Failure surfaces
///
/// Every failure path threads through *both* observable surfaces a host
/// might be watching:
///
/// 1. A `ShowToast` with the human-readable reason (always — covers hosts
///    that don't subscribe to `action_stages`).
/// 2. When `correlation_id` is `Some`, a `RecordActionFailure` so the
///    `dispatch_action` spinner the host keyed on the returned id clears
///    on the next tick (mirrors the existing NIP-17 send pattern).
///
/// On success the bolt11 invoice is surfaced as a `ShowToast` whose
/// `message` starts with `Zap invoice: lnbc…`. A host can substring-match
/// `lnbc` (or `lntb`/`lnbcrt` for testnet/regtest) to drive its NWC
/// auto-pay flow. A snapshot-projection surface for invoices is the
/// designed follow-up (per memory note #57 — `last_action_outcomes`); the
/// toast is the minimum-viable observable per ADR-0024.
///
/// The worker ALSO sends [`ActorCommand::RecordActionSuccess`]
/// when a `correlation_id` was supplied, so the dispatched-action spinner
/// keyed on that id clears on the next snapshot tick. Without this the
/// `nmp.nip57.zap` spinner hangs forever: `ShowToast` is a human-readable
/// surface, NOT the spinner-closing one (`action_results` is the closing
/// surface). Mirrors the dual-surface contract the failure leg already
/// honours below.
pub(crate) fn handle_fetch_lnurl_invoice(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    command_tx: Sender<ActorCommand>,
    mut unsigned: UnsignedEvent,
    lnurl_or_address: String,
    amount_msats: u64,
    correlation_id: Option<String>,
) {
    // D7 — kernel owns the wall clock. Executors pass `0` as a sentinel; the
    // kernel re-stamps here. Matches the `PublishUnsignedEventToRelays` arm.
    if unsigned.created_at == 0 {
        unsigned.created_at = kernel.now_secs();
    }

    // V-07 — relay selection is kernel policy, never shell policy. The
    // kind:9734 `relays` tag tells the LN provider where to publish the
    // kind:9735 receipt (NIP-57 § "Appendix F"): the correct answer is the
    // RECIPIENT's NIP-65 write list, which only the kernel knows.
    //
    // When the executor produced no `relays` tag (the caller passed an
    // empty list — the D0-correct shape) we inject from
    // `kernel.author_write_relays(recipient)`. That helper falls back to
    // the cold-start bootstrap discovery seed when no kind:10002 is cached
    // yet, so the tag is never empty for a well-formed recipient.
    inject_recipient_relays(kernel, &mut unsigned);

    // ADR-0026 Phase 1 — local keys only. Bunker accounts have no local
    // secret material so the kind:9734 signature cannot be minted on this
    // path. Fail closed with a clear toast + `RecordActionFailure` so the
    // host spinner resolves.
    let Some(keys) = identity.active_local_keys().cloned() else {
        let reason = "zap requires a local-keys account; bunker signing for kind:9734 \
                      is not yet implemented (ADR-0026 Phase 2 follow-up)";
        kernel.set_last_error_toast(Some(reason.to_string()));
        if let Some(cid) = correlation_id {
            kernel.record_action_failure(cid, reason.to_string());
        }
        return;
    };

    // Sign the kind:9734 on the actor thread (D7). The signed JSON is what
    // the LNURL callback expects in its `nostr=<urlencoded>` query param.
    let signed_json = match sign_zap_request(&keys, &unsigned) {
        Ok(json) => json,
        Err(reason) => {
            let msg = format!("failed to sign zap request: {reason}");
            kernel.set_last_error_toast(Some(msg.clone()));
            if let Some(cid) = correlation_id {
                kernel.record_action_failure(cid, msg);
            }
            return;
        }
    };

    // Spawn the HTTP worker. `std::thread` (not tokio) — nmp-core has no
    // async runtime; the actor itself is `std::thread`-based. The worker
    // owns its own clones of everything it needs; nothing references the
    // actor's mutable state after this point. D8: zero blocking on the
    // actor thread.
    std::thread::spawn(move || {
        match fetch_lnurl_invoice_blocking(
            &lnurl_or_address,
            amount_msats,
            &signed_json,
        ) {
            Ok(bolt11) => {
                let message = format!("Zap invoice: {bolt11}");
                // D6 — a disconnected actor (Shutdown raced our worker) is
                // a benign drop. The toast was the observable signal; if the
                // actor's gone there's no host watching anyway.
                let _ = command_tx.send(ActorCommand::ShowToast { message });
                // ADR-0024 follow-up — auto-dispatch WalletPayInvoice when
                // the wallet feature is active so the bolt11 → NWC pay loop
                // closes without a second host round-trip. `correlation_id`
                // on the wallet pay is `None`: the wallet payment is a
                // *separate* async operation whose outcome surfaces as its
                // own `ShowToast` from the NWC handler, not a failure of
                // the zap correlation_id (the zap "succeeded" the moment
                // the LNURL provider returned a valid invoice). Sent
                // BEFORE `RecordActionSuccess` so a host observing the
                // success can never double-tap before the WalletPayInvoice
                // is enqueued — `Sender::send` is microseconds-non-blocking,
                // so the host-visible ordering is preserved either way,
                // but the strict pre-success ordering keeps the
                // dispatch-then-pay invariant readable.
                #[cfg(feature = "wallet")]
                let _ = command_tx.send(ActorCommand::WalletPayInvoice {
                    bolt11: bolt11.clone(),
                    amount_msats: Some(amount_msats),
                    correlation_id: None,
                });
                // When the zap originated from `dispatch_action` the
                // registry minted a correlation_id and the host is
                // waiting on `action_results` to close its spinner.
                // `ShowToast` is the human-readable signal, NOT the
                // spinner-closing surface; without `RecordActionSuccess`
                // the spinner hangs forever. Mirror the failure leg's
                // correlation_id guard — direct callers that pass `None`
                // (no dispatched action waiting on an id) get the toast
                // only, same as the failure branch below.
                if let Some(cid) = correlation_id {
                    let _ = command_tx
                        .send(ActorCommand::RecordActionSuccess { correlation_id: cid });
                }
            }
            Err(reason) => {
                let _ = command_tx.send(ActorCommand::ShowToast {
                    message: format!("Zap failed: {reason}"),
                });
                if let Some(cid) = correlation_id {
                    let _ = command_tx.send(ActorCommand::RecordActionFailure {
                        correlation_id: cid,
                        reason,
                    });
                }
            }
        }
    });
}

/// V-07 — inject the kind:9734 `relays` tag from the recipient's NIP-65
/// (kind:10002) write list when the caller produced no tag. Relay
/// selection is kernel policy: shells (Swift, web) MUST NOT decide where
/// the LN provider should publish the kind:9735 receipt.
///
/// Semantics:
/// - If a non-empty `relays` tag is already present (length > 1 — the key
///   plus at least one URL), do nothing. The caller chose explicitly.
/// - Otherwise extract the recipient pubkey from the `p` tag and inject
///   `["relays", <write-relay-urls…>]`. `Kernel::author_write_relays`
///   falls back to `bootstrap_discovery_relays` on cold-start, so the
///   tag is never empty for a well-formed recipient. A missing `p` tag
///   is a builder bug — we still inject the bootstrap seed so the
///   kind:9734 carries a valid `relays` tag rather than crashing later
///   in the signed-JSON path.
fn inject_recipient_relays(kernel: &Kernel, unsigned: &mut UnsignedEvent) {
    let relays_present = unsigned.tags.iter().any(|t| {
        t.first().is_some_and(|k| k == "relays") && t.len() > 1
    });
    if relays_present {
        return;
    }
    let recipient: String = unsigned
        .tags
        .iter()
        .find(|t| t.first().is_some_and(|k| k == "p"))
        .and_then(|t| t.get(1))
        .cloned()
        .unwrap_or_default();
    let relays = if recipient.is_empty() {
        kernel.bootstrap_discovery_relays()
    } else {
        kernel.author_write_relays(&recipient)
    };
    let mut relays_tag = Vec::with_capacity(1 + relays.len());
    relays_tag.push("relays".to_string());
    relays_tag.extend(relays);
    unsigned.tags.push(relays_tag);
}

/// Sign `unsigned` with `keys` and emit the flat NIP-01 JSON object the
/// LNURL callback expects in its `nostr=<urlencoded>` parameter.
///
/// Mirrors the wallet-runtime `sign_nwc_request` precedent — build a
/// `nostr::Event` via `EventBuilder`, then re-serialize to JSON. The reseat
/// step is the bridge between the substrate's typed `UnsignedEvent` shape
/// (kind / tags / content / `created_at`) and the nostr crate's signer API.
fn sign_zap_request(keys: &Keys, unsigned: &UnsignedEvent) -> Result<String, String> {
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
#[path = "zap/tests.rs"]
mod tests;
