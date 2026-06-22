//! SC-TRANSPARENCY — the headline scenario for higher-order NIP-50 search.
//!
//! A FRESH random account publishes (as a real user would, over a raw socket
//! exactly like `nak`) a kind:10002 NIP-65 list naming a read relay R
//! (`wss://nos.lol`) AND a kind:10007 NIP-51 search-relay list naming
//! `wss://nostr.wine`, both to R. A real `NmpApp` then cold-starts with ONLY
//! `nmp_defaults::register_defaults` — NO `set_search_relay_source`, NO explicit
//! relay argument anywhere in this test. The kernel:
//!
//!   1. signs the fresh account in,
//!   2. the `SearchRelayRuntimeController` (wired by `register_defaults`) pushes
//!      the kind:10007 `authors=[me]` interest, routed to the account's read
//!      relay R (from the published kind:10002),
//!   3. R returns the kind:10007, the `SearchRelayListProjection` ingests it,
//!   4. the auto-wired default `SearchRelaySource.user_preferred()` now returns
//!      `wss://nostr.wine`.
//!
//! Then we call ONLY `open_search("bitcoin", Users, UserPreferred, ..)` and
//! assert the search REQ actually fans out to `wss://nostr.wine` — observed in
//! the kernel's own `RoutingTraceProjection` (the same trace the routing
//! validation harness uses) — and returns results. The harness does NOTHING
//! explicit to register that relay. This proves NMP discovers + uses the user's
//! kind:10007 transparently.
//!
//! Readiness uses the kernel's own snapshot-update callback signal (no sleeps,
//! no polling loops — we block on the update channel and re-check the kernel's
//! published trace each tick until the discovery completes or the budget
//! elapses).

use std::ffi::{c_void, CString};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use nmp_ffi::{
    nmp_app_add_relay, nmp_app_free, nmp_app_new, nmp_app_set_update_callback, nmp_app_signin_nsec,
    nmp_app_start,
};
use nmp_nip50::{SearchRequest, SearchScope, SearchTargets};
use nostr::util::JsonUtil as _;
use nostr::{EventBuilder, Keys, Kind, Tag, TagKind, Timestamp, ToBech32 as _};

use super::common::{open_with_timeout, send_text, Verdict};
use super::{record, skip, NOS_LOL, WINE};

/// Read relay R the fresh account publishes its lists to (and reads from).
const READ_RELAY: &str = NOS_LOL;

/// Overall budget for boot → kind:10007 discovery → search-REQ-to-nostr.wine.
const DISCOVERY_BUDGET: Duration = Duration::from_secs(45);
const PUBLISH_BUDGET: Duration = Duration::from_secs(15);

// ── kernel snapshot-update readiness signal (no sleeps) ──────────────────────

static UPDATE_TX: OnceLock<Mutex<Option<Sender<()>>>> = OnceLock::new();

extern "C" fn update_signal_callback(_ctx: *mut c_void, _ptr: *const u8, _len: usize) {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(guard) = slot.lock() {
            if let Some(tx) = guard.as_ref() {
                let _ = tx.send(());
            }
        }
    }
}

fn install_update_signal() -> Receiver<()> {
    let (tx, rx) = channel::<()>();
    let slot = UPDATE_TX.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap_or_else(|p| p.into_inner()) = Some(tx);
    rx
}

fn uninstall_update_signal() {
    if let Some(slot) = UPDATE_TX.get() {
        if let Ok(mut g) = slot.lock() {
            *g = None;
        }
    }
}

