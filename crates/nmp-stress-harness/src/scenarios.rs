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

    // ── NOT-YET-LANDED stubs (honest coverage) ───────────────────────────
    out.push(skip("A9.1", "n/a", "contacts backfill (follow new author backfills prior stored events)", "pending PR3 (contacts→parser)"));
    out.push(skip("A9.2", "n/a", "kind:3 rebuilds timeline_authors via parser", "pending PR3 (contacts→parser)"));
    out.push(skip("B.1", "n/a", "acquisition one-door: store-first-then-network uniform serve", "pending Workstream B (acquisition one-door)"));
    out.push(skip("F.1", "n/a", "doctrine gate: store.insert banned outside ingest module", "pending Workstream F (runtime via doctrine_lint_smoke, not this harness)"));
    out.push(skip("F.2", "n/a", "doctrine gate: notify_event_observers banned outside chokepoint", "pending Workstream F"));

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

// (No local poll/sleep helpers — all waits are push-signal based: the observer
// condvar `wait_for` / `wait_for_match`, or the `Harness::barrier()` drain
// which blocks on a sentinel observation. D8: no polling.)
