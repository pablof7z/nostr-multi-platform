# Crate Layers and the Layer-Inversion Rule

> **Authority:** `docs/architecture/crate-boundaries.md` §2–§10a and
> `docs/product-spec/doctrine.md`. If this reference disagrees with `crate-boundaries.md`, fix
> `crate-boundaries.md` (the single source of truth for the crate graph) and re-derive this.
> Do not create a parallel specification. GitHub Issues track unresolved violations.

## Layer Table

| Layer | Owns | Canonical crates |
|---|---|---|
| 0 | Dependency-light vocabulary and interface types | `nmp-kinds`, `nmp-signer-iface`, `nmp-nip42-types`, `nmp-nip92-types`, `nmp-nip59`, `nmp-relay-url`, `nmp-nostr-id` |
| 1 | Storage, network transport, concrete signer transport | `nmp-store`, `nmp-nostr-lmdb`, `nmp-network`, `nmp-signers` |
| 2 | Routing and subscription-planning algorithms | `nmp-router`, `nmp-planner` |
| 3 | Kernel substrate contracts and actor state | `nmp-core`, `nmp-coverage-gate` |
| 4 | Reusable Nostr protocol / product modules | `nmp-nip01`, `nmp-replies`, `nmp-nip17`, `nmp-nip18`, `nmp-nip22`, `nmp-nip25`, `nmp-nip29`, `nmp-nip42`, `nmp-nip47`, `nmp-nip51`, `nmp-nip57`, `nmp-nip60`, `nmp-nip77`, `nmp-nwc`, `nmp-marmot`, `nmp-threading`, `nmp-feed`, `nmp-wot`, `nmp-content` |
| 5 | App composition | `apps/<app>/…` Rust crates and runtime builders that explicitly compose substrate/protocol/app features |
| 6 | Platform runtimes, bindings, deliverables | `nmp-native-runtime`, `nmp-uniffi-support`, `nmp-browser-runtime`, app-owned UniFFI facades and delivery crates |
| Sidecars | Tooling, tests, diagnostics | `nmp-cli`, `nmp-codegen`, `nmp-testing`, app shells |

## Dependency Direction

Dependencies flow higher → lower. A lower-layer crate may implement a higher-layer trait only
through **explicit dependency inversion at composition time** (the trait lives in the
lower-layer crate; the concrete implementation is injected at L5/L6). Sibling crates at the
same layer do not depend on each other unless that dependency is part of their declared
responsibility.

## The Layer-Inversion Rule

**render / display / app-noun / aggregation concerns must NOT leak into L0–L4.**