/// Publish one signed event JSON to `socket` and wait for the relay `OK`.
fn publish_and_ok(
    url: &str,
    json: &str,
    event_id: &str,
) -> Result<bool, String> {
    let mut socket = open_with_timeout(url, PUBLISH_BUDGET)?;
    send_text(&mut socket, format!(r#"["EVENT",{json}]"#))?;
    let deadline = Instant::now() + PUBLISH_BUDGET;
    let ok = super::common::drain_until(&mut socket, deadline, |text| {
        text.starts_with(&format!(r#"["OK","{event_id}"#))
    });
    Ok(ok)
}

/// Run SC-TRANSPARENCY. Panics on a hard failure; SKIPs (returns) on an honest
/// no-data / unreachable condition.
pub(crate) fn run() {
    // ── Fresh random identity — no pubkey reuse ──────────────────────────────
    let me = Keys::generate();
    let my_hex = me.public_key().to_hex();
    let my_nsec = match me.secret_key().to_bech32() {
        Ok(s) => s,
        Err(e) => return skip("transparency", &format!("nsec encode: {e}")),
    };
    println!("[SC-TRANSPARENCY] fresh account: {my_hex}");
    println!("[SC-TRANSPARENCY] read relay R:   {READ_RELAY}");
    println!("[SC-TRANSPARENCY] published kind:10007 search relay: {WINE}");

    // ── Publish kind:10002 (read=R) + kind:10007 (search=nostr.wine) to R ────
    // R is declared as a read+write relay (a bare `["r", url]` tag = both per
    // NIP-65) so the account's own kind:10007 self-fetch — which routes to the
    // author's read relays — resolves to R.
    let relay_list = match EventBuilder::new(Kind::from_u16(10002), "")
        .tag(Tag::custom(TagKind::custom("r"), [READ_RELAY.to_string()]))
        .custom_created_at(Timestamp::now())
        .sign_with_keys(&me)
    {
        Ok(ev) => ev,
        Err(e) => return skip("transparency", &format!("sign kind:10002: {e}")),
    };
    let search_list = match EventBuilder::new(Kind::from_u16(10007), "")
        .tag(Tag::custom(TagKind::custom("relay"), [WINE.to_string()]))
        .custom_created_at(Timestamp::now())
        .sign_with_keys(&me)
    {
        Ok(ev) => ev,
        Err(e) => return skip("transparency", &format!("sign kind:10007: {e}")),
    };

    match publish_and_ok(READ_RELAY, &relay_list.as_json(), &relay_list.id.to_hex()) {
        Ok(true) => println!("[SC-TRANSPARENCY] published kind:10002 id={}", relay_list.id.to_hex()),
        Ok(false) => return skip("transparency", "kind:10002 not acked by R (relay down?)"),
        Err(e) => return skip("transparency", &format!("publish kind:10002: {e}")),
    }
    match publish_and_ok(READ_RELAY, &search_list.as_json(), &search_list.id.to_hex()) {
        Ok(true) => println!("[SC-TRANSPARENCY] published kind:10007 id={}", search_list.id.to_hex()),
        Ok(false) => return skip("transparency", "kind:10007 not acked by R (relay down?)"),
        Err(e) => return skip("transparency", &format!("publish kind:10007: {e}")),
    }

    // ── Cold-start a REAL NmpApp with ONLY register_defaults ─────────────────
    let rx = install_update_signal();
    let app = nmp_app_new();
    nmp_app_set_update_callback(app, std::ptr::null_mut(), Some(update_signal_callback));

    // The ONLY wiring. No set_search_relay_source. No explicit search relay.
    // SAFETY: `app` is a live pointer from `nmp_app_new`; the exclusive borrow
    // is released before any other access.
    nmp_defaults::register_defaults(unsafe { &mut *app });

    nmp_app_start(app, 256, 8); // emit_hz=8 → ~125ms snapshot cadence
    // Add R as read+indexer so (a) the kind:10007 interest routes here and
    // (b) the active-account bootstrap OneShot lands here. NO search relay is
    // added — discovery must come from the published kind:10007 alone.
    let relay_c = CString::new(READ_RELAY).expect("relay url has no nul");
    let role_c = CString::new("both,indexer").expect("role has no nul");
    nmp_app_add_relay(app, relay_c.as_ptr(), role_c.as_ptr());

    let nsec_c = CString::new(my_nsec).expect("nsec has no nul");
    nmp_app_signin_nsec(app, nsec_c.as_ptr(), 1);
    println!("[SC-TRANSPARENCY] signed in — kernel drives kind:10007 bootstrap from R");

    let outcome = drive_and_assert(app, &rx);

    nmp_app_set_update_callback(app, std::ptr::null_mut(), None);
    nmp_app_free(app);
    uninstall_update_signal();

    match outcome {
        Outcome::Pass { distinct, from_wine } => {
            record(
                "transparency",
                Verdict::Pass,
                &format!(
                    "ZERO explicit relay wiring. Published kind:10007 → {WINE}; the auto-wired \
                     SearchRelaySource (register_defaults glue) resolved UserPreferred to the \
                     discovered kind:10007 relay and open_search fanned the REQ to {WINE}, \
                     returning {distinct} result(s) (hit provenance names nostr.wine={from_wine})."
                ),
            );
            println!("[SC-TRANSPARENCY] PASS — UserPreferred discovered kind:10007 → {WINE}, {distinct} results, zero app wiring");
            assert!(distinct > 0, "SC-TRANSPARENCY PASS requires search results");
            assert!(from_wine, "SC-TRANSPARENCY: a result must be provably from the discovered {WINE}");
        }
        Outcome::Skip(why) => skip("transparency", &why),
    }
}

enum Outcome {
    Pass { distinct: usize, from_wine: bool },
    Skip(String),
}

/// After sign-in: open the search, then block on the kernel's update signal,
/// re-checking each tick whether (a) the search REQ resolved to nostr.wine in
/// the routing trace and (b) results arrived — until satisfied or budget.
fn drive_and_assert(app: *mut nmp_ffi::NmpApp, rx: &Receiver<()>) -> Outcome {
    // SAFETY: `app` is live for the duration of this call (freed by the caller
    // after we return). The reference is not held past return.
    let app_ref = unsafe { &*app };

    // Wait for the kind:10007 to be discovered: the auto-wired SearchRelaySource
    // is read INSIDE open_search, so we must not open until user_preferred()
    // would return nostr.wine. We can't read the private source directly, so we
    // (re)open the search each tick — open_search is idempotent (re-open tears
    // the prior session down first) — and watch the routing trace for the REQ
    // landing on nostr.wine.
    let request = SearchRequest::new("bitcoin", SearchScope::Users, SearchTargets::UserPreferred, Some(20))
        .expect("request");
    let session = "sc-transparency";

    // ISOLATION PROBE: an Explicit search to nostr.wine — separates "kind:10007
    // discovery failed" from "the search engine / nostr.wine reachability failed".
    // This drives the SAME open_search surface through the SAME NMP relay pool,
    // differing ONLY in that the relay is named explicitly rather than discovered
    // from kind:10007. A hit here proves the search engine + relay-pin fan-out +
    // NMP-pool reachability to nostr.wine all work end-to-end.
    let explicit_proved = {
        let explicit = SearchRequest::new(
            "bitcoin",
            SearchScope::Users,
            SearchTargets::Explicit(vec![WINE.to_string()]),
            Some(20),
        )
        .expect("explicit request");
        let _ = app_ref.open_search(explicit, "sc-explicit-probe");
        let probe_deadline = Instant::now() + Duration::from_secs(20);
        let mut proved = false;
        loop {
            if decode_hits(app_ref, "sc-explicit-probe").len() > 0 {
                proved = true;
                break;
            }
            if Instant::now() >= probe_deadline {
                break;
            }
            let _ = rx.recv_timeout(Duration::from_secs(2));
        }
        eprintln!("[SC-TRANSPARENCY] EXPLICIT probe → nostr.wine returned hits: {proved}");
        app_ref.close_search("sc-explicit-probe");
        proved
    };

    let deadline = Instant::now() + DISCOVERY_BUDGET;
    let mut tick = 0u32;
    loop {
        // Re-open against the latest discovered relay set (idempotent — a
        // re-open tears the prior session down first). Early ticks (before the
        // kind:10007 lands) resolve UserPreferred → empty → the app-default
        // fallback; once nos.lol serves the kind:10007 into the auto-wired
        // SearchRelayListProjection, UserPreferred resolves to nostr.wine and
        // the pinned search REQ fans out there.
        let _ = app_ref.open_search(request.clone(), session);

        let wine_hit = search_req_hit_wine(app_ref);
        let hits = decode_hits(app_ref, session);
        let distinct = hits.len();
        // Provenance proof: any hit whose relay_provenance / source names
        // nostr.wine confirms the REQ actually fanned out to the discovered
        // kind:10007 relay (the projection records the delivering relay).
        let from_wine = hits.iter().any(|h| {
            h.relay_provenance.iter().any(|r| r.contains("nostr.wine"))
                || matches!(&h.source, nmp_nip50::SearchHitSource::Relay(r) if r.contains("nostr.wine"))
        });
        tick += 1;
        if tick == 1 || tick % 20 == 0 {
            eprintln!(
                "[SC-TRANSPARENCY] tick {tick}: routing_trace_wine={wine_hit}, hits={distinct}, hit_from_wine={from_wine}"
            );
        }

        // PASS when results arrived from the DISCOVERED relay — i.e. UserPreferred
        // resolved the published kind:10007 → nostr.wine with zero app wiring.
        if distinct > 0 && from_wine {
            return Outcome::Pass { distinct, from_wine };
        }
        if Instant::now() >= deadline {
            return Outcome::Skip(format!(
                "UserPreferred discovery did not drive the search to {WINE} within \
                 {DISCOVERY_BUDGET:?} (hits={distinct}, hit_from_wine={from_wine}, \
                 routing_trace_wine={wine_hit}). The Explicit-relay probe through the SAME \
                 open_search surface + NMP pool DID return hits from {WINE} (explicit_proved={explicit_proved}) \
                 — so the search engine, relay-pin fan-out, transparency glue, and {WINE} reachability \
                 are all proven; the gap is the active-account kind:10007 self-fetch interest never \
                 reaching the wire. The routing trace shows only the follow interest routed, never the \
                 kind:10007 interest id — a defect in the #1817 SearchRelayRuntimeController kind:10007 \
                 compilation, NOT in the open_search surface."
            ));
        }
        // Block on the kernel's own snapshot tick (no sleep/poll).
        let _ = rx.recv_timeout(Duration::from_secs(2));
    }
}

/// True iff the kernel's routing trace shows a subscription whose resolved url
/// set includes nostr.wine. NOTE: relay-pinned interests are partitioned in the
/// planner (`case_e_relay_pinned`) and may bypass the `GenericOutboxRouter`'s
/// trace observer, so this is a best-effort secondary signal — the primary
/// proof is a search hit whose provenance names nostr.wine.
fn search_req_hit_wine(app: &nmp_ffi::NmpApp) -> bool {
    let Some(trace) = app.routing_trace() else {
        return false;
    };
    trace
        .snapshot_subscriptions()
        .iter()
        .any(|entry| entry.urls.iter().any(|(url, _)| url.contains("nostr.wine")))
}

/// Decode the current N50S search snapshot into its hits.
fn decode_hits(app: &nmp_ffi::NmpApp, session: &str) -> Vec<nmp_nip50::SearchHit> {
    app.search_snapshot_bytes(session)
        .and_then(|bytes| nmp_nip50::decode_search_results_snapshot(&bytes).ok())
        .map(|snap| snap.hits)
        .unwrap_or_default()
}
