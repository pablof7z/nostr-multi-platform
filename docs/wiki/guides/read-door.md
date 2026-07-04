---
title: "The Read Door: Typed Read Sessions and API Surface"
slug: read-door
topic: read-door
summary: The read door follows the typed sessions architecture established in ADR-0070
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
  - session:fb992e80-b32b-4673-b2c2-40e8044504ee
---

# The Read Door: Typed Read Sessions and API Surface

## Read Door (Typed Sessions)

The read door follows the typed sessions architecture established in ADR-0070. It requires `open_interest`, `ObservedProjection`, and `ReducedSource` to be crate-private, with raw feed/interest C ABI symbols deleted. This makes typed read sessions the sole app-facing read API. The app-visible read model is one typed session descriptor and handle owning the complete lifecycle: acquisition → route policy → bounded replay → live sink → admission → typed output → wake sources → teardown. `open_interest` is demoted to acquisition-only substrate; `ObservedProjection` and `ReducedSource` are private machinery, not app vocabulary. Empty dynamic source sets in read sessions fail closed and never silently become wildcard relay demand. Shells must not hand-author filters or projections.

The read door is not yet 100% complete. The product-read cutover (#2399) and the search read doors (#2418/#2427) are still open.

<!-- citations: [^898a4-84131] [^898a4-83357] [^3c942-35224] -->

## Read Door (Concept-Owned Active Reads)

The read door is the concept-owned active-read architecture: one internal lifecycle engine, many explicit concept owners, and rendered typed output. Apps never assemble interests, observers, projections, source reducers, generic sessions, or Trellis resources. The public read vocabulary is limited to `open_<concept>(spec/target) -> Handle`, `close(handle)`, `load_more(handle)` (only where paging is intrinsic), and typed output. A generic `open_session(namespace, bytes)` API is rejected. NMP has one read abstraction but many concept APIs: one lifecycle model plus many explicit owners, not one generic read API with many opaque namespaces.

A concept-owned active read is a kept-live query with a close handle that opens demand, replays cache/store, pushes typed output while mounted, then closes exact demand. The term `active read` is kept as an architecture noun, renamed to `concept-owned active read`; `typed session` is deleted from public docs in favor of `active read` / `open_<concept>`; `feed session` is deleted as a public noun in favor of `feed read` / `open_feed`. `observed delivery`, `projection`, `source reducer`, and `Trellis` are private-only vocabulary, not mentioned in the NMP app API. Shells must not hand-author filters or projections.

The internal read model is a single read lifecycle engine with three stages: Demand (what events must be alive), Admission + model (which facts matter and how state updates), and Output (what typed data is emitted to the host). The `ConceptRead` trait is an internal pattern, not a universal public trait; it is not exposed publicly unless there is a concrete reason.

Deleted from public/app-facing API: `open_observed_feed_source` (deleted, not deprecated; all callers updated in one PR), `open_interest` (if reachable from product/app surfaces), raw observer/projection lifecycle APIs, generic session namespace doors, relation bucket APIs, and app-visible `ReducedSource` / `ObservedProjection` nouns. If a consumer needs app-owned row projection it expresses that as a named concept-owned read or app-owned recipe, not a public raw `ObservedProjectionSink` doorway. Internal names may survive only behind private modules and explicit doctrine-lint ratchets; no public docs, generated helpers, templates, examples, or pub API items may expose `session`/`ObservedProjection`/`ReducedSource`/`open_interest`/`Trellis` vocabulary unless the file is explicitly internal-runtime documentation. The public-surface retirement ratchet is extended to cover nmp-content, banning `open_pointer_source` and `register_pointer_source` from reappearing in `nmp-content/src/lib.rs` and `nmp-content/src/pointer_source/mod.rs`. Every surviving app-visible read door is classified as typed helper, internal/private executor, transitional with owner issue, or stale/violating; that audit discipline is a permanent ratchet.

<!-- citations: [^dcc80-7a8b4] [^fb992-d685d] -->
## Vocabulary Rename: Session → Handle

The `FeedSessions` public Rust type is renamed to `Feeds` (not `FeedReads`); the doctrine noun `feed read` stays in docs but type names use concept nouns plus generic lifecycle nouns. UniFFI `FeedSessionHandle` is renamed to `FeedHandle`, `close_feed_session` to `close_feed`, and `session_id` to `handle_id`; the same treatment applies to the wasm worker protocol and regenerated web helpers. The `nmp-uniffi-support` helper family `open_feed_session`/`load_older_feed_session(_status)`/`reopen_feed_session`/`FeedSessionError` is renamed to `open_feed`/`load_older_feed(_status)`/`reopen_feed`/`FeedError`. The `BrowserFeedSessions` browser-runtime sibling facade is renamed to `BrowserFeeds`. The `nmp_feed::FeedHandle.session_id` Rust field keeps its internal identifier but carries `#[serde(rename = "handle_id")]` so the browser JSON wire speaks the new vocabulary while the internal Rust name stays unchanged; the Rust field identifier and the browser JSON wire field name diverge on that one struct.

Internal crates keep `session` vocabulary freely (`nmp-feed-session`, `session_engine`, `FeedSessionRegistry`); #2508 ratifies `session` as runtime bookkeeping vocabulary that is private, not public. A doctrine-lint gate bans `Session` in pub items of facade crates (`nmp-native-runtime` public API, `nmp-uniffi`, `nmp-browser-runtime` exports, generated-helper templates), with internal crates allowlisted. The rename lands as one hard-break PR sweeping all surfaces and docs together, before the #2690 feed-surface freeze and before the next consumer re-pin; no aliases. <!-- [^dcc80-d022e] -->

## Feed Read API Shape and Ownership Boundary

The feed's public API shape is `app.feeds().open(FeedKey, feed::events()...primary_kinds().from().shape().order().window().project())` returning a handle. Feed owns primary item acquisition, source/perspective resolution, repost wrapper inclusion, windowing/order, and feed row output. Feed does NOT own profiles, reply counts, reactions, zaps, thread hydration, referenced-event previews, media loading, or app-specific social bars. When a feed row needs reply count, the consumer mounts `open_replies`; when it needs avatar, the consumer mounts/opens profile output; when it needs thread graph, the consumer mounts threading. The feed row exposes stable references, not hydrated screen content. <!-- [^dcc80-ff1a8] -->

## Shared Lifecycle Engine

NMP has exactly one internal lifecycle engine for all concept reads; no exempt lanes are permitted. If the engine cannot express a shipped read, the engine is wrong, not the read. There are already more than one read lifecycle lane in the codebase today: the feed session engine, NIP-50 search lifecycle, NIP-29/group-feed lifecycle, and the deprecated observed feed-source escape hatch. These are to be consolidated onto the single engine.

The shared internal lifecycle engine owns: handle allocation, open/replace/close registry, replay-before-live ordering, live activation, exact-demand withdrawal, reverse teardown ordering, output clear/tombstone, account/source switch behavior, coalesced output emission, and leak instrumentation. The engine owns the common lifecycle spine, not everything; feed-specific capabilities like windowing, paging, and root-admission scope compilation, and search-specific capabilities like relay pinning, stay concept-specific as optional capabilities on top of the spine, not slots in a god-trait.

A concept owner supplies only: target/spec type, event demand compiler, admission predicate, event reducer, typed output encoder, concept-specific validation, and concept-specific tests. Concept owners must not directly implement lifecycle: replay, live activation, registry replacement, exact close, reverse teardown, or tombstone emission.

The extraction sequence for the lifecycle engine is: extract the concept-neutral spine from feed/group machinery into an internal module, keep feed as the first heavy client, implement one #2758 read as the boundary proof, implement the other three as near-declarative specs, add a ratchet/test banning direct `open_interest`/`open_observed_projection`/`close_interest` calls outside the shared engine, then migrate search/group-feed onto the engine. <!-- [^dcc80-57da7] -->


`InterestLifecycle::OneShot` is the existing kernel enum variant that models a subscription which sends CLOSE on EOSE. It already flows end-to-end through planner, compiled plan, wire frames, and the EOSE handler, but was previously not exposed through the ReadDemand delivery seam. <!-- [^fb992-c7d0a] -->
## nmp-read-session Crate

The `nmp-read-session` crate is a new Layer-4 crate owning the single implementation of read-lifecycle mechanics: `ReadSessionRegistry` (handle alloc + open/close + reverse teardown + one leak audit), `open_read`/`close_read` (replay-before-live, exact per-demand withdrawal, reverse teardown, typed-output tombstone), and the `ReadHost` seam. Concept doorways live in concept crates, not in `nmp-native-runtime`; `open_replies` lives in its concept crate consuming a `ReadHost` seam, and no `open_<concept>` is defined as an `NmpApp` method. A concept crate's `open_<concept>` symbol does not exist in an app that does not import that crate; omitting a concept crate means its symbols, demand machinery, and binary weight are absent. The dependency direction is concept-crate → engine ← runtime; concept crates never depend on the runtime, and the runtime never accumulates per-concept dependencies. Runtimes implement the `ReadHost` seam once generically; no per-concept method is added to the runtime. A doctrine-lint ratchet bans `nmp-nip*` dependencies in runtime crates and bans `open_<concept>` definitions outside their concept crate. <!-- [^dcc80-8b1fe] -->

## Concept Reads (#2758)

The four #2758 concept reads (`open_replies`, `open_reactions`, `open_reposts`, `open_zaps`) are acceptance tests for the concept-read engine boundary: if any contains lifecycle code, the engine is incomplete and that is the bug to fix, not the read. The concept reads are not yet exposed through any FFI/wasm binding surface; they are reachable from Rust but not from Swift or TS, and per the app-owned-facade decision (#2763) each consumer composes the concept crates and exposes `openReplies`/etc. through its own facade. #2758's baked-in acceptance criterion is that Chirp removes its fail-closed guard (chirp#15) after the four concept reads land; the collapse is not proven until a client renders actual reply/reaction/zap counts against a live relay.

Concept reads are identity-free: `open_reactions` and `open_reposts` expose raw `reactor_pubkeys`/`reposter_pubkeys` with no viewer parameter; the shell derives `"did I react/repost"` by comparing its own active-account pubkey against the raw lists.

`nmp-reactions` reuses `nmp_nip25::ReactionAggregateProjection` unmodified for admission, per-token aggregation, and NIP-09 retraction folding; it adds zero new semantic logic.

`open_reposts` admission accepts kind:6 reposts always and kind:16 generic reposts only when their `k` tag proves target kind `1` (NIP-18 discrimination). Its reducer is keyed by repost-wrapper event id → reposter pubkey; snapshot dedups to distinct pubkeys (`count` + `reposter_pubkeys`, raw only). A same-author double repost survives partial retraction correctly. Deletion handles kind:5 from the reposting pubkey retracting their wrapper via `nmp_nip09::DeleteRecord`, mirroring `nmp_nip18::RepostActivityProjection::apply_delete`.

`open_zaps` admission goes entirely through `nmp_nip57::try_from_kernel_event_validated` (amount-consistency + provider-mismatch validation); no crypto/bolt11/receipt-validation is reimplemented in the concept crate. Its reducer dedups by receipt event id and aggregates raw per-sender totals (`zappers: pubkey -> msats/count`, one bucket for anonymous receipts). `nmp-zaps` is classified as a private package (not public) in `release/nmp-release.toml` because #2318 settled NIP-57/zaps as post-v1 and excluded `nmp-nip57` from the public release train.

A static `ReadSpec` (fixed demand set at `open_read` time) cannot route kind:5 retractions of events it has not yet seen, because deletions name the deleted event's own id; this is a systemic engine gap tracked as #2818 and affects all four concept crates. The fix is an engine-owned dependent-demand capability (prior art: the kernel's dependent-interest owner that the feed engine already uses), not a concept-side re-subscription loop. <!-- [^dcc80-cbd6d] -->
