//! Scenario 6 — a real kind:3 contact-list change forces a subscription
//! re-plan.
//!
//! ## What this proves (honestly)
//!
//! We fetch a **real, live kind:3** contact list from a public relay, extract
//! its `p`-tagged followee set, and feed that set through the kernel's
//! in-process subscription compiler (`nmp_core::planner::SubscriptionCompiler`).
//! We then mutate the followee set exactly as a fresher kind:3 would (drop one
//! followee, add one new pubkey), recompile, and assert the recompiled plan's
//! REQ filter author-set differs from the original *by exactly that delta* and
//! that the content-addressed `plan_id` changed. This proves the re-plan logic
//! on a **real follow-graph input**, not a synthetic fixture.
//!
//! ## Known gap (deliberate, not a defect of this test)
//!
//! `crates/nmp-testing/tests/real_relay_smoke.rs` documents that the
//! actor-side subscription **rewire** wiring (drive a live CLOSE/REQ swap over
//! the socket when the follow list changes) does **not** exist yet. Driving a
//! live re-subscribe here would be a fabricated pass. So this scenario
//! validates the *planner-level* re-plan only — the layer that already exists
//! and is the thing under test. The live actor-side leg is an explicit M11+
//! gap recorded in the report's "Known gap" section.
//!
//! Honest-validation: if no candidate author returns a kind:3 with ≥2 p-tags
//! from any candidate relay within budget, we write a SKIP finding and return
//! without panicking — never a fake green.
//!
//! ```bash
//! cargo test -p nmp-testing --test real_relay_replan -- --ignored --nocapture
//! ```

#[path = "real_relay_common/mod.rs"]
mod common;

use std::collections::BTreeSet;
use std::time::{Duration, Instant};

use common::{
    report_page, send_text, try_open, write_report, Verdict, DAMUS_RELAY, NOSTR_BAND, PRIMAL_RELAY,
};
use nmp_core::planner::{
    InMemoryMailboxCache, InterestId, InterestLifecycle, InterestScope, InterestShape,
    LogicalInterest, MailboxSnapshot, Pubkey, SubscriptionCompiler,
};
use serde_json::Value;

/// Per-relay budget for the live kind:3 fetch.
const FETCH_BUDGET: Duration = Duration::from_secs(10);

/// Well-known authors very likely to have a published, non-trivial kind:3.
/// (name, hex pubkey). Tried in order.
const CANDIDATE_AUTHORS: &[(&str, &str)] = &[
    (
        "jb55",
        "82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2",
    ),
    (
        "fiatjaf",
        "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
    ),
    (
        "hodlbod",
        "97c70a44366a6535c145b333f973ea86dfdc2d7a99da618c40c64705ad98e322",
    ),
];

/// A synthetic, structurally-valid pubkey to *add* to the followee set (the
/// "new follow" leg of the delta). 64 hex chars, deterministic, and certain
/// not to already be in any real kind:3.
const NEW_FOLLOW: &str = "deadbeef00000000000000000000000000000000000000000000000000000000";

/// Parse an `["EVENT", subid, {event}]` text frame and, if it is a
/// structurally-valid signed kind:3 authored by `author_hex` for `sub_id`,
/// return its de-duplicated `p`-tag pubkey set.
fn parse_kind3_followees(text: &str, sub_id: &str, author_hex: &str) -> Option<BTreeSet<Pubkey>> {
    let v: Value = serde_json::from_str(text).ok()?;
    let arr = v.as_array()?;
    if arr.first()?.as_str()? != "EVENT" {
        return None;
    }
    if arr.get(1)?.as_str()? != sub_id {
        return None;
    }
    let ev = arr.get(2)?.as_object()?;
    if ev.get("kind")?.as_u64()? != 3 {
        return None;
    }
    if ev.get("pubkey")?.as_str()? != author_hex {
        return None;
    }
    // Structural signature sanity (we never verify sig cryptographically here;
    // shape-validity is enough — the planner only consumes the p-tag set).
    let id = ev.get("id")?.as_str()?;
    let sig = ev.get("sig")?.as_str()?;
    if id.len() != 64
        || !id.chars().all(|c| c.is_ascii_hexdigit())
        || sig.len() != 128
        || !sig.chars().all(|c| c.is_ascii_hexdigit())
    {
        return None;
    }
    let tags = ev.get("tags")?.as_array()?;
    let mut followees: BTreeSet<Pubkey> = BTreeSet::new();
    for tag in tags {
        let t = tag.as_array()?;
        if t.first().and_then(Value::as_str) == Some("p") {
            if let Some(pk) = t.get(1).and_then(Value::as_str) {
                if pk.len() == 64 && pk.chars().all(|c| c.is_ascii_hexdigit()) {
                    followees.insert(pk.to_string());
                }
            }
        }
    }
    Some(followees)
}

