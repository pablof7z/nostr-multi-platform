//! ADR-0055 Rung 1 — REAL-driven scenario tests (review F3).
//!
//! These drive ACTUAL kernel entry points (ingest, claim, settle/drain a
//! publish, RAM-evict, FixedClock TTL, account switch) and the REAL
//! `make_update`, which in `cfg(test)` builds runs the biconditional oracle on
//! every emit (`Kernel::run_projection_oracle`) — a missed stamp panics. Unit /
//! arithmetic / F1-bite tests live in `tests_unit.rs`.

use crate::kernel::clock::FixedClock;
use crate::kernel::projection_rev::ProjectionPresence;
use crate::kernel::{Kernel, NostrEvent};
use crate::nip19::{encode_naddr, encode_nevent, NaddrData, NeventData};
use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

/// A fixed 64-hex pubkey for tests that only need an opaque active-account value
/// (`set_active_account` does not verify the pubkey).
const ACCOUNT_PK: &str = "abababababababababababababababababababababababababababababababab";

// ── Real-kernel helpers ───────────────────────────────────────────────────────
//
// Events are REALLY signed (`::nostr::Keys::generate()` + `EventBuilder`) so
// they pass `verify_and_persist`'s signature gate — the store-ingest chokepoints
// (F1) only fire on the genuine wire path, not on the `from_raw_unchecked` test
// shortcut.

/// A fresh kernel pinned to a deterministic `FixedClock` so TTL math and the
/// oracle are reproducible. Returns the kernel and the base time.
fn kernel_at(base_secs: u64) -> (Kernel, SystemTime) {
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    let base = SystemTime::UNIX_EPOCH + Duration::from_secs(base_secs);
    kernel.set_clock(Arc::new(FixedClock(base)));
    (kernel, base)
}

/// Convert a signed `::nostr::Event` to the kernel's `NostrEvent` shape.
fn to_kernel_event(ev: &::nostr::Event) -> NostrEvent {
    NostrEvent {
        id: ev.id.to_hex(),
        pubkey: ev.pubkey.to_hex(),
        created_at: ev.created_at.as_secs(),
        kind: ev.kind.as_u16() as u32,
        tags: ev
            .tags
            .iter()
            .map(|t: &::nostr::Tag| t.as_slice().to_vec())
            .collect(),
        content: ev.content.clone(),
        sig: ev.sig.to_string(),
    }
}

/// Build a REAL signed kind:30023 longform article. Returns `(event, id)`.
fn signed_article(keys: &::nostr::Keys, d_tag: &str, title: &str, ts: u64) -> (NostrEvent, String) {
    use ::nostr::{EventBuilder, Kind, Tag, Timestamp};
    let ev = EventBuilder::new(Kind::from_u16(30023), format!("body of {title}"))
        .tags([
            Tag::parse(["d", d_tag]).expect("valid d tag"),
            Tag::parse(["title", title]).expect("valid title tag"),
        ])
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let ke = to_kernel_event(&ev);
    let id = ke.id.clone();
    (ke, id)
}

/// Build a REAL signed kind:1 note. Returns `(event, id)`.
fn signed_note(keys: &::nostr::Keys, content: &str, ts: u64) -> (NostrEvent, String) {
    use ::nostr::{EventBuilder, Timestamp};
    let ev = EventBuilder::text_note(content)
        .custom_created_at(Timestamp::from(ts))
        .sign_with_keys(keys)
        .expect("sign_with_keys cannot fail with a generated keypair");
    let ke = to_kernel_event(&ev);
    let id = ke.id.clone();
    (ke, id)
}

/// Build a REAL signed kind:0 profile.
fn signed_profile(keys: &::nostr::Keys, name: &str, ts: u64) -> NostrEvent {
    use ::nostr::{EventBuilder, Kind, Timestamp};
    let ev = EventBuilder::new(
        Kind::from_u16(0),
        serde_json::json!({ "name": name }).to_string(),
    )
    .custom_created_at(Timestamp::from(ts))
    .sign_with_keys(keys)
    .expect("sign_with_keys cannot fail with a generated keypair");
    to_kernel_event(&ev)
}

