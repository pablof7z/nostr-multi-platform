//! Scenario registry. Each scenario drives REAL NMP public APIs and asserts an
//! observable property of the LANDED architecture (ADR-0057 unified ingest
//! chokepoint, Workstream C publish-policy, PR2 profiles capability). Stubs for
//! not-yet-landed work (PR3 contacts, Workstream B acquisition, Workstream F
//! doctrine gates) print `SKIP (pending …)` so the coverage report is honest.
//!
//! Each landed scenario carries a `// MUTATION:` note describing the sabotage
//! to the landed code that should flip it red — the non-vacuity guard from the
//! catalog's §Non-vacuity.

use nostr::{Keys, Kind, Tag, Timestamp, UnsignedEvent};

use crate::harness::{
    build_signed_event, event_to_value, relay_pinned_interest, Harness,
};
use crate::{Outcome, ScenarioResult, WAIT};

/// Macro to reduce boilerplate building a `ScenarioResult`.
macro_rules! result {
    ($id:expr, $driver:expr, $title:expr, $outcome:expr) => {
        ScenarioResult {
            id: $id,
            title: $title,
            driver: $driver,
            outcome: $outcome,
        }
    };
}

fn ok() -> Outcome {
    Outcome::Pass
}
fn fail(why: impl Into<String>) -> Outcome {
    Outcome::Fail(why.into())
}
fn check(cond: bool, why: impl Into<String>) -> Outcome {
    if cond {
        Outcome::Pass
    } else {
        Outcome::Fail(why.into())
    }
}

pub fn run_all() -> Vec<ScenarioResult> {
    let mut out: Vec<ScenarioResult> = Vec::new();

    // ── Area 1 — read-your-writes ────────────────────────────────────────
    out.push(guard("A1.1", "local-publish", "RYW kind:1 note appears (store+observer) before relay ACK", a1_note_ryw));
    out.push(guard("A1.2", "local-publish", "RYW kind:6 repost appears before relay ACK", a1_repost_ryw));
    out.push(guard("A1.3", "local-publish", "RYW kind:7 reaction appears before relay ACK", a1_reaction_ryw));

    // ── Area 3 — relay echo dedup / D4 ───────────────────────────────────
    out.push(guard("A3.1", "fixture-relay", "relay echo of local publish dedups: observer fires once (D4)", a3_echo_dedup));
    out.push(guard("A3.2", "fixture-relay", "foreign-author note from relay reaches store+observer once", a3_foreign_ingest));

    // ── Area 5 / codex 9 — D9 created_at clamp ───────────────────────────
    out.push(guard("A5.1", "fixture-relay", "future-dated event clamped in observer; store keeps raw ts (D9)", a5_d9_clamp));

    // ── Area 4 — ephemeral ───────────────────────────────────────────────
    out.push(guard("A4.1", "fixture-relay", "ephemeral (20000-29999) reaches observers, never persisted (ADR-0057 fix)", a4_ephemeral));

    // ── Area 2 — persistence != relevance ────────────────────────────────
    out.push(guard("A2.1", "fixture-relay", "non-followed note PERSISTS even if not timeline-projected", a2_persist_not_relevance));

    // ── codex 7 — bad-sig / malformed no-side-effect, no poison ──────────
    out.push(guard("CX7", "kernel-inject", "bad-sig/malformed rejected; does not poison next valid ingest", cx7_bad_sig_no_poison));

    // ── codex 8 — gift-wrap 1059 ingest contract ─────────────────────────
    out.push(guard("CX8", "fixture-relay", "self-sent gift-wrap (kind:1059) ingested once via real relay", cx8_giftwrap_ingest));

    // ── Area 13 / codex 1,2 — replaceable supersession ───────────────────
    out.push(guard("A13.1", "fixture-relay", "replaceable kind:0 supersession: newer wins, older Superseded silent", a13_replaceable_supersede));

    // ── codex 3 — kind:5 delete unprojects target ────────────────────────
    out.push(guard("CX3", "fixture-relay", "kind:5 delete tombstones target (gone from store)", cx3_delete_unprojects));

    // ── Area 7 / codex 14 — cold-restart rebuildability ──────────────────
    out.push(guard("A7.1", "persistent+relay", "cold restart rebuilds store: a published note survives restart", a7_cold_restart));

    // ── Area 10 / PR2 — local kind:0 read-your-writes ────────────────────
    out.push(guard("A10.1", "local-publish", "[PR2] local kind:0 profile RYW: stored + readable after publish", a10_profile_ryw));

    // ── Area 11 / Workstream C — publish policy fail-closed ──────────────
    out.push(guard("A11.1", "local-publish", "[C] reserved kind:0 via PublishRaw is rejected (use PublishProfile)", a11_reserved_kind_rejected));
    out.push(guard("A11.2", "local-publish", "[C] explicit publish with empty relay set fails closed (D10)", a11_empty_explicit_rejected));

    // ── NIP-40 (codex 5) — expired-on-arrival silent ─────────────────────
    out.push(guard("CX5", "fixture-relay", "NIP-40 expired-on-arrival event is silent (no observer fire)", cx5_nip40_expired));

    // ── Area 9 / PR3 — contacts backfill + timeline_authors rebuild ──────
    out.push(guard("A9.1", "fixture-relay", "contacts backfill: kind:3 follow re-serves a new author's already-stored notes", a9_contacts_backfill));
    out.push(guard("A9.2", "fixture-relay", "kind:3 ingest rebuilds timeline_authors via the contacts parser", a9_kind3_rebuilds_authors));

    // ── Workstream B — acquisition one-door / store-first-then-network ───
    out.push(guard("B.1", "store+local", "acquisition: an interest opened against stored events serves store-first (no relay had it)", b1_store_first_serve));

    // ── Area 6 / codex 12 — GC pin / in-flight publish (test-support GC) ──
    out.push(guard("A6.1", "test-gc", "in-flight publish is pinned (open-view) and survives GC pressure", a6_inflight_publish_pinned));
    out.push(guard("A6.2", "test-gc", "cold non-followed event is reaped under GC pressure to the ceiling", a6_cold_reaped));
    out.push(guard("CX12", "test-gc", "no pin-leak: an unreferenced published event is GC-evictable", cx12_no_pin_leak));

    // ── codex 11 — cache-serve provenance transition / no double-notify ──
    out.push(guard("CX11", "fixture-relay", "cache-served event: live relay dup does not double-notify; provenance transitions", cx11_provenance_transition));

    // ── codex 16 — multi-account two-instance isolation ──────────────────
    out.push(guard("CX16", "two-instance", "two NmpApp instances: A's read-your-write stays in A's store, absent from B", cx16_multi_account_isolation));

    // ── STILL not driven through this harness (honest coverage) ──────────
    out.push(skip("F.1", "n/a", "doctrine gate: store.insert banned outside ingest module", "Workstream F is a compile-time doctrine gate — covered by `cargo test -p nmp-testing --test doctrine_lint_smoke`, not a runtime harness"));
    out.push(skip("F.2", "n/a", "doctrine gate: notify_event_observers banned outside chokepoint", "Workstream F is a compile-time doctrine gate — covered by doctrine_lint_smoke, not a runtime harness"));

    out
}

