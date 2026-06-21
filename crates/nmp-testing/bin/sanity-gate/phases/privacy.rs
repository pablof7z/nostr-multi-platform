//! Robustness family 3 — PRIVACY / SECURITY (catastrophic-if-wrong).
//!
//! aim.md §6 doctrine: provenance preserved — private events are NEVER
//! republished; the live wire path NEVER accepts an unverified event.
//!
//! Falsifiable hypotheses (a FAIL here is catastrophic, not cosmetic):
//!  - GIFT-WRAP NEVER REPUBLISHED: drive a real NIP-17 DM (kind:1059 gift-wrap)
//!    via the Chirp `nmp.nip17.send` action; read the routing-decisions ledger
//!    and assert NO kind:1059 publish targets a configured PUBLIC relay. A 1059
//!    routed to a public relay is a DM leak — instant FAIL. (#1518 provenance.)
//!  - UNVERIFIED EVENTS REJECTED: the live ingest path runs full Schnorr+id
//!    verification. We tamper a valid event's signature and inject it; the seam
//!    MUST reject it (return false) and the kind:1 counter MUST NOT move.
//!  - PRE-VERIFIED BYPASS IS TEST-GATED: the `from_raw_unchecked` /
//!    inject-pre-verified path is `cfg(any(test, feature = "test-support"))`
//!    only — recorded as a compile-time invariant row.

use std::time::Duration;

use nostr::{EventBuilder, JsonUtil, Keys, Timestamp};

use crate::config::{Args, Phase};
use crate::report::{GateRow, SanityReport, Verdict};

/// NIP-17 gift-wrap kind.
const GIFT_WRAP_KIND: u64 = 1059;

pub fn run_privacy(report: &mut SanityReport, args: &Args) {
    let phase = Phase::Privacy.as_str();
    unverified_rejected(report, phase, args);
    pre_verified_bypass_gated(report, phase);
    giftwrap_no_public_republish(report, phase, args);
}

/// UNVERIFIED EVENTS REJECTED — tamper a signature; the ingest seam must reject.
fn unverified_rejected(report: &mut SanityReport, phase: &str, args: &Args) {
    let Some(app) = super::connect_or_skip_optional(report, phase, args) else {
        return;
    };
    let keys = Keys::generate();
    let valid: nostr::Event = match EventBuilder::text_note("privacy-oracle tamper target")
        .custom_created_at(Timestamp::from(crate::report::now_unix()))
        .sign_with_keys(&keys)
    {
        Ok(e) => e,
        Err(_) => return,
    };
    // Flip one hex nibble of the signature so id-hash still parses but Schnorr
    // verification fails (the event JSON is otherwise well-formed).
    let mut json = valid.as_json();
    let tampered = tamper_sig(&valid.as_json()).unwrap_or_else(|| json.clone());
    json = tampered;

    let notes_before = app.with_state(|s| s.latest().map(|r| r.note_events).unwrap_or(0));
    let accepted = std::ffi::CString::new(json.as_str())
        .map(|c| nmp_ffi::nmp_app_inject_signed_event_json(app.raw(), c.as_ptr()))
        .unwrap_or(false);
    let _ = app.wait_until(Duration::from_secs(1), |s| {
        s.latest().map(|r| r.note_events).unwrap_or(0) > notes_before
    });
    let notes_after = app.with_state(|s| s.latest().map(|r| r.note_events).unwrap_or(0));

    // PASS iff the seam returned false AND the counter did not move.
    let leaked_in =
        if accepted { 1.0 } else { 0.0 } + (notes_after.saturating_sub(notes_before)) as f64;
    report.push(
        GateRow::max(
            "privacy-unverified-rejected",
            phase,
            "nmp_app_inject_signed_event_json (full Schnorr+id verify path)",
            "VerifiedEvent::try_from_raw rejection + note_events delta",
            leaked_in,
            0.0,
            "accepted-unverified",
        )
        .with_note(&format!(
            "tampered-signature event: inject returned accepted={accepted} (must be false), \
             note_events {notes_before}→{notes_after} (must not move)"
        )),
    );
}

