# ADR-0018 — ContentTree Wire Projection

**Date:** 2026-05-18
**Status:** accepted
**Doctrines invoked:** D0, D1, D6

## Context

`nmp-content` owns an internal recursive `ContentTree` / `Segment` /
`MarkdownNode` model. That model is ergonomic for Rust parsing but is not the
right host boundary: it contains recursive structures and protocol helper types
that should not be forced into platform serialization.

## Decision

Expose content across host/projection boundaries through a separate
serde-serializable wire projection, `ContentTreeWire`.

The internal tree stays Rust-native. A pure projection function flattens it into
an arena:

```rust
pub struct ContentTreeWire {
    pub nodes: Vec<WireNode>,
    pub roots: Vec<u32>,
    pub mode: RenderMode,
}
```

Parent/child relationships use explicit indices into `nodes`. Nostr references
project to flat URI/reference fields so Swift, Kotlin, TS, and desktop hosts do
not need to understand the internal Rust tree.

## D1 And D6

Projection never drops content silently and never panics on malformed or deep
input:

- excessive depth becomes a typed placeholder node,
- unformattable Nostr references become typed placeholder nodes,
- unsupported or future nodes require an explicit wire variant before they can
  ship.

## Consequences

- `ContentTreeWire` is the host-facing content payload.
- `ContentTree` remains the internal parsing/rendering substrate.
- Adding a content node kind is a cross-platform schema decision and must update
  the wire projection and tests.
