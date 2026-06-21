---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - p2-per-nip-branches
  - classify-kind
  - nip19-codec
supersedes:
  - 2026-06-18-8-remove-per-nip-specialization-from-generic
related_claims: []
source_lines:
  - 26-27
  - 876-904
  - 1035-1041
captured_at: 2026-06-18T21:31:23Z
---

# Episode: Per-NIP branches eliminated from generic router and kernel

## Prior State

Router had per-NIP classify_kind dispatch tables (NIP-54/NIP-37); nmp-core contained a parallel hand-rolled bech32/TLV codec for nip19 instead of using rust-nostr's implementation.

## Trigger

#1493 P2 finding: per-NIP/per-kind branches in generic layers violate D0 (protocol nouns belong in their own crates, not in the kernel or router).

## Decision

classify_kind table deleted from router (+ mirrored test removal). nip19 rewritten as a thin adapter over nostr::nips::nip19 (retires the parallel hand-rolled bech32/TLV codec). Public API preserved; consumers source-compatible. DmInboxLookup on ProtocolCommandContextParts assessed and left as-is (D15 Noop capability pattern, not a D0 violation).

## Consequences

- Two latent bugs caught by codex review: u32 kind > u16::MAX silently truncating (now typed Err), >255-byte relay/identifier overflowing single-byte TLV length (now typed Err).
- Wire golden fixtures needed regeneration (nip19 naddr/nevent URIs re-encode to canonical rust-nostr bech32 with different TLV byte layout — identical coordinates, different field ordering + RelayUrl normalization).
- nip21 integration tests split into tests/nip21.rs to stay under file-size hard-cap.

## Open Tail

*(none)*

## Evidence

- transcript lines 26-27
- transcript lines 876-904
- transcript lines 1035-1041

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-4-per-nip-branches-eliminated-from-generic.json`](transcripts/2026-06-18-4-per-nip-branches-eliminated-from-generic.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-4-per-nip-branches-eliminated-from-generic.json`](transcripts/raw/2026-06-18-4-per-nip-branches-eliminated-from-generic.json)
