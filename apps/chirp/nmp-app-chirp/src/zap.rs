//! Chirp's NIP-57 zap wiring — the host-side glue that turns a validated
//! `nmp.zap` action into both a kind:9734 publish AND an out-of-process
//! LNURL-pay round-trip.
//!
//! # Why this lives in `nmp-app-chirp` (not `nmp-nip57`)
//!
//! `nmp-nip57` owns the protocol: building the kind:9734 [`UnsignedEvent`],
//! decoding kind:9735 receipts, and performing the LNURL HTTP round-trip
//! ([`nmp_nip57::lnurl::fetch_invoice`]). It is intentionally ignorant of the
//! kernel's signing seam and the `ActorCommand` channel.
//!
//! Chirp owns the host wiring: capturing an [`ActorCommand`] sender clone at
//! registration time, accessing the kernel's `nip17_local_keys` slot to sign
//! the kind:9734 locally for the LNURL POST, spawning the worker thread that
//! performs the blocking HTTP calls, and routing the outcome back into the
//! actor as a [`ActorCommand::ShowToast`] follow-up. None of that crosses the
//! D0 line because it is not protocol logic — it is plumbing between an
//! `ActionModule` and the actor.
//!
//! # Flow
//!
//! 1. Host calls `nmp_app_dispatch_action("nmp.zap", json)`.
//! 2. The registered validator runs [`ZapModule::start`] — pure shape check.
//! 3. The registered executor:
//!    a. Calls [`zap_request_command`] to build the kind:9734 `UnsignedEvent`
//!       and wrap it in [`ActorCommand::PublishUnsignedEventToRelays`].
//!    b. Sends that command to the actor via the `send` callback — the actor
//!       signs, ids, and publishes the event to the recipient's relays. The
//!       `dispatch_action` correlation_id terminates here (the spinner clears
//!       when the publish settles; see *Known limitations* below).
//!    c. Spawns a worker thread holding (i) a clone of the unsigned event,
//!       (ii) a clone of the `Sender<ActorCommand>`, (iii) a clone of the
//!       shared `nip17_local_keys` slot, (iv) the `lnurl` + `amount_msats`
//!       fields. The worker locks the keys slot, signs the kind:9734 locally
//!       (NIP-44 path — same seam NIP-17 uses), calls `fetch_invoice`, and
//!       routes the bolt11 invoice (success) OR a human-readable error
//!       (failure) into the actor as `ActorCommand::ShowToast`.
//!
//! Two events are signed (one in the actor, one in the worker) — both from
//! the same private key, both for the same logical kind:9734. They have
//! identical `created_at` because the worker re-stamps using the actor's
//! `now_secs()` value sent into the command path… *actually* they differ:
//! the actor re-stamps the command's `event.created_at` if it is `0` (the
//! sentinel `zap_request_command` sets); the worker re-stamps locally with
//! `SystemTime::now()`. The two ids will therefore differ by a few ms of
//! wall-clock skew, which is **acceptable** per NIP-57:
//!
//! - The kind:9734 the LN provider EMBEDS in the kind:9735 receipt is the one
//!   it received via LNURL POST (the worker's copy).
//! - The kind:9734 PUBLISHED to relays is the actor's copy.
//!
//! NIP-57 does not require these to be byte-identical — receipts are matched
//! to requests by recipient + amount + zapped-event, not by request id. The
//! published copy is essentially a diagnostic record; the LN-embedded copy is
//! the canonical one.
//!
//! # Known limitations
//!
//! - **Bunker (NIP-46) signers are unsupported.** Remote signers do not
//!   expose a local `nostr::Keys`, so the worker thread cannot sign the
//!   kind:9734 for the LNURL POST. When the local-keys slot is `None`
//!   (bunker sign-in OR no active account), the worker routes
//!   `ActorCommand::ShowToast` with an explicit "zap requires nsec sign-in
//!   in this build" message. Same constraint NIP-17 hit; ADR-0026 is the
//!   forward path for both.
//! - **No `correlation_id` round-trip past the kind:9734 publish.** The
//!   spinner UI keys off `nmp_app_dispatch_action`'s returned id, which is
//!   tied to the kind:9734 `PublishUnsignedEventToRelays` terminal. The
//!   LNURL invoice / failure surfaces as a separate `ShowToast` with no
//!   correlation. Closing that gap requires either a new
//!   `ActorCommand::ShowToast { correlation_id, … }` variant or routing the
//!   invoice through `WalletPayInvoice` (gated on `feature = "wallet"`,
//!   which `nmp-nip57` deliberately does not enable for D0 reasons).
//! - **`ShowToast` is the `last_error_toast` slot.** Reusing it for a
//!   success-path "invoice ready: lnbc…" message overloads the error
//!   surface. The wallet hand-off note in the PR description carries the
//!   rationale (and the explicit task-spec permission).

