---
type: episode-card
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
salience: root-cause
status: superseded
subjects:
  - chirp-ios-performance
  - subscription-compiler
  - snapshot-pipeline
  - nostr-avatar
supersedes: []
related_claims: []
source_lines:
  - 441-623
captured_at: 2026-06-15T07:37:27Z
---

# Episode: Chirp iOS performance root-cause: unconditional recompile–snapshot–SwiftUI cascade

## Prior State

Chirp iOS performance issues were observed but the root cause was unverified; ADR-0055 Rung 3 'omit-unchanged' was planned but not empirically confirmed as the primary bottleneck, and the specific cascade path from Rust actor tick to SwiftUI redraw was not profiled.

## Trigger

Time Profiler trace captured on physical device (iPhone 17 Pro Max, iOS 26.6, 1062 samples / 1.82s) while the user exercised the app.

## Decision

The dominant performance bottleneck is the cascade of: (a) SubscriptionCompiler recompiling all subscriptions from scratch on every actor drain tick with no short-circuit for unchanged input (205ms, 19.3% CPU, dominated by BTreeSet<String> clone/traverse in InterestShape), (b) make_update running full snapshot + FlatBuffer serialization after every dispatch_command including trivial UI interactions (99ms + 57ms FFI), and (c) every snapshot emission invalidating SwiftUI state causing full AttributeGraph diff + NostrAvatar.body re-evaluation without Equatable guard (~373ms). Secondary: MarmotProjection::messages_since queries SQLite on every snapshot cycle even with no new MLS events; FlatBuffer decode copies ByteBuffer.Storage.Blob per subscript (not zero-copy).

## Consequences

- ADR-0055 Rung 3 'omit-unchanged' empirically validated as highest-impact fix — skipping snapshot emission when projections haven't changed would eliminate the entire SwiftUI cascade for idle ticks
- Memoization of subscription compile results identified as #2 priority — cache InterestShape lattice per-subscription, only recompile on actual input change
- MarmotProjection::messages_since needs a dirty-flag/watermark to avoid redundant SQLite queries on unchanged snapshot cycles
- InterestShape BTreeSet<String> (64-char hex pubkeys) identified as expensive clone/drop path — replacement with sorted Vec<[u8;32]> would halve clone cost in lattice::merge
- NostrAvatar lacks Equatable conformance on (pubkey, url, colorHex) — SwiftUI cannot skip body re-evaluation when identical profile data is delivered

## Open Tail

- None of the identified fixes have been implemented yet
- NostrAvatar Equatable guard proposed as a quick independent win — awaiting user go-ahead
- FlatBuffer zero-copy decode not yet addressed

## Evidence

- transcript lines 441-623
