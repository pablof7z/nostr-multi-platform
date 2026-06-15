---
type: research-record
date: 2026-06-15
session: 78b50727-bccd-4088-8493-a07624a4fa83
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78b50727-bccd-4088-8493-a07624a4fa83.jsonl
source_lines: 963-1098
agent_attribution: a94105b925cd09e96
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Architectural research report resolving four pre-registered open questions (Q1-Q4) about unified ingest chokepoint design: Q1 [RESOLVED-IN-CODE] clean split at mod.rs:281/282, Q2 [SUBJECTIVE-OWNER-DECISION] contacts planner coupling prevents parser migration in first PR, Q3 [RESOLVED-IN-CODE] no coverage breakage, Q4 [SUBJECTIVE-OWNER-DECISION] GC wired but truncation-skip path must be tested"
captured_at: 2026-06-15T09:17:34Z
---

Architectural research report resolving four pre-registered open questions (Q1-Q4) about unified ingest chokepoint design: Q1 [RESOLVED-IN-CODE] clean split at mod.rs:281/282, Q2 [SUBJECTIVE-OWNER-DECISION] contacts planner coupling prevents parser migration in first PR, Q3 [RESOLVED-IN-CODE] no coverage breakage, Q4 [SUBJECTIVE-OWNER-DECISION] GC wired but truncation-skip path must be tested

---

User: [Agent task result: Agent "Close open questions in code" completed]
I now have complete evidence for all four questions. Let me compile the final report.

---

# Architectural Research Report: Unified kind-agnostic ingest chokepoint

Read `docs/plans/arch-fixes.md` (the plan) plus the live ingest, contacts, profile, status, GC, and eviction sources. All four questions resolved with code evidence below.

---

## Q1 — Chokepoint placement vs `handle_event`'s relay bookkeeping

**There is a clean split point.** `handle_event` (`crates/nmp-core/src/kernel/ingest/mod.rs:245-426`) does two distinct things in sequence: relay-frame bookkeeping (lines 252-280), then a `claim_expansion_match_author` lookup (281), then the kind dispatch (296-425). The bookkeeping block is cleanly separable.

