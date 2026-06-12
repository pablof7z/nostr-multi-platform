---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: root-cause
status: active
subjects:
  - rust-nostr-dependency
  - secret-key-safety
  - memory-budget
  - nip44-performance
supersedes: []
related_claims: []
source_lines:
  - 1215-1219
captured_at: 2026-06-12T06:08:15Z
---

# Episode: rust-nostr dependency footguns: SecretKey Debug, TagStandard overhead, ConversationKey reuse

## Prior State

NMP depends on rust-nostr as its crypto foundation (doctrine: thin adapters, never scratch crypto) without auditing specific footguns in its API surface that could leak secrets, waste memory, or re-derive expensive operations

## Trigger

Coracle-rust research identifies three specific rust-nostr issues: (1) SecretKey derives Debug directly — only redacted via composite Keys type, (2) every tag carries OnceCell<Option<TagStandard>> — per-tag memory overhead for an enum NMP mostly doesn't use, (3) ConversationKey reuse — ECDH+HKDF-Extract is expensive and welshman LRU-caches 10k of these

## Decision

Three targeted audits needed: (1) verify nmp-core never Debug/Display-formats SecretKey into logs/traces/snapshots, especially in signer-transparency paths, (2) quantify TagStandard OnceCell overhead against kernel in-memory tier budgets (#1088), (3) check whether NIP-17 DM/giftwrap paths re-derive ConversationKey per message or reuse

## Consequences

- Secret material may be leaking through Debug traces in the sign-account port — needs immediate verification
- TagStandard overhead may be measurable in RAM-eviction tuning for bounded kernel tiers
- DM performance may be unnecessarily slow if ConversationKey is re-derived per message

## Open Tail

- Secret-key Debug audit across nmp-core
- TagStandard memory quantification
- ConversationKey reuse check in nip17 paths

## Evidence

- transcript lines 1215-1219