/// Wrap a scenario fn that returns `Outcome`, catching panics so one blow-up
/// does not abort the whole run (a panic becomes a FAIL with the message).
fn guard(
    id: &'static str,
    driver: &'static str,
    title: &'static str,
    f: fn() -> Outcome,
) -> ScenarioResult {
    let outcome = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(o) => o,
        Err(e) => {
            let msg = e
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| e.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "panic".to_string());
            Outcome::Fail(format!("scenario panicked: {msg}"))
        }
    };
    result!(id, driver, title, outcome)
}

fn skip(
    id: &'static str,
    driver: &'static str,
    title: &'static str,
    why: &'static str,
) -> ScenarioResult {
    result!(id, driver, title, Outcome::Skip(why.to_string()))
}

// ─────────────────────────────────────────────────────────────────────────
// Area 1 — read-your-writes
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if the chokepoint did NOT route local publishes through
// notify_event_observers (the ADR-0057 fix), the observer never fires for a
// locally-published note and A1.1 goes red.
fn a1_note_ryw() -> Outcome {
    ryw_publish(1, "read-your-writes note", vec![], "kind:1 note")
}

// MUTATION: same chokepoint dependency for kind:6.
fn a1_repost_ryw() -> Outcome {
    // A repost references a target event id via an `e` tag. Content is a stable
    // marker we can match on (the actor signs, so we cannot predict the id).
    let target = "0".repeat(64);
    ryw_publish(6, "ryw-repost-marker", vec![vec!["e".into(), target]], "kind:6 repost")
}

fn a1_reaction_ryw() -> Outcome {
    let target = "1".repeat(64);
    ryw_publish(7, "+", vec![vec!["e".into(), target]], "kind:7 reaction")
}

/// Shared read-your-writes driver: publish via the real engine, then confirm
/// the locally-published event surfaces through the observer AND the store
/// before any relay ACK. The published event's final id is unknown at dispatch
/// (the actor stamps created_at + signs), so we match by (kind, author,
/// content) — exactly what a host UI would key its optimistic row on.
fn ryw_publish(kind: u32, content: &str, tags: Vec<Vec<String>>, label: &str) -> Outcome {
    let h = Harness::in_memory();
    let me = h.keys.public_key().to_hex();
    let ret = h.publish_raw(kind, content, tags);
    if ret.contains("\"error\"") {
        return fail(format!("{label} publish dispatch errored: {ret}"));
    }
    let content_owned = content.to_string();
    let me2 = me.clone();
    let observed = h.collector.wait_for_match(WAIT, move |e| {
        e.kind == kind && e.author == me2 && e.content == content_owned
    });
    match observed {
        Some(e) => check(
            h.store_has(&e.id),
            format!("{label} {} fired observer (RYW) but is absent from the store", e.id),
        ),
        None => fail(format!(
            "{label}: observer never fired for a locally-published event (RYW broken); dispatch={ret}"
        )),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Area 3 — relay echo dedup / D4
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if the chokepoint re-notified on a `Duplicate` store outcome
// (removing the D4 single-fire gate), the echo would fire the observer a
// second time and A3.1 goes red (fire_count == 2).
fn a3_echo_dedup() -> Outcome {
    let h = Harness::in_memory();
    let me = h.keys.public_key().to_hex();
    // Open a tailing interest pinned to the fixture relay for our own author so
    // the relay echo of our publish comes back through the real ingest path.
    h.open_interest(relay_pinned_interest(h.relay.url(), 9001, vec![1], vec![me.clone()], vec![]));
    // Publish to the fixture relay explicitly so it stores+echoes the event.
    let ret = h.publish_raw_explicit(1, "echo dedup note", vec![]);
    if ret.contains("\"error\"") {
        return fail(format!("explicit publish errored: {ret}"));
    }
    // Discover the published event id via the RYW observer fire (match content).
    let me2 = me.clone();
    let observed = h.collector.wait_for_match(WAIT, move |e| {
        e.kind == 1 && e.author == me2 && e.content == "echo dedup note"
    });
    let id = match observed {
        Some(e) => e.id,
        None => return fail("observer never fired on local publish (RYW) for the echo-dedup note"),
    };
    // The publish reached the fixture relay, which fans the event back out LIVE
    // on the already-open self-author sub (interest 9001) as a normal
    // `["EVENT", sub, ev]` frame — the real relay-echo path through
    // `handle_event` → `verify_and_persist`. The relay-worker uses ONE socket
    // per relay URL, so publish-out, echo-in, and the barrier REQ/EVENT are all
    // ordered on that single connection: once the barrier sentinel is observed,
    // the echo frame (sent earlier on the same socket) has been processed by the
    // chokepoint. Drain via the barrier (push-signal, no polling).
    h.barrier();
    h.barrier();
    let fires = h.collector.fire_count(&id);
    check(
        fires == 1,
        format!(
            "D4 single-fire violated: observer fired {fires}x for {id} \
             (local publish + relay echo must dedup to 1)"
        ),
    )
}

// MUTATION: if admission were relevance-gated (the pre-ADR-0057 should_store
// persistence gate), a foreign author's note delivered by the relay would be
// dropped and A3.2 goes red.
fn a3_foreign_ingest() -> Outcome {
    let h = Harness::in_memory();
    let foreign = Keys::generate();
    let foreign_pk = foreign.public_key().to_hex();
    let ev = build_signed_event(&foreign, 1, "foreign author note", vec![], h.now());
    let id = ev.id.to_hex();
    // Stage the event on the relay, then open an interest that pulls it.
    h.relay.stage_event(&event_to_value(&ev));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9002, vec![1], vec![foreign_pk], vec![]));
    if !h.collector.wait_for(&id, WAIT) {
        return fail(format!("foreign-author note {id} never reached observer via relay"));
    }
    check(h.store_has(&id), format!("foreign note {id} reached observer but not store"))
}