/// Ingest a kernel event through the REAL wildcard ingest path
/// (`handle_event` -> `verify_and_persist`). `NostrEvent` is not `Serialize`, so
/// the EVENT-payload `Value` is assembled field-by-field (matching the wire
/// shape `serde_json::from_value::<NostrEvent>` expects in `handle_event`).
fn ingest(kernel: &mut Kernel, role: RelayRole, sub_id: &str, ev: &NostrEvent) {
    let value = serde_json::json!({
        "id": ev.id,
        "pubkey": ev.pubkey,
        "created_at": ev.created_at,
        "kind": ev.kind,
        "tags": ev.tags,
        "content": ev.content,
        "sig": ev.sig,
    });
    kernel.handle_event(role, "wss://relay.test/", sub_id, &value);
}

/// `nostr:naddr…` URI for a kind:30023 article.
fn naddr_uri(kind: u32, author: &str, d_tag: &str) -> String {
    let bech = encode_naddr(&NaddrData {
        identifier: d_tag.to_string(),
        pubkey: author.to_string(),
        kind,
        relays: vec![],
    })
    .expect("encode_naddr");
    format!("nostr:{bech}")
}

/// `nostr:nevent…` URI for a note.
fn nevent_uri(event_id: &str, kind: Option<u32>, author: Option<&str>) -> String {
    let bech = encode_nevent(&NeventData {
        event_id: event_id.to_string(),
        relays: vec![],
        author: author.map(str::to_string),
        kind,
    })
    .expect("encode_nevent");
    format!("nostr:{bech}")
}

/// Drive one real emit. In `cfg(test)` builds this runs the oracle and panics on
/// any violation — so simply calling it is a completeness assertion.
fn emit(kernel: &mut Kernel) {
    let _ = kernel.make_update(true);
}

/// Read a projection's LIVE `(rev, presence)` from the current tracker (presence
/// is rev-vs-last-emit here; use it for "did the rev advance after a mutation?"
/// assertions that run BEFORE the next emit).
fn live_state(kernel: &Kernel, key: &str) -> (u64, ProjectionPresence) {
    let s = kernel.projection_state(key);
    (s.rev, s.presence)
}

/// Read a projection's `(rev, presence)` AS THE LAST emit carried it (presence
/// overrides applied). Use for tristate (`Changed`/`Cleared`/`Unchanged`)
/// assertions after `emit`.
fn emitted_state(kernel: &Kernel, key: &str) -> (u64, ProjectionPresence) {
    let s = kernel
        .last_emitted_projection_state(key)
        .unwrap_or_else(|| panic!("no last-emit manifest state for key '{key}' — call emit() first"));
    (s.rev, s.presence)
}

// ── Scenario 1: store-backed fresh-longform claim (F1) ────────────────────────

/// S1: claim a kind:30023 by COORD, then ingest the article for the FIRST time
/// (`InsertOutcome::Inserted`). The store-ingest chokepoint must bump
/// `claimed_event_content_ver` even though the claim key is a coord, not the new
/// event's hex id — F1. The `claimed_events` rev must advance and the projection
/// be Changed. Driven through the REAL ingest + claim + emit path; the oracle
/// guards completeness.
#[test]
fn s1_fresh_longform_claim_store_ingest_bumps_claimed_events_rev() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();
    let d_tag = "my-article";

    let (mut kernel, _) = kernel_at(1_000);

    // Claim by coord BEFORE the article exists (the common cold-claim case).
    let uri = naddr_uri(30023, &author, d_tag);
    let _ = kernel.claim_event(uri, "view-1".to_string(), true, false);

    // Baseline emit so the manifest has a recorded last-emit for claimed_events.
    emit(&mut kernel);
    let (rev_before, _) = live_state(&kernel, "claimed_events");

    // The article arrives for the FIRST time -> InsertOutcome::Inserted. Signed,
    // so it passes verify_and_persist; matched to the live claim by COORD (the
    // new event's hex id is NOT the claim key — that is the F1 case).
    let (article, _id) = signed_article(&keys, d_tag, "Hello", 1_700_000_000);
    ingest(&mut kernel, RelayRole::Content, "lf-sub", &article);

    // The chokepoint bumps claimed_event_content_ver via the COORD fallback.
    let (rev_after, _) = live_state(&kernel, "claimed_events");
    assert!(
        rev_after > rev_before,
        "F1: fresh-longform Inserted matching a live coord claim must advance \
         claimed_events rev; before={rev_before} after={rev_after}"
    );

    // The emit reflects Changed and the oracle (run inside make_update) passes.
    emit(&mut kernel);
}

