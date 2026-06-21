---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: architecture
status: superseded
subjects:
  - p2-per-nip-branches
  - classify-kind
  - nip19-codec
  - nmp-router
  - nmp-core
supersedes:
  - 2026-06-18-6-per-nip-per-kind-branches-removed
related_claims: []
source_lines:
  - 25-27
  - 876-893
captured_at: 2026-06-18T20:25:04Z
---

# Episode: Remove per-NIP specialization from generic layers

## Prior State

The generic router had a per-NIP classify_kind dispatch table (NIP-54/NIP-37). nmp-content had bare kind literals in sniff_mode_from_kind and embed-projection kind dispatch. nip19.rs was a parallel hand-rolled bech32/TLV codec duplicating rust-nostr's implementation.

## Trigger

Issue #1493 P2: per-NIP/per-kind branches in generic layers violate D0 (no protocol-specific branching in generic code).

## Decision

classify_kind table removed from the generic router. Bare kind literals in nmp-content named via constants. nip19.rs rewritten as a thin adapter over nostr::nips::nip19 (retires the parallel codec). Public API preserved; consumers source-compatible. Two latent edge-case bugs fixed: u32 kind > u16::MAX now typed Err (was silent truncation), >255-byte relay/identifier overflowing single-byte TLV length now typed Err.

## Consequences

- Router is now NIP-agnostic — no per-NIP dispatch table
- DmInboxLookup on ProtocolCommandContextParts assessed and left as-is (Noop D15 capability pattern, not a D0 violation)
- SELF_KINDS_TAILING kept — already has bootstrap_self_kinds_override FFI slot, kind:10050 is a deliberate OneShot
- Repost "triple-path" verified as test-only; nip21/tags.rs verified as compliant kind-agnostic codecs; longform/embed_registry already use named consts

## Open Tail

*(none)*

## Evidence

- transcript lines 25-27
- transcript lines 876-893

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-8-remove-per-nip-specialization-from-generic.json`](transcripts/2026-06-18-8-remove-per-nip-specialization-from-generic.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-8-remove-per-nip-specialization-from-generic.json`](transcripts/raw/2026-06-18-8-remove-per-nip-specialization-from-generic.json)
