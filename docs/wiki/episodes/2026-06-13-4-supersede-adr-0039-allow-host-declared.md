---
type: episode-card
date: 2026-06-13
session: 78c8ec3a-f558-4738-98af-1f3af4978ec4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/78c8ec3a-f558-4738-98af-1f3af4978ec4.jsonl
salience: reversal
status: active
subjects:
  - adr-0039
  - projection-subscription
  - push-interest
  - host-kernel-contract
supersedes:
  - 2026-06-13-5-host-declared-projection-consumption-supersedes-adr
related_claims: []
source_lines:
  - 5346-5351
  - 5403-5425
  - 5435-5447
  - 5516-5525
captured_at: 2026-06-13T21:09:24Z
---

# Episode: Supersede ADR-0039: Allow host-declared projection interest (rejecting the blanket prohibition)

## Prior State

ADR-0039 explicitly rejected letting the host declare which projections it wants, arguing: "An 'active group' concept would require a round-trip to set kernel state from the host — a one-way-data-flow violation." This meant every registered projection rides every snapshot, regardless of whether any host view consumes it. relay_diagnostics (a debug screen rarely opened) was serialized, decoded, and (until the verifier PR) verified 4×/sec on every device forever.

## Trigger

Architectural analysis revealed ADR-0039 committed a category error: it conflated "the kernel knows which view is open" (a genuine smell — dynamic per-view subscription that leaks UI state) with "the host declares which projections it consumes" (a static interest/capability set, declared once at registration — no round-trip, no view-state leak). The kernel already accepts host-declared interests for relays (push_interest), profile claims, event claims, and dynamic feed keys. A declared projection set is the output-side sibling of push_interest — resource ownership, not business logic. Codex second opinion confirmed: "a declared projection set is the output-side sibling of push_interest — resource ownership, not business logic."

## Decision

ADR-0039 is superseded. The host will declare its consumed-projection set once at init (static, the consumer analogue of push_interest). The kernel only serializes keys the host has declared. relay_diagnostics and similar rarely-viewed projections stop shipping unless explicitly declared. The one-way-data-flow invariant is preserved — the declaration is one-shot at registration, no per-view round-trips. An Opus architect agent was launched to produce the redesign ADR and implementation, explicitly scoping out the genuine view-state-leak case (dynamic per-view subscription) as future work.

## Consequences

- Projections not declared by the host are never serialized, decoded, or diffed — permanent per-projection savings for every unused projection
- relay_diagnostics, metrics, and other debug projections ship only when a debug/developer host declares interest
- The host-kernel contract becomes symmetric: the host declares input interests (push_interest) and output interests (consumed projections), both one-way declarations with no feedback loop
- ADR-0039's reasoning about "active group" round-trips is preserved as a future scope boundary — dynamic per-view subscriptions remain excluded
- Composes with ADR-0053 Rung 5 (incremental emission + host-declared interest)

## Open Tail

- Opus architect agent still producing the superseding ADR
- Implementation awaits owner sign-off on the ADR before any structural merge
- Dynamic per-view subscription remains out of scope (correctly, per the original ADR-0039 concern)

## Evidence

- transcript lines 5346-5351
- transcript lines 5403-5425
- transcript lines 5435-5447
- transcript lines 5516-5525

