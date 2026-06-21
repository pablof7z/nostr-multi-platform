---
type: episode-card
date: 2026-05-21
session: 45fcf96e-5b37-414f-a080-820b74a4e179
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/45fcf96e-5b37-414f-a080-820b74a4e179.jsonl
salience: reversal
status: active
subjects:
  - nmpkit-ui-library
  - chirp-loc-overrun
  - substrate-thesis
supersedes: []
related_claims: []
source_lines:
  - 1-58
  - 60-149
  - 460-474
captured_at: 2026-06-18T04:53:12Z
---

# Episode: UI component library go/no-go: rejected in favor of substrate fixes

## Prior State

Assumption that shipping a cross-platform UI component library (avatars, note renderers, etc.) might be a natural next step for the NMP substrate, mirroring NDK's ~80-component Svelte registry.

## Trigger

User asked whether NMP should ship SwiftUI/Android component kits. Three research agents analyzed the NDK registry, Chirp's existing Components/ folder, and the substrate architecture. The finding: only ~50 of Chirp's 709 Marmot LOC is substrate-shaped policy — the rest is legitimate declarative layout. The 80-component Svelte registry is mostly unused stylistic variants.

## Decision

Do NOT ship a UI component library. The LOC problem is substrate-shaped, not UI-library-shaped. If NMP components take `eventId`/`pubkey` and resolve via snapshot, they compete with NDK without differentiating. If they take raw structs, they violate the snapshot doctrine. Either way, the correct move is to fix the substrate gaps first (move business logic to Rust), after which Chirp's screens collapse to thin bindings and there is far less UI code worth extracting.

## Consequences

- NMPKit/NMPCompose/nmp-ui-svelte are explicitly NOT on the roadmap
- Chirp's role is reconfirmed: surface substrate gaps, not accumulate UI components
- The shadcn-style copy-paste registry model is acknowledged as desirable long-term but gated on a second app existing (dogfood-or-die rule)
- Focus shifted to the 34 aim.md §4/§6 violations already present in Chirp's Swift files

## Open Tail

- If a second app materializes, re-evaluate extracting the 8-component shortlist (NMPAvatar, NMPUserLabel, NMPNoteContent, NMPNoteCard, NMPMediaView, NMPReactionBar, NMPComposer, NMPLoginSheet)
- The `@NMPSnapshot` property wrapper idea remains architecturally interesting but unimplemented

## Evidence

- transcript lines 1-58
- transcript lines 60-149
- transcript lines 460-474