/// Compile a tailing timeline interest over `followees` and return the union
/// of `authors` across every REQ sub-shape in the plan, plus the plan_id.
///
/// We seed each followee with an indexer-routed mailbox so the outbox-direction
/// partition places it on a concrete relay sub-shape (otherwise an empty
/// mailbox cache + empty indexer set yields no per-relay plan and nothing to
/// diff). The relay choice is irrelevant to the assertion — we diff the
/// author-set, which is invariant to routing.
fn compile_author_set(followees: &BTreeSet<Pubkey>) -> (BTreeSet<Pubkey>, String) {
    let indexer = ["wss://planner.indexer.test".to_string()];
    let mut cache = InMemoryMailboxCache::new();
    for pk in followees {
        cache.put(
            pk.clone(),
            MailboxSnapshot {
                write_relays: vec!["wss://planner.indexer.test".to_string()],
                read_relays: Vec::new(),
                both_relays: Vec::new(),
            },
        );
    }
    let compiler = SubscriptionCompiler::new(&cache, &indexer);
    let interest = LogicalInterest {
        id: InterestId(1),
        scope: InterestScope::ActiveAccount,
        shape: InterestShape::timeline_for(followees.clone()),
        hints: Vec::new(),
        lifecycle: InterestLifecycle::Tailing,
    };
    let plan = compiler
        .compile(&[interest])
        .expect("compile of non-empty timeline interest must succeed");

    let mut authors: BTreeSet<Pubkey> = BTreeSet::new();
    for relay_plan in plan.per_relay.values() {
        for sub in &relay_plan.sub_shapes {
            for a in &sub.shape.authors {
                authors.insert(a.clone());
            }
        }
    }
    (authors, plan.plan_id)
}

fn skip(attempted_relays: &[&str], detail: &str) {
    let body = format!(
        "No candidate author returned a kind:3 with ≥2 p-tags within \
         {FETCH_BUDGET:?} per relay.\n\n\
         {detail}\n\n\
         This is a SKIP, not a pass: either the public relay set was \
         unreachable from this host/network, or none of the candidate \
         authors' contact lists were served in the budget. Re-run with \
         network access; if it persists, revisit the candidate author / \
         relay list.\n\n\
         No planner assertion was attempted — fabricating a green here \
         would defeat the honest-validation suite."
    );
    write_report(
        "scenario6-replan",
        &report_page(
            "Scenario 6 — kind:3 change forces subscription re-plan",
            "6-replan",
            Verdict::Skip,
            attempted_relays,
            &body,
        ),
    );
    eprintln!("SKIP: scenario 6 — {detail}");
}