// ── Scenario 2: profile enrichment after claim ────────────────────────────────

/// S2: claim a note, then a kind:0 for its author arrives while the claim is
/// live. `ingest_profile`'s enrichment chokepoint (profiles_ver + event_claims
/// non-empty -> claimed_event_content_ver) must advance BOTH `profile` and
/// `claimed_events`. Real ingest + claim + emit; oracle guards completeness.
#[test]
fn s2_profile_enrichment_after_claim_bumps_both_revs() {
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let (mut kernel, _) = kernel_at(2_000);

    // Ingest the note (real signed), then claim it by id (so event_claims is
    // non-empty for this author).
    let (note, note_id) = signed_note(&keys, "hi", 1_700_000_000);
    ingest(&mut kernel, RelayRole::Content, "n-sub", &note);
    let uri = nevent_uri(&note_id, Some(1), Some(&author));
    let _ = kernel.claim_event(uri, "view-2".to_string(), true, false);

    emit(&mut kernel);
    let (ce_before, _) = live_state(&kernel, "claimed_events");
    let (prof_before, _) = live_state(&kernel, "profile");

    // kind:0 for the claimed note's author arrives (real signed).
    let profile = signed_profile(&keys, "alice", 1_700_000_500);
    ingest(&mut kernel, RelayRole::Indexer, "meta-sub", &profile);

    let (ce_after, _) = live_state(&kernel, "claimed_events");
    let (prof_after, _) = live_state(&kernel, "profile");
    assert!(
        ce_after > ce_before,
        "claimed_events must advance on profile enrichment; {ce_before} -> {ce_after}"
    );
    assert!(
        prof_after > prof_before,
        "profile must advance on profiles_ver bump; {prof_before} -> {prof_after}"
    );
    emit(&mut kernel);
}

// ── Scenario 3: drain present -> cleared -> unchanged (F2) ─────────────────────

/// S3: settle a `signed_events` entry (Changed), emit; then emit with an empty
/// drain (Cleared, exactly once); then emit AGAIN while stably empty (Unchanged,
/// no churn). Driven through the REAL `record_signed_event_return` + `make_update`
/// drain path. The oracle guards every emit.
#[test]
fn s3_drain_changed_then_cleared_then_unchanged() {
    let (mut kernel, _) = kernel_at(3_000);

    // Tick 1: a signed-event result lands -> non-empty drain -> Changed.
    kernel.record_signed_event_return("corr-1", Ok("{}".to_string()));
    emit(&mut kernel);
    let (_, p1) = emitted_state(&kernel, "signed_events");
    assert_eq!(
        p1,
        ProjectionPresence::Changed,
        "tick1: non-empty drain must be Changed"
    );

    // Tick 2: nothing new -> empty drain after non-empty -> Cleared (exactly once).
    emit(&mut kernel);
    let (_, p2) = emitted_state(&kernel, "signed_events");
    assert_eq!(
        p2,
        ProjectionPresence::Cleared,
        "tick2: non-empty -> empty transition must be Cleared (F2)"
    );

    // Tick 3: still empty -> stably empty -> Unchanged (no idle churn).
    emit(&mut kernel);
    let (_, p3) = emitted_state(&kernel, "signed_events");
    assert_eq!(
        p3,
        ProjectionPresence::Unchanged,
        "tick3: stably-empty drain must settle to Unchanged (no replay, no churn)"
    );
}

/// S3-bite: prove the oracle and tristate would CATCH the old "bump every tick"
/// behaviour. If a stably-empty drain advances its rev (the pre-F2 bug), the
/// presence would be Changed forever — which is exactly the idle churn F2 fixes.
/// Here we assert the REAL state machine does NOT churn: a third stably-empty
/// emit's rev equals the second's.
#[test]
fn s3_bite_stably_empty_drain_does_not_advance_rev() {
    let (mut kernel, _) = kernel_at(3_500);
    kernel.record_signed_event_return("corr-x", Ok("{}".to_string()));
    emit(&mut kernel); // Changed
    emit(&mut kernel); // Cleared
    let (rev_cleared, _) = live_state(&kernel, "signed_events");
    emit(&mut kernel); // stably empty
    let (rev_idle1, _) = live_state(&kernel, "signed_events");
    emit(&mut kernel); // stably empty again
    let (rev_idle2, _) = live_state(&kernel, "signed_events");
    assert_eq!(
        rev_cleared, rev_idle1,
        "rev must not advance once the drain settles empty (F2 anti-churn)"
    );
    assert_eq!(
        rev_idle1, rev_idle2,
        "rev must stay stable across repeated stably-empty drains"
    );
}

