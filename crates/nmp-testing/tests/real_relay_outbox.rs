//! Scenario 2 — NIP-65 outbox routing against a REAL author's live kind:10002.
//!
//! `real_relay_smoke::outbox_resolves_to_kind10002_writes` proves the
//! `Nip65OutboxResolver` decision logic, but it does so against a *synthetic*
//! kind:10002 we sign ourselves. That does not prove the resolver routes a
//! publish to the relays a real, well-known author actually declared on the
//! live network. This scenario closes that gap: fetch a stable author's live
//! kind:10002 over a real socket, feed it through the real store + resolver,
//! and assert the resolved relay set is *exactly* that author's declared
//! write-relay set — i.e. NIP-65 outbox routing, not the indexer fallback.
//!
//! Honest-validation: if no candidate author yields a usable kind:10002 (≥1
//! write `r`-tag) from any candidate relay within budget, this writes a SKIP
//! finding and pass-but-skips. It never fabricates a green assertion.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_outbox -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use common::{
    report_page, send_text, try_open, write_report, Verdict, DAMUS_RELAY, NOS_LOL, NOSTR_BAND,
    PRIMAL_RELAY,
};
use nmp_core::publish::{OutboxResolver, PublishTarget};
use nmp_core::store::{EventStore, MemEventStore, RawEvent, VerifiedEvent};
// Spec §271 (2026-05-25): `Nip65OutboxResolver` was moved from
// `nmp_core::publish::nip65` into `nmp-router`.
use nmp_router::Nip65OutboxResolver;
use serde_json::Value;

/// Per (author, relay) fetch budget. Short so a relay that does not hold the
/// listing does not burn the whole run.
const FETCH_BUDGET: Duration = Duration::from_secs(10);

/// Stable, well-known authors. First whose kind:10002 yields ≥1 write `r`-tag
/// from a reachable relay wins. `(label, hex_pubkey)`.
const AUTHORS: &[(&str, &str)] = &[
    ("jb55", "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2"),
    ("fiatjaf", "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"),
    ("hodlbod", "97c70a44366a6535c145b333f973ea86dfdc2d7a99da618c40c64705ad98e322"),
];

/// Relays to try, in order, for the kind:10002 REQ.
const RELAYS: &[&str] = &[DAMUS_RELAY, PRIMAL_RELAY, NOSTR_BAND, NOS_LOL];

/// A live kind:10002 captured from a real relay, plus its parsed write-set.
struct LiveListing {
    author_label: &'static str,
    author_hex: String,
    relay_used: &'static str,
    /// Raw `{event}` JSON of the kind:10002 (insertable as a `RawEvent`).
    event_json: String,
    /// Write relays per NIP-65: `r`-tags with marker `"write"` or no marker.
    write_set: BTreeSet<String>,
}

/// Parse an `["EVENT", subid, {event}]` text frame. Returns the kind:10002
/// event object iff it matches `sub_id`, is kind 10002, authored by
/// `author_hex`, and is structurally a signed event.
fn parse_kind10002(text: &str, sub_id: &str, author_hex: &str) -> Option<Value> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "EVENT" {
        return None;
    }
    if arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?.as_object()?;
    if ev.get("kind")?.as_u64()? != 10002 {
        return None;
    }
    if ev.get("pubkey")?.as_str()? != author_hex {
        return None;
    }
    let id = ev.get("id")?.as_str()?;
    let sig = ev.get("sig")?.as_str()?;
    let ok = id.len() == 64
        && id.chars().all(|c| c.is_ascii_hexdigit())
        && sig.len() == 128
        && sig.chars().all(|c| c.is_ascii_hexdigit())
        && ev.get("created_at")?.as_u64().is_some();
    if ok {
        Some(arr.get(2)?.clone())
    } else {
        None
    }
}

/// Extract the NIP-65 write-relay set from a kind:10002 event object: `r`-tags
/// whose marker is `"write"`, or absent/empty (unmarked ⇒ both). Mirrors the
/// resolver's own parsing so set-equality is a meaningful discriminator.
fn write_relays(ev: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(tags) = ev.get("tags").and_then(Value::as_array) else {
        return out;
    };
    for tag in tags {
        let Some(parts) = tag.as_array() else { continue };
        if parts.first().and_then(Value::as_str) != Some("r") {
            continue;
        }
        let Some(url) = parts.get(1).and_then(Value::as_str) else {
            continue;
        };
        if !(url.starts_with("wss://") || url.starts_with("ws://")) {
            continue;
        }
        match parts.get(2).and_then(Value::as_str) {
            Some("write") | None | Some("") => {
                out.insert(url.to_string());
            }
            Some("read") => {}
            // Unknown marker ⇒ both (resolver mirrors this tolerant parse).
            Some(_) => {
                out.insert(url.to_string());
            }
        }
    }
    out
}

