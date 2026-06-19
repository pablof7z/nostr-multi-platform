# ADR-0061 — Canonical, app-agnostic feed surface (descriptor open + viewport intent)

- **Status:** Accepted; foundation landed (ladder steps 1–3). Shell migration +
  deletion of the app-named symbols + proof gates follow in later PRs (§5).
- **Date:** 2026-06-19
- **Doctrine:** `doctrine:d0` (substrate names no app noun), `doctrine:d4`
  (single writer), `doctrine:d5` (bounded), `doctrine:d6` (FFI fail-closed),
  `doctrine:d8` (no polling)
- **Related:**
  - **ADR-0033** (`nmp-feed` viewport FFI — the generic `nmp_app_load_older_feed`
    viewport-intent ABI). This ADR **supersedes its per-call `load_older`
    trigger**: the shell no longer decides *when* to page. ADR-0033's mechanism
    (a feed riding a `FeedController`) is preserved and reused.
  - **ADR-0039** (push projection seam is canonical; reject generic pull
    snapshots). **Preserved.** `open_feed` returns ONLY a deterministic handle —
    never feed state. Projection data still flows exclusively through the pushed
    snapshot frame.
  - **ADR-0058** (cursor-based event-log consumption — the pull pager). **Reused
    verbatim.** The viewport decision drives the existing
    `PullFeedController`/`FeedPullPager`; this ADR adds no new paging mechanism.
  - **`docs/aim.md`** §1 (app devs never touch subscription lifecycle), §2
    invariant 4 (no native business logic), §4 (Rust owns logic; the shell
    renders).

## 1. Problem — the smell

NMP exposed **app-named feed verbs** in the C ABI
(`nmp_app_chirp_open_author_feed`, `…_thread_feed`, `…_home_feed`,
`…_tag_feed`) and every shell hand-rolled pagination bookkeeping: desktop
branched on `page.has_more` before calling load-older, the TUI tracked
`timeline_has_more` and a `selected + 5` threshold, Android stored a
`TimelineWindowCursor`/`hasMore`, iOS kept `lastLoadMoreCursor`. The `{1,6}`
note-kind choice and the follow-set authors lived in the shell / app-named ABI.

This violates the north star three times: app developers were forced to touch
subscription lifecycle (§1), native shells owned business logic — page size,
threshold, cursor, exhaustion (§2 inv. 4), and the framework leaked policy that
Rust should own (§4).

## 2. Decision — descriptor open + viewport intent (Option B)

One open vocabulary; NMP owns all feed lifecycle and pagination policy.

**Descriptor collapse.** Home, author, thread, tag, and arbitrary shape become a
single open call carrying a `FeedDescriptor`:

```json
{"profile":"notes","source":{"homeFollowSet":{}},"scope":"activeAccount"}
{"profile":"notes","source":{"author":{"pubkey":"<hex>"}},"scope":"global"}
{"profile":"notes","source":{"thread":{"rootEventId":"<hex>"}},"scope":"global"}
{"profile":"notes","source":{"tag":{"name":"t","value":"nostr"}},"scope":"global"}
{"profile":"notes","source":{"interestShape":{"shape":{…}}},"scope":"global"}
```

**Auto-extend from declared viewport (Option B).** The shell reports raw
viewport facts — `{firstVisible, lastVisible, renderedLen}` — and renders. NMP
owns `prefetch_threshold`, `default_page`, `cap`, the in-flight / duplicate-drain
guard, `has_more`, `after_seq`, and the decision to drive the existing pull
pager. **No shell calls `loadOlder`, reads or stores a cursor, or branches on
`has_more` to decide behavior.** `tail_state` (`idle_more | loading | exhausted
| unavailable`) is a render-only output, NEVER a behavior input.

**Profiles own the kinds.** The `{event_kinds, renderer, page_policy}` policy
lives in a Rust `FeedProfile` registry the app composition root installs. The
`{1,6}` choice moves OUT of the shell / app-named ABI and INTO the `"notes"`
profile.

## 3. The surface (`nmp-feed::surface`)

Generic mechanism only — no protocol noun, no app name, no `InterestShape`
construction, no feed engine (D0):