// ── Scenario 4: action_lifecycle TTL expiry via FixedClock (F3/codex #3) ───────

/// S4: record a terminal stage, advance a `FixedClock` past
/// `RECENT_TERMINAL_TTL_MS`, and emit. The TTL prune actually removes the row,
/// so `ttl_expiry_ver` bumps and `action_lifecycle` rev advances. An idle emit
/// before expiry must NOT advance the rev. Real lifecycle + clock + emit.
#[test]
fn s4_action_lifecycle_ttl_expiry_bumps_rev_only_on_expiry() {
    use crate::kernel::action_stages::ActionStage;
    use crate::kernel::action_lifecycle::RECENT_TERMINAL_TTL_MS;

    let base_secs = 4_000u64;
    let (mut kernel, base) = kernel_at(base_secs);

    // Record a terminal stage (Accepted) -> enqueue bump.
    kernel.record_action_stage("corr-ttl", ActionStage::Accepted, None);
    emit(&mut kernel);
    let (rev_after_enqueue, _) = live_state(&kernel, "action_lifecycle");

    // Idle emit BEFORE the TTL elapses: no prune, rev stable.
    emit(&mut kernel);
    let (rev_idle, _) = live_state(&kernel, "action_lifecycle");
    assert_eq!(
        rev_idle, rev_after_enqueue,
        "idle emit before TTL must not advance action_lifecycle rev"
    );

    // Advance the clock past the terminal TTL and emit -> real prune.
    let past_ttl = base + Duration::from_millis(RECENT_TERMINAL_TTL_MS + 1_000);
    kernel.set_clock(Arc::new(FixedClock(past_ttl)));
    emit(&mut kernel);
    let (rev_expired, _) = live_state(&kernel, "action_lifecycle");
    assert!(
        rev_expired > rev_idle,
        "TTL expiry (real prune) must advance action_lifecycle rev; {rev_idle} -> {rev_expired}"
    );
}

// ── Scenario 5: configured-relays change classification ───────────────────────

/// S5: changing the configured relay set advances `configured_relays`,
/// `relay_role_options`, `settings_hub` (via `configured_relays_ver`) and
/// `relay_diagnostics` (via the per-emit fingerprint). `publish_queue` stays
/// Unchanged. Real `set_configured_relays` + emit; oracle guards.
#[test]
fn s5_configured_relays_change_classification() {
    use crate::kernel::AppRelay;

    let (mut kernel, _) = kernel_at(5_000);
    emit(&mut kernel);

    let (cr_before, _) = live_state(&kernel, "configured_relays");
    let (pq_before, _) = live_state(&kernel, "publish_queue");

    kernel.set_configured_relays(vec![AppRelay::new(
        "wss://relay.example/".to_string(),
        "both".to_string(),
    )]);

    // configured_relays / relay_role_options / settings_hub are stamped at the
    // mutation site (`configured_relays_ver`), so they advance immediately.
    let (cr_after, cr_presence) = live_state(&kernel, "configured_relays");
    assert!(cr_after > cr_before, "configured_relays rev must advance");
    assert_eq!(cr_presence, ProjectionPresence::Changed);

    for key in ["relay_role_options", "settings_hub"] {
        let (_, p) = live_state(&kernel, key);
        assert_eq!(
            p,
            ProjectionPresence::Changed,
            "{key} depends on configured_relays_ver -> must be Changed"
        );
    }

    // publish_queue does not depend on configured_relays_ver — unmoved.
    let (pq_after, pq_presence) = live_state(&kernel, "publish_queue");
    assert_eq!(pq_after, pq_before, "publish_queue rev must not move");
    assert_eq!(pq_presence, ProjectionPresence::Unchanged);

    // relay_diagnostics advances via the per-emit fingerprint reconcile (the
    // configured relay set is in its snapshot) — assert on the emitted state.
    emit(&mut kernel);
    let (_, rd_presence) = emitted_state(&kernel, "relay_diagnostics");
    assert_eq!(
        rd_presence,
        ProjectionPresence::Changed,
        "relay_diagnostics must be Changed after the configured relay set changes"
    );
}