**Lines that must stay relay-only (`ingest/mod.rs:252-280):**
- `:252-255` — JSON→`NostrEvent` parse (relay wire path; local/replay already have a typed event).
- `:257` — `let now = Instant::now()` (relay arrival timing).
- `:258-262` — per-relay counters: `relay.counters.events_rx += 1`, `relay.last_event_at = now`.
- `:263` — `self.record_transport_event(role, relay_url, now)` (transport provenance).
- `:264` — `self.events_since_last_update += 1` (emit-cadence metric, arguably belongs to any accepted event, but today is relay-keyed).
- `:265-266` — `self.timing.last_event_at` / `first_event_at` (relay-stream timing milestones).
- `:267-280` — `wire_key_url` canonicalization + wire-sub diagnostics: flips sub `"opening"→"live"`, bumps per-sub `events_rx`/`last_event_at` (lines 273-280). This is pure relay-frame diagnostics keyed by `(canonical_url, sub_id)`.
- `:281` — `claim_match_author = self.claim_expansion_match_author(sub_id, &event)` — **relay-only**: it is sub_id-driven (claim-expansion REQ tracking). Local publish and replay have no sub_id.

**The shared seam = everything from the `match event.kind` at `:296` onward**, but note the current per-kind arms (`:296-425`) fuse three things the plan wants split: `verify_and_persist` (kind-agnostic, already shared), then per-kind cache mutation (`ingest_profile`/`ingest_contacts`/`ingest_timeline_event`), then `notify_event_observers`. The `record_claim_expansion_hit` calls inside each arm (`:301, 314, 331, 393`) are the only other relay-only residue inside the match — they all guard on `claim_match_author`, which a local/replay caller would simply pass as `None`.

**Concrete recommendation for `ingest_accepted_event(source, event)`:** place it to receive a typed `NostrEvent` plus a `source` enum carrying `{ provenance: String, claim_match_author: Option<String>, sub_id: Option<String> }`. The relay path runs lines 252-281 then calls it; local publish (`publish_engine.rs:169` → `record_local_publish_intent`) and replay call it directly with `provenance="local://publish"` / replay provenance and `claim_match_author=None`. The body is the `match event.kind` block with `notify_event_observers` pulled into `verify_and_persist` per plan step 1. The plan's claim in §2(b) (codex: "don't route local through handle_event literally") is correct and verified — lines 252-281 are genuinely relay-shaped.

One subtlety: `ingest_timeline_event` (kind 1|6) currently does NOT call `verify_and_persist`; it has its own duplicate `store.insert` + sig-verify (`timeline.rs:51-179`). The chokepoint unification must route 1|6 through the same `verify_and_persist` and demote the timeline-cache append to an observer (plan steps 3-4).

**[RESOLVED-IN-CODE]** — clean split at `ingest/mod.rs:281/282`; lines 252-281 are relay-only, the kind-match at 296+ is the shared body.

---

## Q2 — Can `self.profiles` and `self.seed_contacts` become parser/observer-owned caches?

Field defs: `profiles: HashMap<String, Profile>` (`kernel/mod.rs:639`); `seed_contacts: HashMap<String, Vec<String>>` (`kernel/mod.rs:685`). Writers today: `ingest_profile` (`profile.rs:20`), `ingest_contacts` (`contacts.rs:260`), plus `prepopulate_seed_contacts` (`mod.rs:2438`, called from identity sign-in `actor/commands/identity.rs:1032`).

### Exhaustive reader classification — `self.profiles`

| Site | Purpose | Class |
|---|---|---|
| `update.rs:119,126` | snapshot counter `profile_events` / store-bytes | (ii) loose — count only |
| `ram_eviction.rs:374,393,408` | RAM eviction of the profiles map | (i) **owns the map** |
| `status.rs:181,220,386` | `Profile(...)` interest state, claim "loaded" count, byte estimate | (i) sync read |
| `diagnostic_counters.rs:83` | `lnurl_for_pubkey` — NIP-57 zap destination resolution | (i) **sync, load-bearing** |
| `ingest/profile.rs:13,20` | the writer (supersession compare) | writer |
| `requests/profile.rs:117,220,228,291` | TTL gate / pending-claim dedup ("do I already have this profile?") | (i) **sync, load-bearing** |
| `update/views.rs:185` (`profile_for_pubkey`) | profile enrichment of timeline items / accounts | (i) **sync, hot path** |
| `requests/relay_lifecycle.rs:142` | filter authors needing profile fetch | (i) sync |
| `projection_rev/source_versions.rs:134,153` | `profiles_ver` revision tracking | (ii) version counter |
| `typed_projections/mod.rs:364` | profile-cluster typed projections | (i) sync read for snapshot |

### Exhaustive reader classification — `seed_contacts`

| Site | Purpose | Class |
|---|---|---|
| `ram_eviction.rs:423,435,446` | eviction | (i) owns map |
| `actor/commands/identity.rs:1032` | `prepopulate_seed_contacts` at sign-in (BEFORE any kind:3 relay arrival) | **writer, non-ingest** |
| `status.rs:397` | byte estimate | (ii) count |
| `update.rs:127,166` | snapshot counters | (ii) count |
| `ingest/timeline.rs:397` (`should_open_timeline`) | "does active account have contacts?" milestone gate | (i) sync, load-bearing |
| `ingest/contacts.rs:303` (`register_follow_feed_for_active_account`) | re-derive follows for active account on screen re-entry | (i) sync, load-bearing |

### The decisive finding: `ingest_contacts` is FAR more than a cache write

`ingest_contacts` (`contacts.rs:228-262`) does, in order:
1. `capped_contact_follows` extraction (`:234`)
2. enqueue `CompileTrigger::FollowListChanged` (`:245-249`) — **lifecycle/planner side effect**
3. **active-account-gated** `sync_follow_feed_interests(&follows)` (`:255-257`) — which itself (`contacts.rs:86-175`) withdraws/pushes `LogicalInterest`s into the registry, rebuilds `self.timeline_authors` (`:131`), calls `flush_pre_kind3_buffer()` (`:137`), and runs `enqueue_interest_cache_serve_deferred` + `run_cache_serve_step()` (ADR-0045 cache-serve, `:161-174`).
4. only THEN writes `self.seed_contacts.insert` (`:260`).

These are kernel-owned planner/lifecycle effects, gated on `self.active_account` — a parser registered via `IngestParser` (which receives a `VerifiedEvent` with no kernel `&mut self`, no `active_account` context, no lifecycle access) **cannot** drive them. The `nmp-nip17`/`nmp-nip65` parser pattern works precisely because those parsers write a self-contained cache and the kernel re-reads it via a narrow capability trait + a post-dispatch snapshot-diff (`on_mailbox_changed`/`on_dm_relays_changed`, `ingest/mod.rs:386-421`). For contacts, the "side effect" is not a cache the kernel reads back — it is direct mutation of the planner registry and `timeline_authors`, which is the relevance set itself.

Additionally `seed_contacts` has a **non-ingest writer** (`prepopulate_seed_contacts` at sign-in, `identity.rs:1032`) and a non-ingest consumer (`register_follow_feed_for_active_account`, called on `OpenContactFeed`). Moving the cache out of the kernel would require those two paths to also go through the parser-owned cache, widening blast radius well beyond ingest.

`self.profiles` is closer to movable (it has the `lnurl_for_pubkey` zap-destination reader and the TTL/claim-dedup readers in `requests/profile.rs`, all of which are simple `get`/`contains_key` and could route through a capability trait like `DmInboxRelayLookup`), but it is still read synchronously at ~10 sites including the hot `profile_for_pubkey` enrichment path (`views.rs:185`).

### PR-SCOPE VERDICT

**Do NOT move profile/contacts to parser/observer-owned caches in THIS PR.** Keep `ingest_profile` and `ingest_contacts` as kernel-owned steps **fed by the chokepoint** (i.e. the chokepoint calls them post-`verify_and_persist`, gated on `Inserted|Replaced`, exactly as the arms do today), and scope this PR to:
1. pull `notify_event_observers` into `verify_and_persist` (step 1),
2. introduce `ingest_accepted_event(source, event)` below relay bookkeeping (step 2),
3. **decouple `should_store_event` from `store.insert`** — route kind:1|6 through `verify_and_persist`, demote `should_store_event` to the timeline-cache observer's read-time predicate (steps 3 + the timeline half of 4),
4. unify the source paths (steps 5-6) and the version cut (step 8).

This delivers the entire #1442/#1440 fix (complete store, read-your-writes, no relevance holes) without taking on the planner-coupling risk. Plan step 4's "demote profile/contacts caches to observers/parsers (kill kind-match arms)" should be **descoped to a follow-up** (it is the crate-boundaries.md §4.2 finish-line, not required for the bug fix). The plan's "single atomic PR" claim still holds for the bug — it's the *over-reach* of full profile/contacts parser migration that's risky, and that piece is independently shippable later.

**[SUBJECTIVE-OWNER-DECISION]** — the readers are enumerated and the contacts→planner coupling is a hard code fact, but "split the PR (timeline+admission now, profile/contacts parser migration later)" vs "do the full migration in one PR with new capability-trait read seams for `profiles`/`seed_contacts` + reroute the two non-ingest writers" is a scope judgment. **My strong recommendation: split.** The `ingest_contacts` planner side effects (`contacts.rs:245-258`) cannot live in an `IngestParser` and are the riskiest part of the migration; forcing them into this PR jeopardizes the whole atomic change.

---

## Q3 — Snapshot/coverage/`status.rs` milestone flags keyed off timeline state

The milestone/coverage machinery is keyed off the **read-caches and wire-sub state, NOT off `self.store` counts** — so decoupling admission from persistence is safe here.

- `timeline_requested` / `timing.timeline_opened_at`: set in `maybe_open_timeline` (`timeline.rs:375-387`), gated by `should_open_timeline` (`timeline.rs:389-403`) which reads `self.seed_contacts` + `contacts_deadline` — **not store counts**. Unaffected.
- `status.rs:195-208` "Timeline" interest row: `state` from `!self.timeline.is_empty()` (the read-cache `VecDeque`), `cache_coverage` from `self.timeline_requested`. Reads the read-cache, not the store. **Unaffected** — and in fact `self.timeline` only ever receives followed-author / firehose ids (`timeline.rs:289`), so it stays relevance-shaped by construction even after the store gets everything.
- `status.rs:181,220` profile interest rows: `self.profiles.contains_key` — read-cache, unaffected.
- EOSE coverage write (`eose.rs:49-52`, `record_eose_coverage`): keyed off the **wire-sub's `since_floor`** (filter coverage per `(filter_hash, relay)`), not store contents (`eose.rs:41-51`). The coverage ledger is per-REQ-shape, so persisting extra events does not perturb it. **Unaffected.**
- `compute_estimated_store_bytes` (`status.rs:373-398`) and the snapshot counters (`update.rs:119-166`) iterate `self.events` / `self.profiles` / `self.seed_contacts` — the **read-caches**, not `self.store`. They will report the read-cache size, which is unchanged by storing more events in `self.store`. **No wrong values.**

**One thing to watch (not a break):** `metric_stored_events` / `metric_note_events` (`mod.rs:653-658`) are maintained at the `self.events` read-cache insert site (`timeline.rs:246-250`), not at `store.insert`. They count read-cache entries, so they remain timeline-shaped. After the fix, if you want a "true store size" diagnostic you'd read the store directly; today nothing assumes `metric_stored_events == store size`, so nothing breaks.

**[RESOLVED-IN-CODE]** — every milestone/coverage site reads read-caches (`self.timeline`, `self.profiles`, `self.seed_contacts`, `self.events`) or wire-sub `since_floor`, never `self.store` event counts. No site assumes store contents are timeline-relevant. No adjustments required.

---

## Q4 — Store-size / write-volume impact of persisting everything

**The prior "gc_step never called in production" finding is STALE — GC is now wired** (#1069). Production call chain:
- `actor/mod.rs:2332-2335`: `if running && last_gc.elapsed() >= GC_TICK_INTERVAL { kernel.run_gc_step(); }` — fires at most once per 60s on the actor idle tick, piggybacking the existing ≤250ms wake (no polling, D8). The comment at `:2320` explicitly documents it as the fix for "audit Finding 1: gc_step was never called in production."
- `kernel/mod.rs:2023-2065` `run_gc_step`: runs RAM-tier eviction (`evict_ram_caches`) + LMDB-tier `gc_step_with_pins_and_coverage` on `GcBudget::production()`.

**Budget (`store/types/gc.rs`):**
- `HOT_EVENT_CEILING = 10_000` (`:16`) — LRU evicts least-recently-accessed **un-pinned** events down to 10k.
- `GC_MAX_EVENTS_PER_STEP = 2_000`, `GC_MAX_DURATION_MS = 50` (`:19,26`) — bounded per-tick scan.

**Does removing the relevance gate risk unbounded growth?** The relevance gate WAS effectively acting as a write-volume limiter for kind:1/6 (today non-followed, non-interest-matching kind:1/6 are dropped at `should_store_event`, `timeline.rs:25,299-329`). After the fix they all persist. But:

1. The newly-stored events are exactly the ones that are **NOT pinned**. The pin set (`ram_eviction.rs:274-291` `derive_store_pin_set`) pins `self.timeline` ids + `event_claims` + open-view working set + floor-coherent shapes. A non-followed author's note that matched no interest and no claim is **unpinned**, so it is the first thing LRU evicts toward the 10k ceiling. So the relevance gate's volume-limiting role is *replaced by* LRU eviction of the same class of events — this is by design (plan §5: "GC/watermark is the ONLY storage bound").
2. Bounded growth ceiling = 10k hot events between 60s ticks. Worst case is the transient burst between GC ticks; the 60s cadence + 2000-events/50ms scan budget bounds it.

**The one real caveat (truncation→no-eviction):** `derive_store_gc_inputs` (`ram_eviction.rs:305-318`) sets `max_total_events = usize::MAX` (LRU **disabled** for that tick) whenever the floor-coherent pin scan was truncated by its D8 budget (`add_floor_coherent_pins` returned `false`). So on a device with many active floored shapes where the pin scan keeps truncating, LRU eviction could be skipped repeatedly while persistence is now unbounded-per-kind. Pre-fix this was masked because kind:1/6 volume was relevance-gated; post-fix the store could accumulate faster than the (sometimes-skipped) LRU drains. NIP-40 expiry + tombstone reaping still run (those don't need the pin scan), but the LRU ceiling is the bound that matters for non-followed content.

**Verdict:** GC/LRU is wired and structurally adequate (10k ceiling, unpinned-first eviction, 60s cadence). This PR does **not** need to wire GC. It **should** include a test that under the new "store everything" regime, non-followed kind:1 events stay unpinned and are reaped to the ceiling, and it should at least confirm the truncation→no-eviction path (`ram_eviction.rs:309-316`) does not regress into unbounded growth under realistic floored-shape counts. Whether to additionally tune `HOT_EVENT_CEILING` or harden the truncation fallback is a judgment call.

**[SUBJECTIVE-OWNER-DECISION]** (with a strong resolved-in-code core): GC is **resolved-in-code as wired and bounded** (the stale "never called" finding is corrected). The genuine owner decision is narrow: whether to harden the `derive_store_gc_inputs` truncation→`usize::MAX` (LRU-skip) fallback (`ram_eviction.rs:309-316`) and/or revisit the 10k ceiling before flipping persistence to "store everything." **Recommendation:** keep the ceiling, but treat the truncation-skip path as a must-verify in this PR's test plan (it is the only place where unbounded persistence could outrun a disabled LRU) — do not ship the persist-everything change without a test that exercises it.

---

### Bottom line on the single-atomic-PR plan
- **Q1, Q3:** green. Clean seam, no milestone/coverage breakage.
- **Q4:** green-with-a-test-caveat. GC is wired (finding stale); the truncation→LRU-skip path is the one thing to verify.
- **Q2 is the real risk to the plan.** The plan's step 4 ("demote profile/contacts caches to observers/parsers, kill kind-match arms") is **not safely doable in this PR** because `ingest_contacts` drives planner/lifecycle side effects (`contacts.rs:245-258`) that an `IngestParser` structurally cannot reach, plus `seed_contacts` has a non-ingest writer at sign-in. Recommend descoping profile/contacts parser migration to a follow-up and shipping the timeline-admission decoupling (the actual #1442/#1440 fix) now. The PR stays atomic for the *bug*; only the architectural finish-line for §4.2 is deferred.