- `FeedProfile { id: FeedProfileId, event_kinds, renderer: FeedRenderer, page_policy: FeedPagePolicy }`
- `FeedSource { HomeFollowSet | Author{pubkey} | Thread{root_event_id} | Tag{name,value} | InterestShape{shape} }`
- `FeedDescriptor { profile, source, scope: FeedScope (ActiveAccount|Global) }`
- `FeedHandle { key: FeedKey }` — deterministic, returned by `open`
- `FeedViewportIntent { first_visible, last_visible, rendered_len }`
- `FeedSurface` — per-host registry: installed profiles + descriptor **openers**
  + open-feed map + the viewport decision.

**Deterministic canonicalization.** `canonical_feed_key(descriptor)` hashes the
canonical descriptor JSON (object keys ordered by `serde_json`'s default map
impl) with the process-stable FNV-1a `StableHasher` — pure integer math, so the
Rust caller, the C ABI, and the wasm worker all compute the SAME key for the
same descriptor (`nmp.feed.<16-hex>`).

**Opener seam (composition-root install).** A `FeedOpener` resolves a descriptor
to an already-wired `FeedController` + its `FeedPagePolicy`. The surface only
decides *when* to call `load_older`. The home opener REUSES the very
`PullFeedController` `register_op_feed_defaults` registered under
`OP_FEED_SNAPSHOT_KEY` — one controller, no parallel registration.

**The viewport decision.** On `set_viewport(key, intent)` NMP drives one
`load_older` drain when: the last-visible row is within `prefetch_threshold` of
the tail, `rendered_len < cap`, the feed is not exhausted, and this
`rendered_len` was not already serviced (the duplicate-drain guard). A drain
that does not progress marks the feed exhausted (no further drains). Driving
calls the EXISTING controller — the seq-ordered pull pager from ADR-0058 —
unchanged.

## 4. Reconciliation with ADR-0039 (push-only) — the load-bearing invariant

`open_feed` returns a `FeedHandle` (a deterministic key) and NOTHING else. It is
an *identity*, not a *snapshot*. The grown projection still arrives only through
the pushed snapshot frame / update channel. There is no new pull accessor for
host projection consumption; ADR-0039 is intact. ADR-0058's pager is the
*event-log* read underneath "give me more" — a different layer, also intact.

## 5. Migration ladder

Rungs **1–3 are this foundation PR** (additive, non-breaking — old symbols kept):

1. **This ADR.** *(landed)*
2. `nmp-feed::surface` types, deterministic canonicalization, the profile
   registry, and the viewport-driven `FeedSurface`. *(landed)*
3. Generic C-ABI `nmp_app_open_feed` / `nmp_app_close_feed` /
   `nmp_app_set_feed_viewport` + the wasm `OpenFeed` / `CloseFeed` /
   `SetFeedViewport` worker operations, internally adapting to the existing
   `PullFeedController`. Home wired through `register_op_feed_defaults`. *(landed)*

Later PRs:

4. Migrate native shells (iOS, Android, desktop egui, TUI) and web to descriptor
   open + viewport intent. Delete `TimelineWindowCursor`, `lastLoadMoreCursor`,
   `loadOlderTimeline(after:)`, the `has_more` branches and tail thresholds.
5. Delete `nmp_app_chirp_open_*_feed` / `…_close_*_feed`,
   `nmp_app_open_contact_feed` / `nmp_app_close_contact_feed`, and the public
   `nmp_app_load_older_feed`.
6. Move `tail_state` onto the feed projection (the one schema change — regenerate
   ALL bindings with the pinned flatc) and add the proof gates (§6). Update
   generated Swift/Kotlin/TS feed types to expose tail render state only.

## 6. Proof gates (later PRs)

Gates that fail on: exported symbols matching `nmp_app_chirp_.*feed`; shell calls
to `nmp_app_load_older_feed`; shell model types named `TimelineWindowCursor` /
`nextCursor` / `lastLoadMoreCursor`; shell-side `hasMore` branching outside pure
rendering; a descriptor-key mismatch between Rust, C ABI, and wasm; viewport
events causing duplicate drains or polling; a web first-page-only regression.

## 7. Consequences

- **Positive.** One open verb; the shell owns zero pagination policy *by
  construction* (it cannot express a cursor). The `{1,6}` set and follow
  resolution leave the ABI. Web and native share one descriptor identity. No
  schema change in the foundation (`tail_state` rides step 6).
- **Negative / deferred.** This PR is additive: the app-named symbols and the
  shell bookkeeping still exist until steps 4–5. The web composition binds no
  opener yet, so web `set_viewport` is honestly inert (the key is still
  deterministic) until step 4. `tail_state` is computed but not yet on the
  projection.