use std::sync::{Arc, Mutex};
use std::sync::mpsc::Sender;
use std::thread;

use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection};
use nmp_core::{ActorCommand, NmpApp};
use nmp_nip57::{fetch_invoice, zap_request_command, ZapAction, ZapModule};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};

/// Register the `nmp.zap` namespace against `app`'s action registry.
///
/// Wires the typed [`ZapModule`] from the `nmp-nip57` protocol crate plus a
/// host-shaped executor that fans the validated action into:
/// 1. an immediate [`ActorCommand::PublishUnsignedEventToRelays`] (the
///    kind:9734 publish — the actor signs);
/// 2. a worker-thread LNURL-pay round-trip that signs the same kind:9734
///    locally (via the shared `nip17_local_keys` slot) and routes the bolt11
///    invoice (or a typed error) back into the actor as a
///    [`ActorCommand::ShowToast`].
///
/// Registration MUST happen during host init — before `nmp_app_start` and
/// before any `nmp_app_dispatch_action` call — because the registry mutation
/// requires `&mut NmpApp`. The caller in `ffi.rs::nmp_app_chirp_register`
/// orders this correctly.
pub(crate) fn register_nip57_actions(app: &mut NmpApp) {
    // Module half — pure shape validator. Mirrors the `wire_action!` pattern
    // from `nip29.post_chat_message`: surface the typed `ZapModule::start`
    // through the host seam.
    app.register_action_module(ZapModule::NAMESPACE, |action_json| {
        let action: ZapAction = serde_json::from_str(action_json)
            .map_err(|e| ActionRejection::Invalid(e.to_string()))?;
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let mut ctx = ActionContext { now_ms };
        ZapModule::start(&mut ctx, action)
    });

    // Executor half. Capture the two host-only seams once at registration
    // time: a `Sender<ActorCommand>` clone (the actor command channel) and
    // the shared `nip17_local_keys` slot (the NIP-44 local-keys read seam).
    // Both are `Clone + Send + Sync + 'static`, so they live happily inside
    // the `Fn(...) + Send + Sync + 'static` executor closure.
    let actor_tx: Sender<ActorCommand> = app.actor_sender();
    let local_keys: Arc<Mutex<Option<Keys>>> = app.nip17_local_keys();

    app.register_action_executor(
        ZapModule::NAMESPACE,
        move |action_json, _correlation_id, send| {
            // 1) Publish the kind:9734 via the in-process actor (D8 — the
            //    actor owns signing + relay routing).
            let cmd = zap_request_command(action_json)?;
            // Extract the unsigned event + relays + lnurl/amount before
            // moving `cmd` into `send` — the worker thread needs them all.
            let (unsigned, relays, lnurl, amount_msats) = match &cmd {
                ActorCommand::PublishUnsignedEventToRelays { event, relays } => {
                    // The action JSON validated; pull the lnurl + amount
                    // back out for the LNURL leg. (The command does not
                    // carry them — `zap_request_command` consumed them into
                    // the `relays` tag and the `amount` tag respectively.)
                    let action: ZapAction = serde_json::from_str(action_json)
                        .map_err(|e| e.to_string())?;
                    let ZapAction::Zap { lnurl, amount_sats, .. } = action;
                    let amount_msats = amount_sats.saturating_mul(1000);
                    (event.clone(), relays.clone(), lnurl, amount_msats)
                }
                other => {
                    return Err(format!(
                        "zap_request_command returned unexpected ActorCommand: {other:?}"
                    ));
                }
            };
            // The actor receives the kind:9734 and signs + publishes it.
            // This terminates the `dispatch_action` correlation_id (the
            // spinner clears when this publish settles — see module docs).
            send(cmd);

            // 2) Spawn the LNURL worker. Cloning is cheap — `Sender` is a
            //    handle, `Arc<Mutex<_>>` is reference-counted. The worker
            //    owns everything it touches for its lifetime; no shared
            //    mutable state crosses the boundary except the keys slot
            //    (read-only via lock+clone).
            let tx = actor_tx.clone();
            let keys_slot = Arc::clone(&local_keys);
            // The worker thread is a "fire and forget" handle — we never
            // join it. On host shutdown the `Sender` channel closes (the
            // actor is gone), and the worker's final `tx.send` becomes a
            // no-op send-on-closed-channel. If the LNURL call is in flight
            // at that moment, the 20-second `ureq` timeout caps the leak.
            // No polling: the worker blocks on TCP I/O, never on a sleep
            // loop (D8 + memory: no polling — ever).
            thread::Builder::new()
                .name("nmp-zap-lnurl".to_string())
                .spawn(move || run_lnurl_leg(tx, keys_slot, unsigned, relays, lnurl, amount_msats))
                // If the OS refuses the thread spawn (vanishingly rare on
                // any platform Chirp targets) we degrade to the publish-only
                // half — kind:9734 was already enqueued in step 1, the user
                // sees no invoice but the zap intent is recorded.
                .ok();
            Ok(())
        },
    );
}