/// Try every (author, relay) pair until one yields a usable kind:10002.
fn fetch_live_listing(attempted: &mut Vec<String>) -> Option<LiveListing> {
    for (label, hex) in AUTHORS {
        for relay in RELAYS {
            let pair = format!("{label}@{relay}");
            attempted.push(pair.clone());

            let Some(mut socket) = try_open(relay) else {
                continue;
            };
            let sub_id = format!("rr-outbox-{}", common::now_ms());
            let req = format!(
                "[\"REQ\",\"{sub_id}\",{{\"authors\":[\"{hex}\"],\"kinds\":[10002],\"limit\":1}}]"
            );
            if send_text(&mut socket, req).is_err() {
                eprintln!("SKIP: {pair}: REQ send failed");
                let _ = socket.close(None);
                continue;
            }

            let deadline = Instant::now() + FETCH_BUDGET;
            let mut captured: Option<Value> = None;
            common::drain_until(&mut socket, deadline, |text| {
                if let Some(ev) = parse_kind10002(text, &sub_id, hex) {
                    captured = Some(ev);
                    return true;
                }
                // EOSE for our sub ⇒ relay has nothing; stop waiting on it.
                if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(text) {
                    if a.first().and_then(Value::as_str) == Some("EOSE")
                        && a.get(1).and_then(Value::as_str) == Some(sub_id.as_str())
                    {
                        return true;
                    }
                }
                false
            });
            let _ = send_text(&mut socket, format!("[\"CLOSE\",\"{sub_id}\"]"));
            let _ = socket.close(None);

            let Some(ev) = captured else {
                eprintln!("[outbox] {pair}: no kind:10002 within {FETCH_BUDGET:?}");
                continue;
            };
            let write_set = write_relays(&ev);
            if write_set.is_empty() {
                eprintln!("[outbox] {pair}: kind:10002 had zero write r-tags");
                continue;
            }
            return Some(LiveListing {
                author_label: label,
                author_hex: (*hex).to_string(),
                relay_used: relay,
                event_json: ev.to_string(),
                write_set,
            });
        }
    }
    None
}

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn outbox_routes_real_author_kind10002_writes() {
    let mut attempted: Vec<String> = Vec::new();

    let Some(listing) = fetch_live_listing(&mut attempted) else {
        let body = format!(
            "No candidate author yielded a usable kind:10002 (≥1 write \
             `r`-tag) from any candidate relay within {FETCH_BUDGET:?} per \
             pair.\n\n\
             Authors tried: {authors:?}.\n\
             Relays tried: {relays:?}.\n\
             Pairs attempted (in order): {attempted:?}.\n\n\
             This is a SKIP, not a pass: either the public relay set was \
             unreachable from this host/network, or none held a kind:10002 \
             for these authors. Re-run with network access; if it persists, \
             revisit the candidate author/relay lists.",
            authors = AUTHORS.iter().map(|(l, _)| *l).collect::<Vec<_>>(),
            relays = RELAYS,
        );
        write_report(
            "scenario2-outbox",
            &report_page(
                "Scenario 2 — NIP-65 outbox routing vs real kind:10002",
                "2-outbox-real-kind10002",
                Verdict::Skip,
                RELAYS,
                &body,
            ),
        );
        eprintln!("SKIP: scenario 2 — no usable real kind:10002 from any candidate");
        return;
    };

    // Feed the REAL event through the REAL store + resolver.
    let raw: RawEvent =
        serde_json::from_str(&listing.event_json).expect("RawEvent decode of live kind:10002");
    let verified = VerifiedEvent::try_from_raw(raw).expect("verify live kind:10002 signature");
    let store: Arc<dyn EventStore> = Arc::new(MemEventStore::new());
    store
        .insert(verified, &"wss://fetch".to_string(), common::now_ms() as u64)
        .expect("insert live kind:10002");

    let resolver = Nip65OutboxResolver::with_default_fallback(store);
    let resolved: BTreeSet<String> = resolver
        .resolve(&listing.author_hex, &[], &PublishTarget::Auto, 1)
        .into_iter()
        .map(|r| r.url)
        .collect();

    // Core assertion: the resolver routed to EXACTLY the author's declared
    // write-relay set. Set-equality is the discriminator that proves the
    // indexer fallback did NOT fire — fallback would extend a different set
    // (it only fires when the write-set is empty, and even when it overlaps
    // a write URL the resulting set differs from the pure write-set).
    assert_eq!(
        resolved, listing.write_set,
        "resolver must route to the author's exact declared write-relays \
         (NIP-65 outbox), not the indexer fallback.\n  resolved={resolved:?}\n  \
         declared_writes={:?}",
        listing.write_set
    );

    // Belt-and-suspenders: any default-fallback URL the author did NOT
    // declare must be absent. (A fallback URL the author *did* declare is
    // legitimately present — that is why set-equality above is the real
    // proof, and why this check skips overlapping URLs.)
    for fb in ["wss://relay.damus.io", "wss://nos.lol"] {
        if !listing.write_set.contains(fb) {
            assert!(
                !resolved.contains(fb),
                "indexer fallback `{fb}` leaked into a NIP-65-routed publish"
            );
        }
    }

    let writes_md = listing
        .write_set
        .iter()
        .map(|r| format!("- `{r}`"))
        .collect::<Vec<_>>()
        .join("\n");
    let body = format!(
        "Fetched author **{label}** (`{hex}`) live kind:10002 from \
         `{relay}`, inserted the real signed event into a `MemEventStore`, \
         and resolved `PublishTarget::Auto` through \
         `Nip65OutboxResolver::with_default_fallback`.\n\n\
         The resolved relay set is **exactly** the author's declared \
         write-relay set ({n} relay(s)) — proving NIP-65 outbox routing \
         against live network data, not the indexer fallback.\n\n\
         - author: `{hex}` ({label})\n\
         - source relay: `{relay}`\n\
         - declared write-relays:\n{writes_md}\n\
         - resolved == declared write-set: ✅ (BTreeSet equality)\n\
         - indexer fallback (non-overlapping URLs) absent: ✅\n",
        label = listing.author_label,
        hex = listing.author_hex,
        relay = listing.relay_used,
        n = listing.write_set.len(),
    );
    write_report(
        "scenario2-outbox",
        &report_page(
            "Scenario 2 — NIP-65 outbox routing vs real kind:10002",
            "2-outbox-real-kind10002",
            Verdict::Pass,
            &[listing.relay_used],
            &body,
        ),
    );
    println!(
        "[outbox] PASS via {} for {} ({}): resolved {:?}",
        listing.relay_used, listing.author_label, listing.author_hex, resolved
    );
}
