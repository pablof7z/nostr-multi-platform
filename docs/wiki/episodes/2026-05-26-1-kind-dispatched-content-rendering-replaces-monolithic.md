---
type: episode-card
date: 2026-05-26
session: 1572547f-2b2d-49fb-a383-e95ca25d0bc3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1572547f-2b2d-49fb-a383-e95ca25d0bc3.jsonl
salience: architecture
status: active
subjects:
  - embed-kind-projection
  - nostr-kind-registry
  - content-tree-wire
  - nostr-quote-card
supersedes: []
related_claims: []
source_lines:
  - 1-146
  - 648-735
  - 740-835
  - 1494-1559
captured_at: 2026-06-18T05:48:52Z
---

# Episode: Kind-dispatched content rendering replaces monolithic NostrQuoteCard

## Prior State

All three platforms rendered embedded events through monolithic widgets (iOS `NostrQuoteCard` with four display variants but no kind dispatch; Android `EmbedCard` with ad-hoc `if article != null / if list != null` branching; TUI `NostrQuoteCard` as a single `Widget`). iOS had zero kind differentiation — kind:1 and kind:30023 rendered identically. Android's typed fields on `EmbedEntry` were the closest to kind-dispatch but were incomplete and not generalized. Two incompatible wire formats existed: `ContentTreeWire` (iOS+TUI) and `ContentTreeDto`/`SegmentDto` (Android).

## Trigger

User asked whether embed cards render different event kinds differently (kind:1 vs kind:30023 article). Investigation revealed iOS had no kind dispatch at all, Android had ad-hoc branching, and NDK Svelte's `ContentRenderer.addKind()` pattern demonstrated the correct architecture — but translating it as a naked native `[kind → widget]` map would violate the V-22…V-28 thin-shell doctrine (putting app policy in Swift/Kotlin rather than Rust).

## Decision

Replace monolithic embed widgets with a Rust-owned `EmbedKindProjection` typed enum + per-platform `NostrKindRegistry` binding projection variants to native widgets. The registry is `[EmbedKindProjection variant → widget]`, NOT `[kind: u32 → widget]` — Rust decides what shape of data (which variant), native decides which widget renders it. An `UnknownProjection { kind, raw }` fallback ensures extensibility to hundreds of future kinds without Rust changes. Converge Android onto `ContentTreeWire` (eliminating `ContentTreeDto`/`SegmentDto`). Converge all three platforms onto `nmp-content::RenderContext` as the single recursion guard. Lock in via ADR-0033.

## Consequences

- Adding a new kind handler is a coordinated Rust+native PR (new `EmbedKindProjection` variant), not a Swift-only edit — enforces thin-shell doctrine per ADR-0032
- Android's `EmbedCard.kt` and the `content-quote-card/` registry components on all platforms are slated for deletion (F-CR-04), replaced by kind-registry-bound handlers
- iOS `NostrQuoteCard` closure-based `quoteCardProvider` API is deprecated for one release then removed
- Display strings (initials, colors, abbreviated npubs, relative times) are NOT pre-computed in Rust projection variants — ADR-0032 compliance enforced; native widgets compute presentation
- The `EmbedKindProjection::Unknown` variant allows app-level handlers for arbitrary kinds (classified ads, zap receipts, NIP-29 groups) without Rust crate additions — zero coupling to core for long-tail kinds
- Twelve work items (F-CR-01 through F-CR-12) form a DAG with critical path through Rust envelope → wire convergence → per-platform registries → kind handlers
- Two new crates will be created: `nmp-nip23` (kind:30023 articles) and `nmp-nip84` (kind:9802 highlights)

## Open Tail

- ADR-0033 is committed but the implementation PRs (F-CR-01 through F-CR-12) have not been started
- Whether NIP-23 (kind:30023) promotion from post-v1 to v1-A scope depends on whether F-CR-05/06/07 are feature-gated on it
- The plan file references `ContentTreeWire` as canonical but Android gallery still consumes `ContentTreeDto` until F-CR-02 lands

## Evidence

- transcript lines 1-146
- transcript lines 648-735
- transcript lines 740-835
- transcript lines 1494-1559