// ─────────────────────────────────────────────────────────────────────────
// Area 5 / codex 9 — D9 created_at clamp
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if the timeline observer dropped the D9 clamp, the observer would
// be handed the hostile future timestamp and A5.1 goes red. Also asserts the
// STORE retains the raw (unclamped) timestamp for protocol correctness.
fn a5_d9_clamp() -> Outcome {
    let h = Harness::in_memory();
    let foreign = Keys::generate();
    let foreign_pk = foreign.public_key().to_hex();
    let now = h.now();
    let future = now + 10 * 365 * 24 * 3600; // +10 years
    let ev = build_signed_event(&foreign, 1, "future-dated hostile note", vec![], future);
    let id = ev.id.to_hex();
    h.relay.stage_event(&event_to_value(&ev));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9003, vec![1], vec![foreign_pk], vec![]));
    if !h.collector.wait_for(&id, WAIT) {
        return fail(format!("future-dated note {id} never reached observer"));
    }
    let observed = h.collector.observed_created_at(&id).unwrap_or(future);
    let stored = match h.event_by_id(&id) {
        Some(ev) => ev.created_at,
        None => return fail(format!("future-dated note {id} not in store")),
    };
    // Observer's created_at must be clamped (<= a small margin over now); store
    // must retain the raw future timestamp.
    let clamped = observed <= now + 60;
    let raw_kept = stored == future;
    check(
        clamped && raw_kept,
        format!(
            "D9: observed_created_at={observed} (now={now}; clamped={clamped}), stored={stored} (raw_kept={raw_kept}); \
             expected observer clamped to ~now AND store keeping raw {future}"
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Area 4 — ephemeral
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: ADR-0057's latent-bug fix added Ephemeral to the observer gate. If
// the observer gate were Inserted|Replaced only (pre-fix), an ephemeral would
// reach parsers but NOT the app observer, and A4.1 goes red (observer silent).
// Conversely the store must NOT contain it (store-layer ephemeral exclusion).
fn a4_ephemeral() -> Outcome {
    let h = Harness::in_memory();
    let foreign = Keys::generate();
    let foreign_pk = foreign.public_key().to_hex();
    // Kind 20001 is ephemeral (20000-29999).
    let ev = build_signed_event(&foreign, 20001, "ephemeral ping", vec![], h.now());
    let id = ev.id.to_hex();
    h.relay.stage_event(&event_to_value(&ev));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9004, vec![20001], vec![foreign_pk], vec![]));
    let fired = h.collector.wait_for(&id, WAIT);
    // Ephemeral must reach the app observer (the ADR-0057 fix)...
    if !fired {
        return fail(format!(
            "ephemeral {id} did NOT reach the app observer — ADR-0057 ephemeral-delivery fix not effective (or not landed)"
        ));
    }
    // ...but must NOT be persisted (store-layer exclusion).
    check(
        !h.store_has(&id),
        format!("ephemeral {id} reached observer (good) but was PERSISTED (store-layer ephemeral exclusion broken)"),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Area 2 — persistence != relevance
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: re-promoting should_store_event to a persistence gate (the
// pre-ADR-0057 behavior) would drop this non-followed note from the store and
// A2.1 goes red. We have no follow set (no kind:3), so this author is
// non-followed; the note must still persist on valid-sig alone.
fn a2_persist_not_relevance() -> Outcome {
    let h = Harness::in_memory();
    let stranger = Keys::generate();
    let stranger_pk = stranger.public_key().to_hex();
    let ev = build_signed_event(&stranger, 1, "non-followed stranger note", vec![], h.now());
    let id = ev.id.to_hex();
    h.relay.stage_event(&event_to_value(&ev));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9005, vec![1], vec![stranger_pk], vec![]));
    if !h.collector.wait_for(&id, WAIT) {
        return fail(format!("non-followed note {id} never ingested via relay"));
    }
    check(
        h.store_has(&id),
        format!("persistence != relevance violated: non-followed note {id} is NOT in the authoritative store"),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// codex 7 — bad-sig / malformed, no side effect, no poison
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if the verify gate were removed, the forged event would be stored
// and fire the observer (CX7 goes red on the forged id). We then prove a real
// event still ingests afterward (no poison).
fn cx7_bad_sig_no_poison() -> Outcome {
    let h = Harness::in_memory();
    // Forge a kind:1 with a valid id-hash but a garbage signature by editing the
    // JSON of a legitimately-built event.
    let author = Keys::generate();
    let good = build_signed_event(&author, 1, "to be forged", vec![], h.now());
    let mut val = event_to_value(&good);
    val["sig"] = serde_json::Value::String("0".repeat(128));
    let forged = serde_json::from_value::<nostr::Event>(val.clone());
    // The inject seam runs full Schnorr verify; a forged sig must be rejected.
    let injected_ok = match forged {
        Ok(ev) => h.inject_signed_event_json(&ev),
        Err(_) => false, // nostr refused to even parse the forged event — also a reject
    };
    if injected_ok {
        return fail("forged-signature event was ACCEPTED by the verify gate (CX7)");
    }
    // No poison: a subsequent valid event must still ingest cleanly.
    let good2 = build_signed_event(&author, 1, "valid after forged", vec![], h.now() + 1);
    let id2 = good2.id.to_hex();
    if !h.inject_signed_event_json(&good2) {
        return fail("valid event rejected after a forged one — verify path poisoned (CX7)");
    }
    if !h.collector.wait_for(&id2, WAIT) {
        return fail(format!("valid event {id2} did not ingest after a forged one (poison)"));
    }
    ok()
}

// ─────────────────────────────────────────────────────────────────────────
// codex 8 — gift-wrap 1059 ingest contract
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if kind:1059 were excluded from ingest/observer fan-out, the
// gift-wrap would never reach the observer and CX8 goes red. Uses REAL
// nmp_nip59::gift_wrap_local (real NIP-44 seal + ephemeral wrap).
fn cx8_giftwrap_ingest() -> Outcome {
    let h = Harness::in_memory();
    // Self-sent DM: sender == receiver == our key. Build a rumor (kind:14).
    let me = &h.keys;
    let my_pk = me.public_key();
    let rumor = UnsignedEvent::new(
        my_pk,
        Timestamp::from(h.now()),
        Kind::from(14u16),
        vec![],
        "self gift-wrapped dm".to_string(),
    );
    let wrap = match nmp_nip59::gift_wrap_local(me, &my_pk, &rumor, Timestamp::from(h.now())) {
        Ok(w) => w,
        Err(e) => return fail(format!("gift_wrap_local failed: {e}")),
    };
    let id = wrap.id.to_hex();
    // Deliver the kind:1059 via the real relay path: stage + p-tag interest.
    h.relay.stage_event(&event_to_value(&wrap));
    h.open_interest(relay_pinned_interest(
        h.relay.url(),
        9006,
        vec![1059],
        vec![],
        vec![("p".into(), my_pk.to_hex())],
    ));
    if !h.collector.wait_for(&id, WAIT) {
        return fail(format!("gift-wrap {id} never reached observer via relay (kind:1059 ingest contract)"));
    }
    check(
        h.store_has(&id),
        format!("gift-wrap {id} reached observer but not store"),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Area 13 / codex 1,2 — replaceable supersession
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if Superseded were treated like Inserted, the older sibling would
// fire the observer / overwrite the newer, and A13.1 goes red. We deliver a
// newer kind:0 then an older sibling; the newer must win in the store and the
// older must be silent.
fn a13_replaceable_supersede() -> Outcome {
    let h = Harness::in_memory();
    let author = Keys::generate();
    let author_pk = author.public_key().to_hex();
    let now = h.now();
    let newer = build_signed_event(&author, 0, "{\"name\":\"newer\"}", vec![], now + 100);
    let older = build_signed_event(&author, 0, "{\"name\":\"older\"}", vec![], now);
    let newer_id = newer.id.to_hex();
    let older_id = older.id.to_hex();
    // Stage newer first, open interest, then stage older (sibling).
    h.relay.stage_event(&event_to_value(&newer));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9007, vec![0], vec![author_pk.clone()], vec![]));
    if !h.collector.wait_for(&newer_id, WAIT) {
        return fail(format!("newer kind:0 {newer_id} never ingested"));
    }
    // Now deliver the older sibling live, then drain via the barrier (the older
    // sibling frame is issued before the barrier sentinel, so once the sentinel
    // is observed the older sibling has already been processed by the chokepoint).
    h.relay.stage_event(&event_to_value(&older));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9012, vec![0], vec![author_pk], vec![]));
    h.barrier();
    h.barrier();
    // Newer must remain in store; older must NOT fire the observer (silent
    // supersession) and must NOT be the resident replaceable.
    let newer_present = h.store_has(&newer_id);
    let older_fired = h.collector.fire_count(&older_id) > 0;
    check(
        newer_present && !older_fired,
        format!(
            "replaceable supersession: newer_present={newer_present}, older_fired_observer={older_fired} \
             (expected newer kept, older silent)"
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// codex 3 — kind:5 delete unprojects target
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if kind:5 delete handling were removed, the target would remain in
// the store after the delete and CX3 goes red.
fn cx3_delete_unprojects() -> Outcome {
    let h = Harness::in_memory();
    let author = Keys::generate();
    let author_pk = author.public_key().to_hex();
    let target = build_signed_event(&author, 1, "delete me", vec![], h.now());
    let target_id = target.id.to_hex();
    h.relay.stage_event(&event_to_value(&target));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9008, vec![1, 5], vec![author_pk.clone()], vec![]));
    if !h.collector.wait_for(&target_id, WAIT) {
        return fail(format!("target {target_id} never ingested before delete"));
    }
    if !h.store_has(&target_id) {
        return fail(format!("target {target_id} not in store before delete"));
    }
    // Author publishes a kind:5 deletion referencing the target via an e-tag.
    let del = build_signed_event(
        &author,
        5,
        "",
        vec![Tag::parse(["e", &target_id]).expect("e tag")],
        h.now() + 1,
    );
    h.relay.stage_event(&event_to_value(&del));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9013, vec![5], vec![author_pk], vec![]));
    // Drain via the barrier: the delete frame is issued before the sentinel, so
    // once the sentinel is observed the tombstone has been applied.
    h.barrier();
    h.barrier();
    check(
        !h.store_has(&target_id),
        format!("kind:5 delete did not remove target {target_id} from the store"),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Area 7 / codex 14 — cold-restart rebuildability
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if persistence were relevance-gated (pre-ADR-0057), a published
// note (self-authored, not in own follow set) would not be in the durable
// store and would vanish on cold restart — A7.1 goes red.
fn a7_cold_restart() -> Outcome {
    let dir = std::env::temp_dir().join(format!(
        "nmp-stress-coldrestart-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.to_string_lossy().to_string();

    let h = Harness::persistent(&path);
    let me = h.keys.public_key().to_hex();
    let ret = h.publish_raw(1, "survive the restart", vec![]);
    if ret.contains("\"error\"") {
        let _ = std::fs::remove_dir_all(&dir);
        return fail(format!("publish errored: {ret}"));
    }
    let me2 = me.clone();
    let observed = h.collector.wait_for_match(WAIT, move |e| {
        e.kind == 1 && e.author == me2 && e.content == "survive the restart"
    });
    let id = match observed {
        Some(e) => e.id,
        None => {
            let _ = std::fs::remove_dir_all(&dir);
            return fail("note never reached observer pre-restart (RYW)");
        }
    };
    if !h.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail(format!("note {id} not in store pre-restart"));
    }
    // Cold restart against the same storage path + key.
    let h2 = h.restart();
    // Barrier proves the new actor is up + ingesting (a fresh sentinel flows
    // through it); the original note was committed to LMDB before restart, so
    // it is synchronously readable once the actor is live.
    h2.barrier();
    let present = h2.store_has(&id);
    let _ = std::fs::remove_dir_all(&dir);
    check(
        present,
        format!("cold-restart rebuild lost note {id} — store not durable across restart"),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Area 10 / PR2 — local kind:0 profile read-your-writes
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if local kind:0 publishes did not flow through the chokepoint /
// profile cache, the just-published profile would not be readable and A10.1
// goes red.
fn a10_profile_ryw() -> Outcome {
    let h = Harness::in_memory();
    let fields = serde_json::json!({ "name": "ryw-profile", "about": "stress harness" });
    let ret = h.publish_profile(fields);
    if ret.contains("error") {
        return fail(format!("PublishProfile dispatch errored: {ret}"));
    }
    // The kind:0 is signed+stored by the active account. We assert it lands in
    // the store under the active pubkey by waiting for the observer to fire for
    // a kind:0 from our key (the correlation id for PublishProfile is the
    // resulting event id once signed).
    let me = h.keys.public_key().to_hex();
    let me2 = me.clone();
    // The kind:0 observer fires once the actor signs+stores it (condvar wait).
    let observed = h.collector.wait_for_match(WAIT, move |e| e.kind == 0 && e.author == me2);
    match observed {
        Some(e) => check(
            h.store_has(&e.id),
            format!("[PR2] kind:0 profile {} fired observer but is not in the store", e.id),
        ),
        None => fail("[PR2] local kind:0 profile did not surface via observer after PublishProfile (RYW)"),
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Area 11 / Workstream C — publish policy fail-closed
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if PublishRaw stopped rejecting reserved kinds (0/3), this would be
// accepted and A11.1 goes red. The dedicated PublishProfile/contacts variants
// exist precisely so raw publishes can't bypass their processing.
fn a11_reserved_kind_rejected() -> Outcome {
    let h = Harness::in_memory();
    let ret = h.publish_raw(0, "{\"name\":\"bypass\"}", vec![]);
    // The dispatch should be rejected (error) OR the action should terminally
    // fail — either way it must NOT be accepted as a normal publish.
    check(
        ret.contains("error") || ret.to_lowercase().contains("reject") || ret.to_lowercase().contains("kind"),
        format!("[C] reserved kind:0 via PublishRaw was NOT rejected: {ret}"),
    )
}

// MUTATION: if validate_explicit_relays stopped rejecting empty relay sets,
// an explicit publish with no relays would silently widen / drop and A11.2 goes
// red. (D10 fail-closed.)
fn a11_empty_explicit_rejected() -> Outcome {
    let h = Harness::in_memory();
    let action = serde_json::json!({
        "PublishRaw": {
            "kind": 1,
            "tags": [],
            "content": "no relays",
            "target": { "Explicit": { "relays": [] } }
        }
    })
    .to_string();
    let ret = h.dispatch("nmp.publish", &action);
    check(
        ret.contains("error") || ret.to_lowercase().contains("relay"),
        format!("[C] empty Explicit relay set was NOT rejected (D10 fail-closed): {ret}"),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// codex 5 — NIP-40 expired-on-arrival is silent
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if the NIP-40 expiry check were dropped, an already-expired event
// would fire the observer and CX5 goes red.
fn cx5_nip40_expired() -> Outcome {
    let h = Harness::in_memory();
    let foreign = Keys::generate();
    let foreign_pk = foreign.public_key().to_hex();
    let now = h.now();
    // expiration tag in the past (already expired on arrival).
    let expired_at = (now - 3600).to_string();
    let ev = build_signed_event(
        &foreign,
        1,
        "already expired",
        vec![Tag::parse(["expiration", &expired_at]).expect("expiration tag")],
        now - 10,
    );
    let id = ev.id.to_hex();
    h.relay.stage_event(&event_to_value(&ev));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9009, vec![1], vec![foreign_pk], vec![]));
    // An expired event must NOT fire the observer. Drain via the barrier: once a
    // later sentinel is observed, the expired event's frame has been processed.
    h.barrier();
    h.barrier();
    let fired = h.collector.fire_count(&id) > 0;
    if fired {
        return Outcome::Fail(format!(
            "NIP-40: already-expired event {id} fired the app observer — expiry not silent (or NIP-40 enforcement not in this path)"
        ));
    }
    ok()
}

// ─────────────────────────────────────────────────────────────────────────
// Area 9 / PR3 — contacts backfill + timeline_authors rebuild
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if `ingest_contacts` stopped calling `sync_follow_feed_interests`
// (the parser→effect-signal path), the kind:3 would not enqueue the follow-feed
// cache-serve and the new author's PRIOR stored notes would never re-surface —
// A9.1 goes red (fire_count never increases after the follow).
//
// Backfill mechanism under test: a non-followed author's notes are admitted to
// the store via an open interest (persistence != relevance). When the active
// account then follows that author (kind:3), `sync_follow_feed_interests`
// registers a follow-feed `LogicalInterest` AND enqueues a store-cache serve for
// it (ADR-0045 E1) — re-delivering the prior stored notes through
// `feed_served_event` → `notify_event_observers`. This is the durable-store
// backfill that replaced the bounded pre-kind:3 buffer for already-stored events.
fn a9_contacts_backfill() -> Outcome {
    // Persistent + cold restart so X's prior notes live in the durable STORE but
    // NOT in the kernel RAM read-cache (`events`) — the cache-serve live→serve
    // dedup (`!events_cache.contains_key`) means a genuine backfill only fires
    // for events not already reflected in projections, which is exactly the
    // "surface prior STORED events on follow" case.
    let dir = unique_tmp_dir("nmp-stress-a9-backfill");
    let path = dir.to_string_lossy().to_string();
    let h = Harness::persistent(&path);
    h.open_contact_feed(&[1]);
    let x = Keys::generate();
    let x_pk = x.public_key().to_hex();
    let now = h.now();
    let n1 = build_signed_event(&x, 1, "x prior note 1", vec![], now - 2);
    let n2 = build_signed_event(&x, 1, "x prior note 2", vec![], now - 1);
    let id1 = n1.id.to_hex();
    let id2 = n2.id.to_hex();
    // Session 1: persist X's notes as a NON-followed author via an open interest.
    h.relay.stage_event(&event_to_value(&n1));
    h.relay.stage_event(&event_to_value(&n2));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9101, vec![1], vec![x_pk.clone()], vec![]));
    if !h.collector.wait_for(&id1, WAIT) || !h.collector.wait_for(&id2, WAIT) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("X's prior notes never reached the store via the open interest");
    }
    if !h.store_has(&id1) || !h.store_has(&id2) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("X's prior notes are not in the store before restart");
    }
    let me = h.keys.public_key().to_hex();

    // Cold restart: RAM read-cache is empty, X's notes remain in the durable
    // store, the follow set is empty (no kind:3 was published in session 1).
    let h2 = h.restart();
    h2.open_contact_feed(&[1]);
    h2.barrier();
    if h2.timeline_authors().contains(&x_pk) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("X already followed after restart (no kind:3 was published)");
    }
    // Fresh collector → X's notes have not been observed in this session yet.
    if h2.collector.fire_count(&id1) != 0 || h2.collector.fire_count(&id2) != 0 {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("X's notes were delivered before the follow (unexpected)");
    }

    // Follow X by ingesting the active account's own kind:3 through the REAL
    // relay path → `Kind3Parser` writes the contacts cache → the kernel's
    // active-account contacts-transition signal fires
    // `sync_follow_feed_interests` → follow-feed cache-serve backfills X's
    // store-resident notes.
    let k3 = build_signed_event(
        &h2.keys,
        3,
        "",
        vec![Tag::parse(["p", &x_pk]).expect("p tag")],
        h2.now(),
    );
    let k3_id = k3.id.to_hex();
    h2.relay.stage_event(&event_to_value(&k3));
    h2.open_interest(relay_pinned_interest(h2.relay.url(), 9102, vec![3], vec![me], vec![]));
    if !h2.collector.wait_for(&k3_id, WAIT) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("active-account kind:3 never ingested via relay");
    }
    h2.barrier();
    h2.barrier();

    let followed = h2.timeline_authors().contains(&x_pk);
    let f1 = h2.collector.fire_count(&id1);
    let f2 = h2.collector.fire_count(&id2);
    let _ = std::fs::remove_dir_all(&dir);
    check(
        followed && f1 > 0 && f2 > 0,
        format!(
            "contacts backfill: followed={followed} (want true), X's prior notes \
             surfaced after the follow with fire counts n1={f1}, n2={f2} (want >0 each) \
             — the kind:3 should backfill the store-resident notes into the timeline"
        ),
    )
}

// MUTATION: if the kind:3 parser stopped rebuilding the `timeline_authors`
// projection (the M2 `sync_follow_feed_interests` derived cache), a follow would
// not be reflected and A9.2 goes red. Distinct from A9.1: this asserts the
// projection rebuild directly (ContactsLookup + effect-signal), independent of
// the backfill re-serve.
fn a9_kind3_rebuilds_authors() -> Outcome {
    let h = Harness::in_memory();
    h.open_contact_feed(&[1]);
    let me = h.keys.public_key().to_hex();
    let a = Keys::generate();
    let b = Keys::generate();
    let a_pk = a.public_key().to_hex();
    let b_pk = b.public_key().to_hex();
    let k3 = build_signed_event(
        &h.keys,
        3,
        "",
        vec![
            Tag::parse(["p", &a_pk]).expect("p tag a"),
            Tag::parse(["p", &b_pk]).expect("p tag b"),
        ],
        h.now(),
    );
    let k3_id = k3.id.to_hex();
    h.relay.stage_event(&event_to_value(&k3));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9111, vec![3], vec![me.clone()], vec![]));
    if !h.collector.wait_for(&k3_id, WAIT) {
        return fail("kind:3 never ingested via relay");
    }
    h.barrier();
    let authors = h.timeline_authors();
    check(
        authors.contains(&a_pk) && authors.contains(&b_pk) && authors.contains(&me),
        format!(
            "timeline_authors not rebuilt from kind:3: have {authors:?}; \
             expected to contain followed {a_pk}, {b_pk} and self {me}"
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Workstream B — acquisition one-door / store-first-then-network
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if `push_interest_and_serve` dropped its store-cache serve hook
// (ADR-0045 E1), opening an interest against an event ONLY in the store (no
// relay holds it) would deliver nothing — B.1 goes red. This proves the
// uniform store-first serve on interest open, decoupled from the network: the
// fixture relay is never staged with the seeded event, so the post-open
// observer fire can only have come from the store.
fn b1_store_first_serve() -> Outcome {
    // Persistent + cold restart: after restart the durable store holds the
    // event but the RAM read-cache is empty AND the (fresh) fixture relay has
    // ZERO copies. Opening an interest must serve the event from the store
    // BEFORE any network delivery — the literal "store-first, network second"
    // acquisition door, proven by the relay never having held the event.
    let dir = unique_tmp_dir("nmp-stress-b1-storefirst");
    let path = dir.to_string_lossy().to_string();
    let h = Harness::persistent(&path);
    let x = Keys::generate();
    let x_pk = x.public_key().to_hex();
    let n = build_signed_event(&x, 1, "store-first served note", vec![], h.now());
    let id = n.id.to_hex();
    // Session 1: persist via an open interest off the relay.
    h.relay.stage_event(&event_to_value(&n));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9201, vec![1], vec![x_pk.clone()], vec![]));
    if !h.collector.wait_for(&id, WAIT) || !h.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("seeded note did not land in the store in session 1");
    }
    // Cold restart: store retains the note; RAM cache + relay are fresh/empty.
    let h2 = h.restart();
    h2.barrier();
    if !h2.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("note did not survive restart (durability)");
    }
    if h2.collector.fire_count(&id) != 0 {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("note observed before any interest opened post-restart");
    }
    // Open an interest matching the note. The fresh relay has NOTHING, so the
    // only delivery path is the store-cache serve.
    h2.open_interest(relay_pinned_interest(h2.relay.url(), 9202, vec![1], vec![x_pk], vec![]));
    h2.barrier();
    h2.barrier();
    let served = h2.collector.fire_count(&id) > 0;
    let relay_had_it = h2.relay.has_event(&id);
    let _ = std::fs::remove_dir_all(&dir);
    check(
        served && !relay_had_it,
        format!(
            "store-first serve: served_from_store={served} (want true), \
             relay_held_it={relay_had_it} (want false) — opening an interest \
             against a store-resident event must serve it before the network"
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// Area 6 / codex 12 — GC pin set + in-flight publish (test-support GC seam)
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: drop the active-account self-interest from `open_view_pins` (or
// remove the published event from `derive_store_pin_set`) and a just-published
// note would be evicted under GC pressure — A6.1 goes red. A published note
// that matches the active home-feed interest is in the pin set and survives an
// aggressive GC pass (ceiling 0), i.e. it is NOT evicted before relay
// confirmation.
fn a6_inflight_publish_pinned() -> Outcome {
    let h = Harness::in_memory();
    // Activate the home feed so the active account's own notes are a live view.
    h.open_contact_feed(&[1]);
    let me = h.keys.public_key().to_hex();
    let ret = h.publish_raw(1, "in-flight pinned note", vec![]);
    if ret.contains("\"error\"") {
        return fail(format!("publish dispatch errored: {ret}"));
    }
    let me2 = me.clone();
    let observed = h.collector.wait_for_match(WAIT, move |e| {
        e.kind == 1 && e.author == me2 && e.content == "in-flight pinned note"
    });
    let id = match observed {
        Some(e) => e.id,
        None => return fail("published note never reached the observer (RYW)"),
    };
    if !h.store_has(&id) {
        return fail(format!("published note {id} not in store"));
    }
    h.barrier();
    let pinned = h.pin_set().contains(&id);
    // Aggressive GC: evict every un-pinned event down to ceiling 0.
    let _ = h.run_gc(0);
    let survived = h.store_has(&id);
    check(
        pinned && survived,
        format!(
            "in-flight publish pin: pinned={pinned}, survived_gc={survived} for {id} \
             (expected the just-published note to be pinned by the active home-feed \
             view and to survive an aggressive GC pass)"
        ),
    )
}

// MUTATION: if `derive_store_pin_set` over-pinned (e.g. pinned every stored
// event), a cold non-followed note would never be reaped — A6.2 goes red. A
// non-followed note that is no longer referenced by any open view (achieved via
// a cold restart that re-opens no interests) IS reaped by GC down to the
// ceiling.
fn a6_cold_reaped() -> Outcome {
    let dir = unique_tmp_dir("nmp-stress-gc-cold");
    let path = dir.to_string_lossy().to_string();
    let h = Harness::persistent(&path);
    let x = Keys::generate();
    let x_pk = x.public_key().to_hex();
    let n = build_signed_event(&x, 1, "cold non-followed note", vec![], h.now());
    let id = n.id.to_hex();
    h.relay.stage_event(&event_to_value(&n));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9301, vec![1], vec![x_pk], vec![]));
    if !h.collector.wait_for(&id, WAIT) || !h.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("cold note never reached the store before restart");
    }
    // Cold restart: fresh kernel, NO interests re-opened → the note is now an
    // unreferenced store leftover (not in timeline, not matched by any view).
    let h2 = h.restart();
    h2.barrier();
    if !h2.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail(format!("cold note {id} did not survive the restart (durability)"));
    }
    let pinned = h2.pin_set().contains(&id);
    let report = h2.run_gc(0);
    let reaped = !h2.store_has(&id);
    let evicted = report.as_ref().map(|r| r.lru_evicted).unwrap_or(0);
    let _ = std::fs::remove_dir_all(&dir);
    check(
        !pinned && reaped && evicted >= 1,
        format!(
            "cold reaping: pinned={pinned} (want false), reaped={reaped} (want true), \
             lru_evicted={evicted} (want >=1) for {id}"
        ),
    )
}