/// PRE-VERIFIED BYPASS IS TEST-GATED — REAL compile-time source check.
fn pre_verified_bypass_gated(report: &mut SanityReport, phase: &str) {
    // FALSIFIABLE source-level assertion (was a hardcoded 1.0 that would still
    // PASS even if the cfg gate were deleted). We embed the ACTUAL nmp-ffi
    // source at compile time via `include_str!` and assert the test-support cfg
    // gate + the bypass symbol are present. If a future change removed the
    // `#![cfg(any(test, feature = "test-support"))]` gate (or ungated `mod
    // testing;` in lib.rs) — the exact regression this row guards — the embedded
    // source no longer contains the gate string and THIS gate FAILS.
    const TESTING_SRC: &str = include_str!("../../../../nmp-ffi/src/testing.rs");
    const LIB_SRC: &str = include_str!("../../../../nmp-ffi/src/lib.rs");

    let module_inner_gate = TESTING_SRC.contains("#![cfg(any(test, feature = \"test-support\"))]");
    let bypass_symbol =
        TESTING_SRC.contains("pub extern \"C\" fn nmp_app_inject_pre_verified_events");
    let unchecked_path = TESTING_SRC.contains("from_raw_unchecked");
    // The `mod testing;` declaration in lib.rs must itself be cfg-gated.
    let mod_decl_gated = LIB_SRC.contains("#[cfg(any(test, feature = \"test-support\"))]")
        && LIB_SRC.contains("mod testing;");

    let checks = [
        ("module #![cfg] gate on testing.rs", module_inner_gate),
        ("nmp_app_inject_pre_verified_events present", bypass_symbol),
        ("from_raw_unchecked bypass path present", unchecked_path),
        ("`mod testing;` cfg-gated in lib.rs", mod_decl_gated),
    ];
    let failed: Vec<&str> = checks
        .iter()
        .filter(|(_, ok)| !ok)
        .map(|(n, _)| *n)
        .collect();

    report.push(
        GateRow::max(
            "privacy-pre-verified-bypass-test-gated",
            phase,
            "include_str!(nmp-ffi/src/{testing,lib}.rs) compile-time source check",
            "cfg(any(test, feature=\"test-support\")) gate on the inject-pre-verified bypass",
            failed.len() as f64,
            0.0,
            "ungated-bypass-sites",
        )
        .with_note(&format!(
            "REAL source check (replaces the prior hardcoded 1.0): {}/{} guards satisfied — the \
             nmp_app_inject_pre_verified_events / from_raw_unchecked bypass is behind \
             #![cfg(any(test, feature=\"test-support\"))] and `mod testing;` is cfg-gated in \
             lib.rs. Deleting the gate makes this gate FAIL (falsifiable). failed={:?}",
            checks.len() - failed.len(),
            checks.len(),
            failed,
        )),
    );
}