This extends D0 ("the framework core knows nothing about any app domain") to *every* sub-L5
crate, not only `nmp-core` (`docs/product-spec/doctrine.md`: "D0 applies to every shared NMP
crate … protocol crates, reusable engines, binding crates, and FFI/wasm delivery surfaces
must stay app-domain agnostic").

Discriminating questions — a "yes" to any is a layer-inversion violation:

1. Does the type or function name a Nostr protocol noun by kind number, NIP name, or
   engagement category (replies/reactions/reposts/zaps)?
2. Does the type carry display-only fields (`display_name`, `picture_url`,
   `author_display_name`) that should be joined reactively at L5?
3. Does the crate's output contract bake a render-card, feed-item, or UI-affordance aggregate
   shape that belongs to the app's composition layer?
4. Does the module duplicate an L4 protocol codec that should live as a thin adapter crate?

## Layer-Specific Purity Rules

### L1 — Storage stays protocol-noun-free
`nmp-store` may expose generic event-reference mechanics (counts of events carrying an `e` tag
to a target, optionally bucketed by caller-supplied opaque keys). It must NOT hard-code kind
numbers, NIP-10 marker semantics, or named engagement aggregates (`reply`/`reaction`/
`repost`/`zap`).

### L3 — Kernel substrate stays generic
`nmp-core` owns substrate contracts and actor-owned state: actor loop, session state,
capability sockets, trait seams, snapshot/update envelopes, and the `nmp-core::display`
helper module (pure render-side utilities for TUI/CLI — NOT for projection builders or FFI
serialization). It must not grow protocol-specific parsers, routing algorithms, action bodies,
app-specific nouns, or typed NIP-NN codecs. A typed NIP-NN entity surface belongs in an L4
protocol crate as a thin `rust-nostr` adapter.

### L4 — Protocol crates carry no render enrichment or kind-named transport actions
Three distinct shapes:

- **A. Display enrichment in projections and `.fbs` wire tables.** L0–L4 projections and wire
  payloads carry raw protocol identifiers only: `author_pubkey` (hex), kind numbers, tag
  arrays, content. `ProfileProjection` is the *sole* legitimate carrier of `display_name` /
  `picture_url`. The L5 composition layer joins `author_pubkey → ProfileProjection`
  reactively.
- **B. Render-card / feed-item aggregates.** A NIP crate must not own a render-card,
  feed-item, or UI-affordance aggregate type. Protocol output may include typed note/event
  projections with raw protocol fields; the render-card owner is L5 or the leaf app.
- **C. Kind-named actions in kind-blind transport.** `nmp-nip29` is h-tag/previous/host-pin
  envelope routing only; its sole write surface is `nmp.nip29.publish_group_event`. Per-kind
  named actions (`react_in_group`, `repost_in_group`) belong to the owning NIP crate (NIP-25
  builds the reaction, NIP-18 the repost), which hands the event to the NIP-29 envelope. See
  `protocol-crates-and-kind-blind-transport.md`.

## Cautionary Violation Families (2026-06-30 audit)

Fifteen confirmed violations across five crates; issues are the canonical tracker.

| Family | Crate (layer) | Live evidence | Issue |
|---|---|---|---|
| Store engagement aggregation | `nmp-store` (L1) | Former store-level social counter API with hard-coded reply/reaction/repost/zap classification | #2512 |
| NIP-29 kind-named actions | `nmp-nip29` (L4) | `REACTION_KIND`/`REPOST_KIND` literals + `ReactInGroupAction`/`RepostInGroupAction`/`ShareEventInGroupAction` | #2513 |
| Content display in wire | `nmp-content` (L4) | `author_display_name`/`author_picture_url` in `embed_projection/variants.rs` + `schema/*.fbs` | #2514 |
| Kernel NIP-19 codec | `nmp-core` (L3) | `pub mod nip19` in `lib.rs`; `Nip19Entity` encode/decode | #2515 |
| NIP-01 render-card | `nmp-nip01` (L4) | `TimelineEventCard` "render-ready event card" + `schema/timeline_snapshot.fbs` | #2510 |

The `#2510` family had a root cause in `crate-boundaries.md` §8, which previously sanctioned a
"note timeline/OP-feed surface" in `nmp-nip01` — a loophole pending a §8 amendment. That
amendment landed via #3082/#3086: `crate-boundaries.md` §8 now names `nmp-feed` as the owner of
the generic feed engine/row/wire (including the composite multi-lane surface below) and
`nmp-note-feed` as a thin protocol-composition adapter with no engine, row, or wire of its own.
#2508 settled the relation/read side: global social summaries are rejected; reads belong to the
concept crate that defines them.

## Composite-feed doctrine (#3082/#3086)

A composite feed is an additive set of **lanes** over one `FlatFeed`-style engine
(`crates/nmp-feed/src/composite.rs`), not a second engine. Two rules extend the layer table
above:

1. **Mappings/extractors cross FFI as opaque registered ids, never native closures.**
   `LaneMappingId(String)` names a `LaneMapping` closure
   (`Arc<dyn Fn(&KernelEvent) -> Vec<MappedRow> + Send + Sync>`) built in Rust **at the
   composition root** (ADR-0069) and resolved through `LaneMappingRegistry`
   (`crates/nmp-feed/src/lane_mapping.rs`). This is the same discipline as `CustomAdmissionId` —
   a "yes" to "does this cross FFI as a closure instead of an id" is a D7 violation.
2. **The engine never learns a kind; protocol crates own extraction, render policy is
   app-owned.** `nmp-feed` registers only its own kind-blind identity mapping
   (`feed.authored`). `nmp-nip18` registers `nip18.target`; `nmp-nip22` registers `nip22.root`.
   A lane's row carries a delivery-tagged `TypedRef` (`RenderOnly` stays in the lazy
   render/embed channel; `Delivered` folds the target into the SAME session's own acquisition).
   Whether an app renders the activity or the target, dedupes, or applies a weighted sort is
   app/composition-root policy — never baked into `nmp-feed`. The former `RootIndexedFeed`
   engine and `FeedShape::RootIndexed` reply-rollup shape are deleted, not generalized:
   `FeedShape` has exactly one variant, `Flat`.

See [`docs/perf/composite-feed-architecture.md`](../../../docs/perf/composite-feed-architecture.md)
for the full design record and driving example (a 30023-article composite feed via authored +
commented + reposted lanes).

## Known-Legitimate Patterns (do NOT flag)

- **`nmp-core::display`** (V-33): pure display-string helpers (bech32, npub abbreviation,
  avatar tint, relative-time buckets) for Rust presentation surfaces (TUI, CLI, tests).
  Legitimate, but must not be called from projection builders, snapshot structs, or FFI
  serialization paths.
- **`ProfileShape::Card`**: a substrate resolution-width enum, not a UI card.
- **Kind numbers in `nmp-kinds` (L0)**: the vocabulary layer owns kind constants. The
  prohibition is on kind-number-keyed *policy* in L1–L4.

## Change Policy

When a crate-boundary rule changes, update `docs/architecture/crate-boundaries.md` first. Do
not create a parallel plan or architecture ladder.
