# ADR-0018 — ContentTree FFI wire projection (`ContentTreeWire`)

**Date:** 2026-05-18
**Status:** Accepted (T93 — codex finding #10: `ContentTree` documented FFI-stable
but not serializable)
**Doctrines invoked:** D0 (no app nouns), D1 (best-effort — never drop content),
D6 (errors never cross FFI as exceptions)

## Context

`crates/nmp-content/src/segment.rs` documents `ContentTree` / `Segment` as the
**FFI-stable boundary** every consuming UI (SwiftUI / Compose / iced / wasm)
walks. The `nmp-content` design (`docs/design/content-rendering.md` §5, row B)
explicitly rejects a parallel `RenderableContent` ViewModel and instead states
that **apps already get `ContentTree` as a payload field** — i.e. it is meant to
be exposed through `ViewModule::Payload`.

But `ViewModule::Payload` is bound `Clone + Serialize + Send`
(`crates/nmp-core/src/substrate/view.rs:41`), and:

1. `Segment` and `MarkdownNode` **deliberately do not derive `Serialize`** —
   `Segment::Mention(NostrUri)` / `EventRef(NostrUri)` transitively contain
   `nmp_core::nip21::NostrUri`, which has **no serde derives**, and `nmp-core`
   is off-limits / we do not want `nmp-content` to force serde onto a wire-format
   type in another crate.
2. `MarkdownNode` / `MarkdownInline` are a **recursively borrowed tree**
   (`BlockQuote(Vec<MarkdownNode>)`, `Emphasis(Vec<MarkdownInline>)`, …).

Net: today `ContentTree` **cannot** be a `ViewModule::Payload`. The content
gallery and every real app that the §5 design promises ("apps already get
`ContentTree` as a payload field; no new abstraction needed") cannot actually
render it across FFI. Codex finding #10 is correct — the FFI-stable claim is
currently false.

## Decision

### Option chosen: a flat, serde-serializable **wire projection** distinct from the internal recursive tree

Introduce a separate, additive type `ContentTreeWire` that **is**
serde-serializable and is what `ViewModule::Payload` exposes. The internal
`ContentTree` / `Segment` / `MarkdownNode` types stay exactly as they are —
ergonomic, recursive, serde-free. A pure projection `fn ContentTree::to_wire(&self)
-> ContentTreeWire` flattens the recursive tree into an **index arena**:

```rust
pub struct ContentTreeWire {
    pub nodes: Vec<WireNode>,   // flat arena — both block and inline kinds
    pub roots: Vec<u32>,        // top-level sequence (indices into `nodes`)
    pub mode: RenderMode,
}
```

Every parent→child relationship that is a recursive `Vec<_>` in the internal
tree (heading inlines, block-quote body, list items, paragraph runs,
emphasis/strong/link children) becomes a `Vec<u32>` of **explicit indices**
into the flat `nodes` arena. There are no recursive borrows in the wire form,
so `serde_derive` works with zero custom impls and the JSON is a flat,
language-neutral array — trivially decoded by Swift / Kotlin / TS.

`NostrUri` is projected to a flat `WireNostrUri { uri, kind, primary_id, relays,
author, event_kind }` where `uri` is the round-trippable
`nmp_core::nip21::format_nostr_uri(...)` string and `kind` / `primary_id` hand
the renderer the discriminator + pubkey/event-id hex without re-decoding.

**Serde derives live ONLY on the wire types.** The internal tree is untouched.

### Why this over the alternative

Alternative: **drop the FFI-stable claim, keep `ContentTree` kernel-internal**
and have each platform bridge hand-marshal the tree at the FFI boundary.

Rejected because:

- It contradicts the canonical §5 design (row B): the explicit promise is "apps
  already get `ContentTree` as a payload field on existing payloads; no new
  abstraction needed". Dropping the claim means every app re-implements the
  flatten-for-FFI step the substrate is supposed to own — that is exactly the
  NDKSwift "every app re-implements dispatch" anti-pattern §5 is designed to
  kill, just moved one layer down.
- It is *less* honest, not more: the doc-comment would have to admit
  `ContentTree` is kernel-internal while the whole rendering pipeline (§7 steps
  2–5) is written around apps consuming it via the payload reactor.

The wire projection keeps the design's promise honest **and** keeps the internal
tree ergonomic (no serde tax, no flattening in tokenizer/markdown). The cost is
one additive type + a pure projection function — no behaviour change, no churn
to existing call-sites, fully test-covered.

### D1 — best-effort, never drop content

A typed `WireNode::Placeholder { reason }` node is emitted, never a dropped
subtree, for:

1. **Recursion / depth collapse.** Projection caps nesting at
   `WIRE_MAX_DEPTH = 32` (projection-internal; *not* the D1 render depth budget
   of 4 — this only bounds the wire arena so an adversarial / pathologically
   deep tree projects to a *finite* form). At the cap the subtree collapses to
   a `Placeholder { reason: DepthLimit }` node — finite, typed, never panics.
2. **NostrUri that fails to format.** Structurally a `NostrUri` should always
   `format_nostr_uri` cleanly; if it ever does not, the mention/event-ref
   projects to `Placeholder { reason: UnresolvedUri }` rather than `unwrap()`.

This is distinct from `nmp_core::substrate::Placeholder<T>` (ADR-0017): that
newtype enforces non-`Option` *display fields*; this is a typed *node variant*
for "content existed here but could not be projected" — different semantics, so
a separate type is correct.

### D6 — no panics

The projection contains **no `unwrap` / `expect` / `panic` / `unreachable` /
indexing that can panic** on non-test paths. `format_nostr_uri` failure,
unparseable URLs (already `Option<Url>` in the internal tree) and depth overflow
all degrade to typed placeholder / `None` — never a panic that would cross FFI
as an exception.

## Consequences

### Positive

- `ContentTreeWire: Clone + Serialize + Send` — it can today be a
  `ViewModule::Payload`. The §5 promise is now true, not aspirational.
- Internal tree stays serde-free and ergonomic; tokenizer/markdown code
  unchanged.
- Flat arena JSON is language-neutral and stable: adding a node *kind* is the
  same load-bearing decision adding a `Segment` variant already is, but the
  *shape* (arena of tagged nodes + index lists) never changes.
- Adversarially deep / collapsed trees provably project to a **finite** wire
  form (depth-cap test).

### Negative / constraints

- The wire form is a second representation of the same data. Mitigated: it is
  *derived* (single projection function, single writer per D4), never authored
  independently, and round-trip-tested. It is not a parallel *authored* type
  like the rejected `RenderableContent`.
- A new `Segment` / `MarkdownNode` variant requires a matching `WireNode`
  variant + projection arm. This is intentional friction: a new variant is
  already a cross-platform breaking decision (§5).
- `to_wire` allocates a fresh arena per call. It is render-time, cached on the
  payload via the existing `ViewModule::Delta` reactor (§7 step 2), so this is
  off the D8 hot path.

## Alternatives rejected

### A — drop the FFI-stable claim; keep `ContentTree` kernel-internal

Rejected: contradicts the canonical §5 design and pushes the flatten-for-FFI
work into every app (the anti-pattern §5 exists to eliminate). See "Why this
over the alternative" above.

### B — add serde derives directly to `Segment` / `MarkdownNode`

Rejected: requires serde on `nmp_core::nip21::NostrUri` (off-limits crate) or a
brittle hand-written `Serialize` for the recursive borrowed tree; still ships a
recursive JSON shape that is awkward for non-Rust decoders and offers no depth
bound (an adversarial markdown nest serialises unboundedly). The wire arena is
strictly better on every axis the boundary cares about.