/// Worker-thread entry point for the LNURL-pay leg.
///
/// Runs entirely off the actor: the only thing it does to the actor is push a
/// single follow-up `ActorCommand::ShowToast` via the captured channel handle.
///
/// `unsigned` is the kind:9734 the actor is also publishing; the worker signs
/// its own copy from the shared `nip17_local_keys` slot. `_relays` is unused
/// here — the worker only needs the LNURL endpoint and amount; relays are
/// already pinned on the actor-side publish.
fn run_lnurl_leg(
    tx: Sender<ActorCommand>,
    keys_slot: Arc<Mutex<Option<Keys>>>,
    unsigned: nmp_core::substrate::UnsignedEvent,
    _relays: Vec<String>,
    lnurl: String,
    amount_msats: u64,
) {
    // Snapshot the local Keys. The slot is `None` when no account is signed
    // in OR the active account uses a remote (NIP-46) signer — both surface
    // as the same explicit-failure toast (see module docs).
    let keys: Keys = {
        let guard = match keys_slot.lock() {
            Ok(g) => g,
            // D6: a poisoned mutex is a soft-fail toast, never a panic that
            // unwinds across the worker thread boundary.
            Err(_) => {
                let _ = tx.send(ActorCommand::ShowToast {
                    message: "zap failed: identity keys slot poisoned".to_string(),
                });
                return;
            }
        };
        match guard.as_ref() {
            Some(k) => k.clone(),
            None => {
                let _ = tx.send(ActorCommand::ShowToast {
                    message: "zap requires nsec sign-in (bunker NIP-46 signers cannot \
                              sign the LNURL request in this build)".to_string(),
                });
                return;
            }
        }
    };

    // Locally sign a fresh copy of the kind:9734. The actor-side publish
    // signs its own copy; the two ids may differ by wall-clock skew, which
    // NIP-57 tolerates (see module docs — receipts match by recipient +
    // amount + zapped-event, not by request id).
    let signed_event = match sign_zap_request(&unsigned, &keys) {
        Ok(json) => json,
        Err(reason) => {
            let _ = tx.send(ActorCommand::ShowToast {
                message: format!("zap sign failed: {reason}"),
            });
            return;
        }
    };

    // Perform the blocking LNURL round-trip. `fetch_invoice` enforces the
    // 20-second per-call timeout internally.
    let outcome = match fetch_invoice(&lnurl, &signed_event, amount_msats) {
        Ok(inv) => format!(
            "zap invoice ready ({} sats): {}",
            amount_msats / 1000,
            inv.invoice
        ),
        Err(e) => format!("zap failed: {e}"),
    };
    // Best-effort send; if the actor channel has closed (host shutdown), the
    // command is silently dropped — there is no consumer anyway.
    let _ = tx.send(ActorCommand::ShowToast { message: outcome });
}

