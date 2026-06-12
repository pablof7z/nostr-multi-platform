---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: product
status: active
subjects:
  - m2-open-interest
  - filter-validation
  - nmp-ffi
supersedes: []
related_claims: []
source_lines:
  - 1211-1213
captured_at: 2026-06-12T06:21:33Z
---

# Episode: M2 filter API must preserve None-vs-empty semantics and prevent tag-key prefix typos

## Prior State

M2 open_interest(filter_json) API design was proceeding without explicit awareness of two filter-parsing hazards: (1) that None vs Some(empty) is a load-bearing distinction in NIP-01 filter semantics, and (2) that tag-key prefix typos (bare 't' vs '#t') silently produce constraints that never match.

## Trigger

Coracle-rust comparison revealed these as deliberate design decisions backed by hard-won experience — coracle uses Option<BTreeSet<T>> to preserve the distinction and a TagMatch::Any/All API to generate the prefix, preventing callers from ever typing the raw prefix string.

## Decision

The M2 open_interest API must not collapse an empty JSON array into 'unconstrained' (or vice versa) when parsing app-supplied filter_json, and must either validate tag keys with loud errors kernel-side or ship a typed filter builder in the bindings. NIP-91 &-matching (TagMatch::All) must also be checked against rust-nostr support before the JSON contract freezes.

## Consequences

- Kernel-side filter parsing must distinguish absent field (match-all) from empty array (match-nothing)
- Filter builders in Swift/Kotlin bindings should prevent raw tag-key construction
- ADR-0042 M2 surface is affected before it ships

## Open Tail

- ADR-0042 read-path projection section is still Proposed, not Accepted
- NIP-91 &-matching support in rust-nostr needs verification

## Evidence

- transcript lines 1211-1213

