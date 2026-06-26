# ADR-0017 — Missing display facts remain raw absent facts

- **Status:** Accepted / reconciled with ADR-0032
- **Date:** 2026-05-18
- **Relates to:** ADR-0032

## Context

D1 requires hosts to be able to render immediately and refine in place. That does
not require Rust projections to invent fake protocol values for facts the network
has not delivered.

Profile pictures, display names, and similar profile facts are raw Nostr data.
When they are absent, the projection should represent absence; the native view
can render a placeholder presentation from the pubkey.

## Decision

Rust-owned projections keep missing protocol facts as optional raw fields.

Examples:

- `TimelineItem.author_picture_url` is optional;
- `ProfileCard.picture_url` is optional;
- missing pictures are not encoded as `identicon:` strings in Rust projection
  data.

Native/app rendering layers own placeholder visuals such as generated avatars,
initials, skeleton rows, or image fallbacks.

## Consequences

- Raw-data projection doctrine stays intact.
- Hosts can still render immediately because presentation fallback is host
  behavior, not fake protocol data.
- Rust projections do not gain a second writer for profile display facts.