/// GIFT-WRAP NEVER REPUBLISHED — drive a real NIP-17 DM, assert no kind:1059
/// publish targets a configured public relay.
fn giftwrap_no_public_republish(report: &mut SanityReport, phase: &str, args: &Args) {
    let Some(app) = super::connect_or_skip(report, phase, args) else {
        return;
    };
    // Treat the configured relay as a PUBLIC relay (DrivenApp adds it as
    // read+write). A correct NIP-17 send routes the gift-wrap to the
    // recipient's DM inbox relays (kind:10050) — NEVER to this public relay.
    let public_relay = normalize_url(&args.relay);

    // Drive a real send to a fresh recipient (no kind:10050 → must NOT leak to
    // the public relay even though no inbox is known).
    let recipient = Keys::generate().public_key().to_hex();
    let body = serde_json::json!({
        "recipient_pubkey": recipient,
        "content": "privacy-oracle DM — must never reach a public relay",
        "reply_to": serde_json::Value::Null,
    })
    .to_string();
    let ns = std::ffi::CString::new("nmp.nip17.send").unwrap();
    let body_c = std::ffi::CString::new(body).unwrap();
    let ret = nmp_ffi::nmp_app_dispatch_action(app.raw(), ns.as_ptr(), body_c.as_ptr());
    let dispatch_result = if ret.is_null() {
        None
    } else {
        let parsed = unsafe { std::ffi::CStr::from_ptr(ret) }
            .to_str()
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
        nmp_ffi::nmp_free_string(ret);
        parsed
    };
    let Some(correlation_id) = dispatch_result
        .as_ref()
        .and_then(|v| v.get("correlation_id"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
    else {
        report.push(GateRow::unmeasured(
            "privacy-giftwrap-no-public-republish",
            phase,
            "nmp.nip17.send action dispatch",
            "dispatch return correlation_id",
            "accepted action with a correlation id",
            Verdict::Blocked,
            &format!(
                "nmp.nip17.send did not return a correlation_id; dispatch_result={dispatch_result:?} \
                 — cannot wait for action_results before reading the routing ledger"
            ),
        ));
        return;
    };

    if !app.wait_for_action_terminal(&correlation_id, Duration::from_secs(8)) {
        report.push(GateRow::unmeasured(
            "privacy-giftwrap-no-public-republish",
            phase,
            "nmp.nip17.send action + action_results typed projection",
            "action_results terminal before routing-ledger read",
            "DM send action terminal observed",
            Verdict::Blocked,
            &format!(
                "no action_results terminal appeared for correlation_id={correlation_id} after \
                 the DM send action — cannot assert the routing ledger without risking a stale read"
            ),
        ));
        return;
    }
    let wrap_publishes = app
        .routing_decisions()
        .map(|d| scan_giftwrap_publishes(&d))
        .unwrap_or_default();

    if wrap_publishes.is_empty() {
        report.push(GateRow::unmeasured(
            "privacy-giftwrap-no-public-republish",
            phase,
            "nmp.nip17.send action + nmp_app_recent_routing_decisions",
            "routing-trace publishes[kind=1059].urls",
            "no kind:1059 publish targets a public relay",
            Verdict::SkipRelayMiss,
            &format!(
                "no kind:1059 gift-wrap publish appeared in the routing ledger after actor \
                 action terminal correlation_id={correlation_id} settled; the recipient has no \
                 kind:10050 inbox relay, \
                 so the send was held rather than routed — the catastrophic leak path did not \
                 fire, but a positive route was not produced. Provide a recipient with a \
                 published kind:10050 inbox to drive a positive assertion. SKIP LOUD."
            ),
        ));
        return;
    }

    // Any 1059 publish whose targets include the public relay is a leak.
    let leaks: Vec<String> = wrap_publishes
        .iter()
        .flat_map(|(_, urls)| urls.iter())
        .filter(|u| normalize_url(u) == public_relay)
        .cloned()
        .collect();
    report.push(
        GateRow::max(
            "privacy-giftwrap-no-public-republish",
            phase,
            "nmp.nip17.send action + nmp_app_recent_routing_decisions",
            "routing-trace publishes[kind=1059] targeting a public relay",
            leaks.len() as f64,
            0.0,
            "public-leaks",
        )
        .with_note(&format!(
            "{} kind:1059 publish(es) observed; public relay={public_relay}; leaked-targets={:?} \
             (any non-empty value is a catastrophic DM leak)",
            wrap_publishes.len(),
            leaks
        )),
    );
}

/// Scan the routing-decisions JSON for `publishes[]` of kind 1059, returning
/// `(event_id_short, target_urls)` for each.
fn scan_giftwrap_publishes(d: &serde_json::Value) -> Vec<(String, Vec<String>)> {
    d.get("publishes")
        .and_then(|p| p.as_array())
        .map(|arr| {
            arr.iter()
                .filter(|p| p.get("kind").and_then(|k| k.as_u64()) == Some(GIFT_WRAP_KIND))
                .map(|p| {
                    let id = p
                        .get("event_id_short")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let urls = p
                        .get("urls")
                        .and_then(|u| u.as_array())
                        .map(|a| {
                            a.iter()
                                .filter_map(|e| {
                                    e.get("url").and_then(|v| v.as_str()).map(str::to_string)
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    (id, urls)
                })
                .collect()
        })
        .unwrap_or_default()
}

/// Flip one nibble of the 128-hex `sig` field so Schnorr verification fails.
fn tamper_sig(json: &str) -> Option<String> {
    let mut v: serde_json::Value = serde_json::from_str(json).ok()?;
    let sig = v.get("sig")?.as_str()?.to_string();
    let mut chars: Vec<char> = sig.chars().collect();
    if let Some(c) = chars.first_mut() {
        *c = if *c == 'a' { 'b' } else { 'a' };
    }
    v["sig"] = serde_json::Value::String(chars.into_iter().collect());
    serde_json::to_string(&v).ok()
}

/// Canonicalise a relay URL for comparison (trim a single trailing slash).
fn normalize_url(url: &str) -> String {
    url.trim_end_matches('/').to_string()
}