// MUTATION: if a publish installed a permanent GC pin on the published event
// (a pin leak) it would survive an aggressive GC pass even when unreferenced —
// CX12 goes red. There is NO publish-in-flight GC pin distinct from the
// open-view / timeline / claim pins (A6.1): a published note that no open view
// references is NOT in the pin set and IS evictable. This is the "pin releases
// after settlement" contract in its faithful master form — the pin was never
// taken, so there is nothing to leak.
fn cx12_no_pin_leak() -> Outcome {
    let h = Harness::in_memory();
    // Deliberately NO open_contact_feed: the active account has no home-feed
    // view, so a published note matches no open interest.
    let me = h.keys.public_key().to_hex();
    let ret = h.publish_raw(1, "unreferenced publish", vec![]);
    if ret.contains("\"error\"") {
        return fail(format!("publish dispatch errored: {ret}"));
    }
    let me2 = me.clone();
    let observed = h.collector.wait_for_match(WAIT, move |e| {
        e.kind == 1 && e.author == me2 && e.content == "unreferenced publish"
    });
    let id = match observed {
        Some(e) => e.id,
        None => return fail("published note never reached the observer (RYW)"),
    };
    if !h.store_has(&id) {
        return fail(format!("published note {id} not in store"));
    }
    h.barrier();
    let pinned = h.pin_set().contains(&id);
    let _ = h.run_gc(0);
    let evicted = !h.store_has(&id);
    check(
        !pinned && evicted,
        format!(
            "pin-leak check: pinned={pinned} (want false), evicted_by_gc={evicted} \
             (want true) for unreferenced published note {id}"
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// codex 11 — cache-serve provenance transition / no double-notify on live dup
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if the live-ingest path re-fired the observer on a `Duplicate`
// (removing D4 single-fire), the live relay delivery of a previously
// cache-served event would double-notify — CX11 goes red on the fire count.
// Conversely, if the live `Duplicate` did not record provenance, the relay
// transition would be invisible. Flow: cold-serve an event from the store
// (relay_count:0 LocalStore marker), then deliver it LIVE from a relay — the
// observer must NOT fire again, and the store provenance must gain the relay.
fn cx11_provenance_transition() -> Outcome {
    let dir = unique_tmp_dir("nmp-stress-prov");
    let path = dir.to_string_lossy().to_string();
    let h = Harness::persistent(&path);
    let x = Keys::generate();
    let x_pk = x.public_key().to_hex();
    let n = build_signed_event(&x, 1, "provenance note", vec![], h.now());
    let id = n.id.to_hex();
    // Session 1: deliver live so the durable store holds it.
    h.relay.stage_event(&event_to_value(&n));
    h.open_interest(relay_pinned_interest(h.relay.url(), 9401, vec![1], vec![x_pk.clone()], vec![]));
    if !h.collector.wait_for(&id, WAIT) || !h.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("note never stored in session 1");
    }
    // Cold restart: store has the note, observers are fresh (fire count 0), and
    // the fixture relay is brand-new (does NOT hold the note).
    let h2 = h.restart();
    h2.barrier();
    if !h2.store_has(&id) {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("note did not survive restart");
    }
    // Open an interest → store-cache COLD serve (relay_count:0). This is the
    // single observer fire for this event in session 2.
    h2.open_interest(relay_pinned_interest(h2.relay.url(), 9402, vec![1], vec![x_pk], vec![]));
    h2.barrier();
    h2.barrier();
    let fire_after_serve = h2.collector.fire_count(&id);
    if fire_after_serve == 0 {
        let _ = std::fs::remove_dir_all(&dir);
        return fail("cache-serve did not deliver the cold-stored event in session 2");
    }
    let prov_before = h2.provenance_relays(&id).len();
    // Now deliver the SAME event LIVE from the (new) fixture relay. The open
    // sub fans it in → `verify_and_persist` → Duplicate → provenance bump, but
    // NO observer re-fire (D4).
    h2.relay.stage_event(&event_to_value(&n));
    h2.barrier();
    h2.barrier();
    let fire_after_live = h2.collector.fire_count(&id);
    let prov_after = h2.provenance_relays(&id).len();
    let _ = std::fs::remove_dir_all(&dir);
    check(
        fire_after_live == fire_after_serve && prov_after > prov_before,
        format!(
            "provenance transition: fire {fire_after_serve}→{fire_after_live} \
             (want equal — no double-notify on live Duplicate), provenance \
             {prov_before}→{prov_after} (want growth — relay confirmed the \
             cache-served event)"
        ),
    )
}

// ─────────────────────────────────────────────────────────────────────────
// codex 16 — multi-account two-instance isolation
// ─────────────────────────────────────────────────────────────────────────

// MUTATION: if any kernel/store state were process-global (a shared static
// store, a leaked singleton) instead of per-`NmpApp`, B would observe A's
// read-your-write — CX16 goes red. Two independent app instances with separate
// signers and separate stores must not cross-talk.
fn cx16_multi_account_isolation() -> Outcome {
    let a = Harness::in_memory();
    let b = Harness::in_memory();
    let a_pk = a.keys.public_key().to_hex();
    // A publishes a note (read-your-write into A's store).
    let ret = a.publish_raw(1, "A private note", vec![]);
    if ret.contains("\"error\"") {
        return fail(format!("A publish errored: {ret}"));
    }
    let a_pk2 = a_pk.clone();
    let observed = a.collector.wait_for_match(WAIT, move |e| {
        e.kind == 1 && e.author == a_pk2 && e.content == "A private note"
    });
    let a_id = match observed {
        Some(e) => e.id,
        None => return fail("A's note never reached A's observer (RYW)"),
    };
    // Let B settle (a barrier proves B's actor processed everything to date).
    b.barrier();
    let in_a = a.store_has(&a_id);
    let in_b = b.store_has(&a_id);
    let b_observed_a = b.collector.fire_count(&a_id) > 0;
    check(
        in_a && !in_b && !b_observed_a,
        format!(
            "two-instance isolation: A.store_has={in_a} (want true), \
             B.store_has={in_b} (want false), B.observed_A={b_observed_a} (want false) \
             for {a_id}"
        ),
    )
}

/// A unique tempdir path for a persistent-store scenario (cold-restart / GC).
fn unique_tmp_dir(prefix: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "{prefix}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

// (No local poll/sleep helpers — all waits are push-signal based: the observer
// condvar `wait_for` / `wait_for_match`, or the `Harness::barrier()` drain
// which blocks on a sentinel observation. D8: no polling.)