// ── Scenario 6: account switch -> epoch bump (F6) ─────────────────────────────

/// S6: switching the active account bumps the within-session epoch so Rung 3's
/// host re-baselines all projections. Real `set_active_account` + emit.
#[test]
fn s6_account_switch_bumps_epoch() {
    let (mut kernel, _) = kernel_at(6_000);
    emit(&mut kernel);
    let epoch_before = kernel.projection_manifest().epoch;

    kernel.set_active_account(ACCOUNT_PK.to_string());
    let epoch_after = kernel.projection_manifest().epoch;
    assert_eq!(
        epoch_after,
        epoch_before + 1,
        "account switch must bump the epoch (host re-baseline, F6)"
    );

    // Switching to the SAME account again must NOT bump (no real change).
    kernel.set_active_account(ACCOUNT_PK.to_string());
    assert_eq!(
        kernel.projection_manifest().epoch,
        epoch_after,
        "re-setting the same active account must not bump the epoch"
    );
    emit(&mut kernel);
}

// ── Scenario 7: relay status transition advances relay_diagnostics (F5) ────────

/// S7 (F5): a relay connection-state transition advances `relay_diagnostics` via
/// the per-emit fingerprint reconcile (the rev advances on the NEXT emit, not at
/// the mutation site; the oracle inside that emit would panic on a missed stamp).
/// Assert against the EMITTED state after a post-mutation emit.
#[test]
fn s7_relay_status_transition_advances_relay_diagnostics() {
    let (mut kernel, _) = kernel_at(7_000);
    emit(&mut kernel);
    let (rd_before, _) = emitted_state(&kernel, "relay_diagnostics");

    // A pure relay connection-state transition, then emit (fingerprint reconcile
    // + oracle run inside make_update).
    kernel.relay_connecting_url(RelayRole::Content, "wss://relay.example/");
    emit(&mut kernel);

    let (rd_after, rd_presence) = emitted_state(&kernel, "relay_diagnostics");
    assert!(
        rd_after > rd_before,
        "F5: a relay status transition must advance relay_diagnostics on the next \
         emit; {rd_before} -> {rd_after}"
    );
    assert_eq!(rd_presence, ProjectionPresence::Changed);
}

// ── Scenario 9: RAM-eviction of profiles advances profile rev (F4) ────────────

/// S9 (F4): RAM-tier eviction removes profile rows the `profile` / `accounts` /
/// `claimed_events` projections derive from. `evict_profiles_cache` must bump
/// `profiles_ver` when it removes ≥1 row, else the host serves a profile the
/// kernel no longer holds. Driven through the REAL `inject_replaceable_event`
/// (kind:0) population + `evict_ram_caches` path.
#[test]
fn s9_ram_eviction_of_profiles_advances_profile_rev() {
    use crate::kernel::ram_eviction::PROFILES_RAM_HWM;

    let (mut kernel, _) = kernel_at(9_000);

    // Populate just over the HWM so eviction has rows to drop. None are pinned
    // (no active account, no claims, no open views), so candidates exist.
    let over = PROFILES_RAM_HWM + 32;
    for i in 0..over {
        let pubkey = format!("{:0>64x}", 0x20000usize + i);
        let id = format!("{:0>64x}", 0x40000usize + i);
        kernel.inject_replaceable_event(
            &id,
            &pubkey,
            1_700_000_000 + i as u64,
            0,
            vec![],
            "wss://relay.test/",
            (1_700_000_000 + i as u64) * 1_000,
        );
    }

    emit(&mut kernel);
    let (rev_before, _) = live_state(&kernel, "profile");

    let report = kernel.evict_ram_caches();
    assert!(
        report.profiles_evicted > 0,
        "precondition: eviction must drop ≥1 profile row"
    );

    let (rev_after, _) = live_state(&kernel, "profile");
    assert!(
        rev_after > rev_before,
        "F4: RAM-eviction that drops profile rows must advance profile rev; \
         {rev_before} -> {rev_after}"
    );
    emit(&mut kernel);
}

