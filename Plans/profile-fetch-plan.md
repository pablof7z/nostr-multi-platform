# Plan: Two-phase profile + relay-list fetch for every visible pubkey

> Status: draft for user review. Once approved, the canonical entry belongs in
> `docs/BACKLOG.md` §4 (v1 backlog) — `Plans/profile-fetch-plan.md` is a review
> artifact only; per `AGENTS.md` planning-discipline rules it is not a
> long-lived top-level planning file.

## Context

Whenever a pubkey appears anywhere in the UI — as a note author, as a `p`-tag
reference, or as a `nostr:npub1…` / `nostr:nprofile1…` mention inside a note's
content — NMP should:

1. Immediately fetch **kind:0** from operator-configured **indexer relays** (the
   known-good profile-indexing relays).
2. Fetch **kind:10002** from indexer relays to learn which relays the pubkey
   publishes to.
3. Once kind:10002 is known, also fetch **kind:0** from those relays (the
   pubkey's own write relays typically carry the freshest kind:0).
4. Deduplicate so a pubkey already loaded recently is not re-fetched.

The user-visible outcome: every avatar, every display name, and every
`@-mention` resolves to real metadata within a single round-trip, regardless of
how the pubkey first arrived (timeline author, reply parent, quote `q`-tag,
content NIP-21 URI, etc.).

---

## Summary of current state

### What already works

The codebase ships ~80 % of the infrastructure described above. The pieces that
are already in production:

| Concern | Where it lives | Notes |
|---|---|---|
| "Indexer relays" as a substrate concept | `crates/nmp-core/src/relay.rs:41` (`FALLBACK_INDEXER_RELAY = "wss://purplepag.es"`); `crates/nmp-core/src/kernel/identity_state.rs:343-353` (`set_relay_edit_rows` derives `indexer_urls` from any relay row whose role contains `"indexer"`) | Fed into `SubscriptionLifecycle::set_indexer_relays` AND into the `indexer_relays_handle` typed slot the router reads via `SessionKeySet::indexer_relays`. |
| Router lane 6 (always-on indexer for discovery kinds) | `crates/nmp-router/src/router.rs:54-64`, `:241-248`, `:368-375` | kind:0, kind:3, kind:10000-19999 always also route to the indexer set, stacking on lane 1 (NIP-65 read/write set). Already defeats the kind:10002 self-seal loop. |
| Implicit kind:10002 probe for unknown-mailbox authors | `crates/nmp-core/src/subs/recompile.rs:112-164` | Every recompile walks each registered interest's `shape.authors`; any author with no cached mailbox AND not in `probed_mailboxes` gets an auto-emitted `kinds:[10002]` REQ to every indexer relay (batches of 500). |
| Mailbox-cache writer | `crates/nmp-router/src/ingest.rs::Kind10002Parser` registered via `EventIngestDispatcher` in `crates/nmp-app-template/src/lib.rs:200-210` | The single writer of `InMemoryMailboxCache`; the kernel's wildcard ingest arm observes the cache transition and fires `CompileTrigger::Nip65Arrived` so a recompile re-routes the author through their newly-known write set. |
| Note-author profile claim | `crates/nmp-core/src/kernel/requests/profile.rs:218-229` (`request_profile_for_rendered_note`) called from `crates/nmp-core/src/kernel/ingest/timeline.rs:138` for every ingested kind:1 | Pushes the author pubkey into `profile_requests.pending`; flushed by `pending_profile_claim_requests` (`profile.rs:302-376`) as one batched `kinds:[0]` REQ per resolved relay. Routes via `route_outbox_subscription_relays(author, kind=0, BootstrapSeed::IndexerOnly)` — warm authors hit NIP-65 write set (lane 1) + indexer (lane 6); cold-start hits indexer-only via lane 7 fallback. |
| Refcounted UI-driven profile claim | `crates/nmp-core/src/kernel/requests/profile.rs:140-216` (`claim_profile` / `release_profile`); FFI: `nmp_app_claim_profile` (`crates/nmp-ffi/src/timeline.rs:75`) | The refcount mechanism + per-pubkey claim bound (`MAX_CLAIMS_PER_PUBKEY = 256`, drop-newest, `claim_drops_total` metric). |
| Tag-walked p/e/q discovery | `crates/nmp-core/src/kernel/discovery.rs:67-86` (`collect_unknown_refs` → `UnknownIds::visit_tags`) called from `crates/nmp-core/src/kernel/ingest/timeline.rs:137`; drained by `drain_unknown_oneshots` (`discovery.rs:95-214`) | For every `p`-tag pubkey not already in `profiles` cache, issues a one-shot `kinds:[0,3,10002]` REQ in batches of ≤50. |
| Active-account self-bootstrap of kind:0 / 3 / 10002 / 10050 | `crates/nmp-core/src/kernel/requests/startup.rs::active_account_bootstrap_requests` | Migrated to `InterestRegistry::ensure_sub` + `CompileTrigger::ViewOpened` (PR #422). |

### What is broken / missing

Three concrete gaps stand between today's behaviour and the spec at the top:

1. **Content-level `nostr:npub1…` / `nostr:nprofile1…` mentions never trigger
   discovery.** `UnknownIds::visit_tags` (`crates/nmp-core/src/subs/unknown_ids.rs:68-104`) walks the
   event's tag array only. A note whose content embeds a NIP-21 URI for a
   pubkey that is NOT also present in a `p` tag never causes a kind:0 or
   kind:10002 fetch. The `nmp-content` tokenizer does produce
   `Segment::Mention(NostrUri::Profile { pubkey, .. })`
   (`crates/nmp-content/src/segment.rs:39`,
   `crates/nmp-content/src/tokenizer.rs:210`), but the kernel never consumes
   those segments for discovery. Result: any author whose visible display name
   only ever appears as `nostr:npub1…` in note bodies remains unresolved.

2. **kind:0 is never re-fetched after kind:10002 lands.**
   `profile_requests.requested` (a `BTreeSet`) is one-shot. The first cold-start
   call to `profile_claim_request` (`crates/nmp-core/src/kernel/requests/profile.rs:378-413`)
   issues kind:0 through `BootstrapSeed::IndexerOnly` (the indexer-only seed —
   correct, because no mailbox is known yet). When kind:10002 arrives later
   revealing the author's real write set, no second kind:0 REQ goes out against
   those relays — even though the user's own write relays usually carry the
   freshest kind:0. Only `relay_lifecycle.rs:130-142` ever transfers a pubkey
   back from `requested` to `pending` (and only on indexer-socket teardown).
   The "freshest kind:0 from author's own relays" half of the spec is missing.

3. **`claimProfile` is wired through the FFI but never called from the
   Swift/iOS shell.** `claimProfile` / `releaseProfile` exist in
   `ios/Chirp/Chirp/Bridge/KernelBridge.swift:140-156` but no view in the iOS
   tree calls them. The author-of-an-ingested-note path covers timeline + open
   thread automatically (gap (1) notwithstanding); the question is whether
   visible-mention pubkeys (which are NOT event authors) need a Swift-side
   `claim` call or whether closing gap (1) in Rust makes the Swift call
   unnecessary. **The plan picks the second answer** — gap (1) is sufficient
   for v1; no Swift wiring is required.

---

## Root cause

Each gap has the same shape: a substrate seam exists, but no producer is
feeding it.

- **Gap 1**: `UnknownIds` has a `note_pubkey(pk, has_pubkey)` entry point
  (`crates/nmp-core/src/subs/unknown_ids.rs:121-129`) explicitly designed for "a
  higher layer pulled a pubkey out of content"; nothing in the ingest pipeline
  calls it for content-extracted mentions.
- **Gap 2**: `CompileTrigger::Nip65Arrived` fires on every cache transition
  (`crates/nmp-core/src/subs/trigger.rs:64`), but the only consumer is the
  planner's `recompile_and_diff`, which re-routes *open* subscriptions through
  the newly-known mailbox. The ad-hoc one-shot `profile_claim_request` path
  isn't a registered interest, so it doesn't get re-routed.
- **Gap 3** is downstream of gap 1: every author that appears anywhere in the
  rendered tree already enters `profile_requests.pending` via the
  `request_profile_for_rendered_note` ingest hook. The missing population path
  is the content-extracted-mention pubkey; once that path exists, the
  `claim_profile` refcount API becomes a UI nicety, not a v1 requirement.

A secondary observation: **kind:0 ingest still lives in `nmp-core`** at
`crates/nmp-core/src/kernel/ingest/profile.rs::ingest_profile`. Per `D0` and
the precedent set by PR #487 (which deleted the kind:10002 ingest leak and
moved it into `nmp-router`'s `Kind10002Parser`), kind:0 ingest is a D0 leak
that should also move to a Layer-4 parser. This plan **flags it** as a
follow-up rather than coupling it to the demand-signal work — the leak is
pre-existing, doesn't block any spec point, and conflating the two doubles
the review surface. **Action item for the PR that implements this plan**:
file `crates/nmp-core/src/kernel/ingest/profile.rs::ingest_profile` as a new
`BACKLOG.md` §1 violation entry (verified absent from the file as of
2026-05-25); per `AGENTS.md` planning-discipline rules, a known violation
must live in `BACKLOG.md` to be a tracked plan, not just in a memory file.

A third observation worth surfacing for the reviewer (not a gap, but a
consequence of these diffs): the codebase will then carry **two parallel
profile-discovery paths**. (a) Note-author claims flow through
`profile_requests.{pending,requested}` → `pending_profile_claim_requests`
(kind:0-only batched REQ). (b) Tag p-refs AND the new content-mention
pubkeys flow through `UnknownIds` → `drain_unknown_oneshots`
(kinds:[0,3,10002] one-shot REQ). Both dedupe against `profiles.contains_key`
and a different in-flight set, so they cannot duplicate work; routing for
both is the same router lane stack (1 + 6 + 7). **Accepted as v1
fragmentation** — unifying them onto `LogicalInterest` is the PD-033-C
Option B work referenced under Diff 2. Flagging it here so a post-merge
reviewer does not file it as a fresh duplication violation.

---

## Proposed architecture

Three diffs, in priority order. Each is independently mergeable; the demand-set
seam (step 2) is the load-bearing one.

### Diff 1 — Content-mention extractor

A new ingest parser, registered against kind:1 (short text note) and the kinds
that carry rendered content (kind:6 repost wraps, kind:9 / kind:11 chat,
kind:30023 long-form), walks the event content for NIP-21 URIs and feeds
extracted pubkeys into the kernel's existing `UnknownIds` collector. Reuses the
established `IngestParser` / `EventIngestDispatcher` substrate seam exactly the
way `Kind10002Parser` does today.

**Layering**: lives in `crates/nmp-nip01/src/mention_extractor.rs` (kind:1 is
the canonical short-text-note owner; reposts in kind:6 / kind:11 wrap a kind:1
inside). The parser uses **`nmp_core::nip21::parse_nostr_uri`** — that helper
already exists at substrate level (`crates/nmp-core/src/nip21.rs:115`) and
returns `NostrUri::Profile { pubkey, relays }`. **Avoids pulling `nmp-content`
into the dependency graph**: full ContentTree tokenization is more than the
discovery path needs; a lightweight `nostr:(npub1|nprofile1)…` regex
(mirroring `crates/nmp-content/src/regex_set.rs:47`) over the raw content
string is enough. The parser produces only pubkeys; routing/REQ emission stays
in `nmp-core::kernel`.

**Substrate seam needed**: `UnknownIds` is kernel-private state today
(`crates/nmp-core/src/kernel/mod.rs` field). It must become reachable from the
ingest dispatcher. Pattern: extend the `IngestParser` trait (already on the
`EventIngestDispatcher`) to receive a *substrate-level demand sink* the kernel
hands to every parser invocation. Concretely:

```rust
// crates/nmp-core/src/substrate/mod.rs (new sibling)
pub trait PubkeyDemandSink: Send + Sync {
    /// Mark a pubkey as "the UI is about to render this — please fetch kind:0
    /// + kind:10002 if not already known". Idempotent; the sink owns dedup.
    fn note_pubkey(&self, pubkey: &str);
}
```

Wire it into the existing `EventIngestDispatcher::dispatch` so each parser
sees an `&dyn PubkeyDemandSink` alongside the event. The kernel's
implementation is a thin shim that calls the existing
`UnknownIds::note_pubkey(pk, |pk| self.profiles.contains_key(pk))` on its
private collector, then enqueues `CompileTrigger::ViewOpened` so
`drain_unknown_oneshots` runs on the next tick (the existing
discovery-oneshot path is the renderer for the seam, with no kernel-side NIP
nouns added).

**Scope: medium.** New parser (~80 LOC + tests), new substrate trait (~30 LOC),
dispatcher signature change rippling through registered parsers (~ten call
sites — the dispatcher is small).

### Diff 2 — kind:0 re-fetch on `Nip65Arrived`

When kind:10002 lands for a pubkey whose kind:0 was already requested through
the indexer-only seed, re-issue kind:0 against the author's freshly-known
write set. Two implementation options, picked by tradeoff:

- **Option A (minimal):** in `crates/nmp-core/src/kernel/ingest/mod.rs` (the
  wildcard ingest arm that already snapshots the `MailboxCache` before/after
  `verify_and_persist` and fires `CompileTrigger::Nip65Arrived`), additionally
  call a new `Kernel::refresh_profile_after_mailbox(pubkey)` helper. That
  helper checks `profile_requests.requested.contains(pubkey)` (we have already
  asked the indexer for their kind:0) AND `mailbox_cache.write_relays(pubkey)`
  is now `Some(_)` — and if so, transfers the pubkey from `requested` → `pending`,
  triggering the next `pending_profile_claim_requests` to re-batch the kind:0
  REQ. The router's lane-1 will route through the new write set; lane-6 still
  stacks on indexer, so the result is "kind:0 fetched from indexer ∪ author's
  own writes" — exactly the spec point.

- **Option B (cleaner, larger):** convert `profile_claim_request` to a
  registered `LogicalInterest` { kinds:[0], authors:[pk], limit:1, OneShot }.
  When kind:10002 lands and fires `Nip65Arrived`, the planner's
  `recompile_and_diff` re-resolves every registered interest's per-author
  relay set; the kind:0 OneShot re-emits to the new write set as part of
  ordinary plan-diff. Aligns with the M2 migration notes that already plan
  this (`crates/nmp-core/src/kernel/requests/profile.rs:32-49`).

**Recommendation: Option A for v1**, with Option B as the post-v1 cleanup
under PD-033-C (the ongoing M2 migration of the M1 `req()` helpers). Option A
is ~20 LOC; Option B is the next chapter of a multi-PR migration already in
flight and would force this plan to wait on PD-033-C Stage 5+.

**Scope: small** (Option A) / large (Option B; out of scope for this plan).

### Diff 3 — Cross-kind dedupe TTL (optional)

The spec says "deduplicate: if we've already loaded kind:0 for a pubkey
recently, skip the fetch". Today's dedup is binary (in `profiles` → never
re-fetch). For v1 this is acceptable; the dynamic kind-0 refresh question
(profile-picture-changed-since-last-fetch) is a separate user-facing decision
and not in scope.

**No code change required for v1.** This bullet is documented here so a future
reader does not re-open it as a missed scope item.

---

## Implementation steps (priority order)

| # | Step | File locations | Scope |
|---|---|---|---|
| 1 | Add `PubkeyDemandSink` substrate trait. Extend `EventIngestDispatcher::dispatch` signature to thread `&dyn PubkeyDemandSink` to every parser. Existing parsers (`Kind10002Parser`, NIP-17's `Kind10050Parser`) ignore the new arg. | `crates/nmp-core/src/substrate/{mod,parser}.rs`; ripple to call sites in `nmp-router`, `nmp-nip17`. | small |
| 2 | Kernel impls `PubkeyDemandSink` for itself (private newtype `KernelDemandSink<'k>`); the wildcard ingest dispatch in `kernel/ingest/mod.rs` constructs one per call and passes it to the dispatcher. The sink delegates to `self.unknown_ids.note_pubkey(pk, predicate)` + sets a flag so `drain_unknown_oneshots` is invoked on the next tick boundary (mirror the `CompileTrigger::ViewOpened` enqueue at the end of `drain_unknown_oneshots`). | `crates/nmp-core/src/kernel/ingest/mod.rs`, `crates/nmp-core/src/kernel/discovery.rs`. | small |
| 3 | New `crates/nmp-nip01/src/mention_extractor.rs`: implements `IngestParser` for the kinds whose `content` field carries renderable text — **kind:1** (short note) and **kind:30023** (long-form). kind:6 reposts: register too because the wrapped event JSON may carry npub mentions that get rendered. NIP-28 (kind:9 / kind:42) and NIP-29 (kind:11) are out of scope for v1 (no production renderer for them); add them when the renderer lands. The parser runs a `regex` over the event content; for each `nostr:(npub1|nprofile1)…` decoded with `nmp_core::nip21::parse_nostr_uri` whose `pubkey` is hex64-valid, calls `sink.note_pubkey(&pubkey)`. Skips when pubkey is already an event tag (the existing tag-walker covers that). | `crates/nmp-nip01/src/{lib,mention_extractor}.rs`; tests in `crates/nmp-nip01/src/mention_extractor/tests.rs`. | medium |
| 4 | Register the kind:1 (and friends) parser in `nmp-app-template::register_defaults` next to the existing kind:10002 / kind:10050 registrations. | `crates/nmp-app-template/src/lib.rs` (~5 LOC addition). | small |
| 5 | Diff 2 Option A: new `Kernel::refresh_profile_after_mailbox(pubkey)`. Call it from `kernel/ingest/mod.rs`'s mailbox-change observer, right alongside the existing `CompileTrigger::Nip65Arrived` enqueue. The helper mutates the set only; the actual REQ flush rides the existing tick boundary — `pending_profile_claim_requests` is already invoked from `maybe_open_timeline` (`crates/nmp-core/src/kernel/ingest/timeline.rs:269`) on every ingested kind:1 AND from the actor's `pending_view_requests` path that fires on relay-connect. **No new immediate-emit path** — D8: piggy-back on an existing tick. Test: starting from `profile_requests.requested = {pk}` and an empty mailbox cache, upsert a non-empty write set into the cache → assert `profile_requests.pending` now contains `pk`, then call `pending_profile_claim_requests` and assert the kind:0 REQ is emitted against the new write set. | `crates/nmp-core/src/kernel/requests/profile.rs` (helper); `crates/nmp-core/src/kernel/ingest/mod.rs` (call site). | small |
| 6 | Tests: end-to-end against the existing in-process fixtures. Verify a kind:1 note with `nostr:npub1…` in content + no matching `p` tag drives a kind:0 + kind:10002 REQ to the indexer set within one tick. Verify the kind:10002 ingest fires a second kind:0 REQ to the discovered write set. Use `crates/nmp-core/src/kernel/profile_claim_tests.rs` as the template. | `crates/nmp-nip01/`, `crates/nmp-core/src/kernel/`. | small |

---

## What must NOT be done

- **No kind:1, kind:6, or kind:30023 parsing in `nmp-core`.** Per `D0` and the
  PR #487 precedent that just cleaned up the kind:10002 ingest leak, all
  NIP-named ingest must live in Layer-4 crates registered through
  `EventIngestDispatcher`. The kernel must learn about the new pubkey demand
  through the trait-only `PubkeyDemandSink` seam.
- **Do not pull `nmp-content` into `nmp-core` or `nmp-nip01`'s ingest path.**
  Full ContentTree tokenization (markdown, media classification, hashtags,
  invoices) is far more than the discovery path needs. A minimal regex over
  the raw content string — duplicating `crates/nmp-content/src/regex_set.rs:47`
  in scope but not in code — keeps the dep graph clean. The shared
  `parse_nostr_uri` already lives at substrate level (`nmp_core::nip21`).
- **Do not introduce a new indexer-relay configuration mechanism.** The
  operator's `RelayEditRow { role: "indexer" }` rows already feed both the
  router (`indexer_relays_handle` slot via `set_relay_edit_rows`) and the
  implicit-discovery probe (`lifecycle.set_indexer_relays`). `FALLBACK_INDEXER_RELAY`
  covers cold-start.
- **Do not duplicate router lane 6.** The router already always-on includes
  indexer relays for kind:0, kind:3, kind:10000-19999 (`crates/nmp-router/src/router.rs:241`,
  `:368`). The plan rides on that lane; it does not add a parallel routing
  rule.
- **Do not modify `claim_profile` / `release_profile` refcount semantics.** The
  bounded `MAX_CLAIMS_PER_PUBKEY` / `claim_drops_total` retention contract is
  load-bearing for D8 (`crates/nmp-core/src/kernel/profile_claim_tests.rs`).
  The plan adds a parallel ingest-driven demand path; the refcount API stays
  available for any future Swift view that wants explicit hold semantics
  (preview-on-press-and-hold, etc.).
- **Do not couple this plan to moving kind:0 ingest out of `nmp-core`.** That
  D0 cleanup mirrors what PR #487 did for kind:10002, but it is an
  independent refactor; conflating it with the demand-signal work doubles the
  PR surface and the review burden. Open it as a separate `BACKLOG.md` §1
  violation entry if not already there.
- **Do not change `CompileTrigger::Nip65Arrived` semantics.** The existing
  cache-transition observer is correct and load-bearing for the planner's
  re-routing. Diff 2 piggy-backs on the existing transition; it does not
  add a second trigger variant.
- **Do not add the iOS Swift `claimProfile` call.** Author-of-rendered-note
  coverage from the ingest pipeline (gap (1) closed) is sufficient for v1.
  Reserving the Swift call for a later "show profile preview on long-press"
  surface keeps v1 scope honest.

---

## Estimated scope summary

| Diff | Files touched | LOC delta | Scope |
|---|---|---|---|
| 1 (substrate seam + content extractor) | ~6 files across `nmp-core`, `nmp-nip01`, `nmp-router`, `nmp-nip17`, `nmp-app-template` | +200 / -10 | medium |
| 2 (kind:0 re-fetch on Nip65Arrived, Option A) | 2 files in `nmp-core/src/kernel/` | +40 / -0 | small |
| 3 (TTL dedupe) | n/a | 0 | not in v1 |

**Total: one medium + one small PR**, mergeable in sequence. Each PR ends with
the always-on local gates per `AGENTS.md`:
`cargo test -p nmp-core --lib`, `cargo test -p nmp-router`,
`cargo test -p nmp-nip01`, `cargo test -p nmp-nip17`,
`cargo test -p nmp-app-template`, and
`cargo test -p nmp-testing --test doctrine_lint_smoke`.

---

## Coordination

- **PR #487 (`refactor/delete-kind10002-leak`)** has already deleted the
  kernel-side `match event.kind { 10002 => ... }` arm and the
  `kernel/ingest/relay_list.rs` file. The mention-extractor work in step 3
  must register against the *post-#487* `EventIngestDispatcher` API (the
  parser-vs-wildcard-arm distinction). Rebase before starting.
- **PR #484 (`move-nip65-resolver-to-router`)** is also in flight on the same
  routing surface but only relocates a publish-side resolver; no conflict
  expected.
- **PD-033-C Stages 5+** would move the ad-hoc `profile_claim_request` over
  to `LogicalInterest` (Option B above). When that stage starts, the helper
  added in step 5 becomes the migration target — collapse it then, not now.

---

## Post-merge follow-ups (NOT part of this plan)

- D0 cleanup: move `Kernel::ingest_profile` (`crates/nmp-core/src/kernel/ingest/profile.rs`)
  out into a `Kind0Parser` in `nmp-nip01`, mirroring `Kind10002Parser` in
  `nmp-router`. Track as a `BACKLOG.md` §1 violation if not already filed.
- TTL-based kind:0 refresh (the "freshness" half of the user spec). Requires a
  product decision on when a cached kind:0 is "stale enough to re-fetch".
- iOS Swift `claimProfile` call from a "long-press profile preview" surface,
  if/when that UI exists.