/// Sign `unsigned` with `keys` and return the canonical NIP-01 JSON.
///
/// Re-stamps `created_at` from local wall clock if the action carries the
/// sentinel `0` (matching the actor-side behaviour for
/// `PublishUnsignedEventToRelays`). The two copies will therefore have
/// slightly different `created_at` values and ids — see the module-level
/// note for why NIP-57 tolerates this.
fn sign_zap_request(
    unsigned: &nmp_core::substrate::UnsignedEvent,
    keys: &Keys,
) -> Result<String, String> {
    if unsigned.kind > u16::MAX as u32 {
        return Err(format!("invalid kind {}: out of u16 range", unsigned.kind));
    }
    let kind = Kind::from_u16(unsigned.kind as u16);

    // Parse each tag through `nostr::Tag::parse` so we hard-fail on a
    // malformed tag rather than silently dropping it (matches the actor's
    // `identity::sign_unsigned_with_keys` behaviour — same correctness bar).
    let mut tags = Vec::with_capacity(unsigned.tags.len());
    for t in &unsigned.tags {
        match Tag::parse(t) {
            Ok(tag) => tags.push(tag),
            Err(_) => return Err("malformed tag in kind:9734".to_string()),
        }
    }
    let created_at = if unsigned.created_at == 0 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    } else {
        unsigned.created_at
    };

    let event = EventBuilder::new(kind, &unsigned.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(created_at))
        .sign_with_keys(keys)
        .map_err(|e| format!("sign failed: {e}"))?;
    serde_json::to_string(&event).map_err(|e| format!("serialise: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::nmp_app_free;
    use nmp_core::nmp_app_new;
    use std::ffi::{CStr, CString};

    /// THE NIP-57 DISPATCH WIRING PROOF: after `register_nip57_actions`, the
    /// `nmp.zap` namespace is reachable through the generic `dispatch_action`
    /// path. A well-formed `ZapAction` yields a 32-hex `correlation_id`
    /// (proves BOTH the typed validator AND the executor are wired); a
    /// malformed body is rejected with `error`.
    ///
    /// The LNURL worker thread is not exercised here — the test runs without
    /// a network and would have nothing useful to assert against
    /// `127.0.0.1:0` from this distance. The `lnurl.rs` mock-server tests
    /// cover that path end-to-end inside `nmp-nip57`.
    #[test]
    fn nip57_zap_dispatches_through_action_registry() {
        let app = nmp_app_new();
        // SAFETY: `app` is a valid pointer from `nmp_app_new`. We hold the
        // exclusive borrow only across the registration call.
        register_nip57_actions(unsafe { &mut *app });

        let recipient = "a".repeat(64);
        let zapped = "b".repeat(64);
        let body = format!(
            r#"{{"Zap":{{"zapped_event_id":"{zapped}","recipient_pubkey":"{recipient}","amount_sats":21,"lnurl":"https://ln.example/.well-known/lnurlp/alice","relays":["wss://relay.example"],"comment":"nice"}}}}"#
        );
        let ns = CString::new("nmp.zap").unwrap();
        let body_c = CString::new(body).unwrap();
        let ptr = nmp_core::nmp_app_dispatch_action(app, ns.as_ptr(), body_c.as_ptr());
        assert!(!ptr.is_null(), "dispatch_action must never return null");
        // SAFETY: ptr is a valid C string from `nmp_app_dispatch_action`.
        let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        nmp_core::nmp_app_free_string(ptr);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        let id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or_else(|| panic!("expected correlation_id, got {parsed}"));
        assert_eq!(id.len(), 32, "correlation id should be 32 hex");

        // Malformed shape (missing required fields) is rejected by the typed
        // validator surfaced through the host seam (D6).
        let bad = CString::new(r#"{"Zap":{"bad":"shape"}}"#).unwrap();
        let ptr = nmp_core::nmp_app_dispatch_action(app, ns.as_ptr(), bad.as_ptr());
        assert!(!ptr.is_null());
        // SAFETY: same as above.
        let out = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_owned();
        nmp_core::nmp_app_free_string(ptr);
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(
            parsed.get("error").is_some(),
            "malformed body must be rejected: {parsed}"
        );

        nmp_app_free(app);
    }

    /// `sign_zap_request` produces a canonical NIP-01 signed event JSON from
    /// the unsigned event + local keys. The output decodes back into a
    /// `nostr::Event` whose id verifies — proves the LNURL POST body would
    /// be a real signed event the LN provider can validate.
    #[test]
    fn sign_zap_request_produces_valid_signed_event() {
        let keys = Keys::generate();
        let unsigned = nmp_core::substrate::UnsignedEvent {
            pubkey: String::new(),
            kind: 9734,
            tags: vec![
                vec!["relays".to_string(), "wss://relay.example".to_string()],
                vec!["amount".to_string(), "21000".to_string()],
                vec!["p".to_string(), "a".repeat(64)],
                vec!["e".to_string(), "b".repeat(64)],
            ],
            content: "great post".to_string(),
            created_at: 0,
        };
        let json = sign_zap_request(&unsigned, &keys).expect("sign should succeed");
        let event: nostr::Event = serde_json::from_str(&json).expect("decodes to nostr::Event");
        assert_eq!(event.kind, Kind::from_u16(9734));
        assert_eq!(event.pubkey, keys.public_key());
        // `verify()` checks both the event-id hash and the Schnorr signature
        // — the same verification any LN provider would run on the kind:9734
        // before embedding it in the kind:9735 receipt.
        event.verify().expect("event must verify");
    }

    /// `sign_zap_request` rejects an unsigned event with a malformed tag
    /// rather than silently dropping it — same correctness bar as the
    /// actor's `identity::sign_unsigned_with_keys`.
    #[test]
    fn sign_zap_request_rejects_malformed_tag() {
        let keys = Keys::generate();
        let unsigned = nmp_core::substrate::UnsignedEvent {
            pubkey: String::new(),
            kind: 9734,
            tags: vec![vec![]], // empty tag — Tag::parse rejects
            content: String::new(),
            created_at: 0,
        };
        assert!(sign_zap_request(&unsigned, &keys).is_err());
    }
}
