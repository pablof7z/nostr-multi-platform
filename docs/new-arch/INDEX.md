# Proposed NMP Architecture

> Status: orientation packet. The durable decisions live in ADR-0069 through
> ADR-0073 under `docs/decisions/`. This directory exists only to explain the
> proposed shape at a high level while migration issues and authoritative docs are
> being updated.

## Read This First

The proposed architecture makes the app-facing model smaller without pretending
that Nostr itself is simple:

```text
install explicit features
  -> open typed read sessions
  -> render Rust-owned outputs
  -> dispatch typed actions
  -> construct unsigned event drafts
  -> finalize route/protocol envelope
  -> sign
  -> publish through Rust-owned routing/status
```

The app developer should not need to understand raw interests, projection
sidecars, dependent-source refresh loops, snapshot ticks, publish planner
internals, or relay retry machinery to build a normal screen.

NMP still owns that machinery internally because correctness depends on it:
replay-before-live, bounded output, fail-closed private routing, protocol-owned
tag mutation, signer parking, publish status, and cache/store lifecycle all
remain Rust-owned.

## Packet Map

- [01 App Model](./01-app-model.md): how an app composes reusable NMP protocol
  features and app-owned product features.
- [02 Typed Read Sessions](./02-live-queries.md): how screens subscribe to the
  state they render without owning raw event streams.
- [03 Write Flow](./03-write-flow.md): how event construction, envelope
  finalization, signing, and publishing stay composable but separate.
- [04 Internal Machinery](./04-internal-machinery.md): what NMP does under the
  hood and which old primitives should be reused, narrowed, or retired.

## Durable ADRs

- ADR-0069: explicit feature composition.
- ADR-0070: typed read sessions.
- ADR-0071: write intents and route provenance.
- ADR-0072: runtime capability and shell boundary.
- ADR-0073: ADR reset and rolling ratchets.

## Non-Goals

- This packet does not define final public API names.
- This packet is not a second tactical backlog; GitHub issues remain the queue.
- This packet does not justify keeping stale ADRs or legacy read/write paths.
- This packet does not move product policy into Swift, Kotlin, TypeScript, or
  other shells.

## Migration Direction

Use this packet as a north-star explanation only. Implementation PRs should cite
the ADRs and issues they satisfy, then delete or narrow old surface in the same
slice whenever practical.