#[test]
#[ignore = "real-relay (run with --ignored)"]
fn kind3_change_forces_subscription_replan() {
    let relays = [DAMUS_RELAY, PRIMAL_RELAY, NOSTR_BAND];
    let mut attempted: Vec<&str> = Vec::new();

    // ── Stage 1: fetch a REAL kind:3 + extract followees ─────────────────────
    let mut fetched: Option<(&str, &str, BTreeSet<Pubkey>)> = None; // (relay, author_name, followees)
    'outer: for relay in relays {
        attempted.push(relay);
        let Some(mut socket) = try_open(relay) else {
            continue;
        };
        for (name, hex) in CANDIDATE_AUTHORS {
            let sub_id = format!("rr-replan-{}", common::now_ms());
            let req =
                format!("[\"REQ\",\"{sub_id}\",{{\"authors\":[\"{hex}\"],\"kinds\":[3],\"limit\":1}}]");
            if send_text(&mut socket, req).is_err() {
                eprintln!("[replan] {relay}: REQ send failed for {name}");
                continue;
            }
            let deadline = Instant::now() + FETCH_BUDGET;
            let mut got: Option<BTreeSet<Pubkey>> = None;
            common::drain_until(&mut socket, deadline, |text| {
                if let Some(f) = parse_kind3_followees(text, &sub_id, hex) {
                    got = Some(f);
                    true
                } else {
                    false
                }
            });
            let _ = send_text(&mut socket, format!("[\"CLOSE\",\"{sub_id}\"]"));
            if let Some(followees) = got {
                if followees.len() >= 2 {
                    eprintln!(
                        "[replan] {relay}: {name} kind:3 -> {} followees",
                        followees.len()
                    );
                    fetched = Some((relay, name, followees));
                    let _ = socket.close(None);
                    break 'outer;
                }
                eprintln!(
                    "[replan] {relay}: {name} kind:3 had only {} p-tags (<2), trying next",
                    followees.len()
                );
            }
        }
        let _ = socket.close(None);
    }

    let Some((relay, author_name, followees)) = fetched else {
        skip(
            &attempted,
            &format!(
                "Authors tried (in order): {:?}. Relays tried: {attempted:?}.",
                CANDIDATE_AUTHORS
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
            ),
        );
        println!("SKIP: scenario 6 — no usable real kind:3");
        return;
    };

    // ── Stage 2: compile a plan from the REAL followee set ───────────────────
    let original_count = followees.len();
    let (orig_authors, orig_plan_id) = compile_author_set(&followees);
    assert_eq!(
        orig_authors, followees,
        "compiled REQ author-set must equal the real followee set"
    );

    // ── Stage 3: mutate followees (drop one, add one) + recompile ────────────
    // Deterministic pick of the followee to drop: the lexicographically
    // smallest (BTreeSet is ordered) — stable across runs of the same kind:3.
    let removed = followees
        .iter()
        .next()
        .expect("≥2 followees guaranteed by Stage 1")
        .clone();
    let added = NEW_FOLLOW.to_string();
    assert!(
        !followees.contains(&added),
        "synthetic NEW_FOLLOW must not collide with a real followee"
    );

    let mut mutated = followees.clone();
    mutated.remove(&removed);
    mutated.insert(added.clone());

    let (new_authors, new_plan_id) = compile_author_set(&mutated);

    // ── Stage 4: assert the delta is EXACTLY {-removed, +added} ──────────────
    assert!(
        !new_authors.contains(&removed),
        "dropped followee {removed} must be gone from the recompiled REQ author-set"
    );
    assert!(
        new_authors.contains(&added),
        "added followee {added} must be present in the recompiled REQ author-set"
    );
    // The symmetric difference between the two author-sets must be exactly the
    // two changed pubkeys — nothing else moved.
    let symmetric: BTreeSet<&Pubkey> = orig_authors
        .symmetric_difference(&new_authors)
        .collect::<BTreeSet<_>>();
    let expected: BTreeSet<&Pubkey> = [&removed, &added].into_iter().collect();
    assert_eq!(
        symmetric, expected,
        "recompiled author-set must differ from the original by exactly {{-removed, +added}}"
    );
    assert_ne!(
        orig_plan_id, new_plan_id,
        "a kind:3 followee change must invalidate the content-addressed plan_id"
    );

    // ── PASS report (with explicit Known gap section) ────────────────────────
    let body = format!(
        "Fetched a **real live kind:3** for `{author_name}` from `{relay}`, \
         extracted **{original_count}** `p`-tag followees, and ran them \
         through `nmp_core::planner::SubscriptionCompiler::compile` (the \
         in-process kernel subscription compiler).\n\n\
         ## Planner API exercised\n\n\
         - `InterestShape::timeline_for(followees)` → tailing kind:[1,6] \
           timeline interest over the real follow-set.\n\
         - `SubscriptionCompiler::new(&InMemoryMailboxCache, &indexer)` then \
           `.compile(&[interest])` → `CompiledPlan`.\n\
         - Asserted on the union of `RelayPlan.sub_shapes[].shape.authors` \
           and on `CompiledPlan.plan_id`.\n\n\
         ## Filter delta observed\n\n\
         - original REQ author-set size: **{original_count}** (== followee \
           count, exact).\n\
         - mutation applied: dropped `{removed}`, added `{added}`.\n\
         - recompiled REQ author-set size: **{}**.\n\
         - symmetric difference vs original: exactly \
           `{{-{removed}, +{added}}}` — no other author moved.\n\
         - `plan_id` changed: `{orig_plan_id}` → `{new_plan_id}` \
           (content-addressed identity correctly invalidated).\n\n\
         This proves the kernel re-plans subscriptions correctly when a real \
         follow-graph (kind:3) changes: the compiled REQ filter set tracks \
         the followee delta exactly, and the plan identity flips so the \
         wire-emitter diff would see the change.\n\n\
         ## Known gap (M11+) — live actor-side re-subscribe NOT validated here\n\n\
         The actor-side subscription **rewire** leg — driving a live \
         CLOSE/REQ swap over the relay socket in response to a fresher \
         kind:3 — is **not wired yet** (documented in \
         `crates/nmp-testing/tests/real_relay_smoke.rs`). Driving that over a \
         socket today would be a fabricated pass, so this scenario \
         deliberately validates only the **planner-level re-plan** (the layer \
         that exists). The end-to-end live re-subscribe leg remains an \
         explicit M11+ gap: when the actor-side rewire lands, a follow-up \
         scenario should assert the relay actually receives the new REQ / \
         CLOSE frames derived from this same plan diff.",
        new_authors.len(),
    );
    write_report(
        "scenario6-replan",
        &report_page(
            "Scenario 6 — kind:3 change forces subscription re-plan",
            "6-replan",
            Verdict::Pass,
            &[relay],
            &body,
        ),
    );
    println!(
        "[replan] PASS via {relay}: author={author_name} followees={original_count} \
         delta=-1/+1 plan_id {orig_plan_id}->{new_plan_id}"
    );
}
