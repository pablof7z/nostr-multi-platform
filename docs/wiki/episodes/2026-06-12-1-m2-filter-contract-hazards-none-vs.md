---
type: episode-card
date: 2026-06-12
session: 954c56b2-d292-4021-8b55-977d3fd8df4d
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/954c56b2-d292-4021-8b55-977d3fd8df4d.jsonl
salience: product
status: active
subjects:
  - m2-open-interest
  - filter-json-contract
  - kernel-filter-parsing
supersedes: []
related_claims: []
source_lines:
  - 1211-1218
captured_at: 2026-06-12T06:08:15Z
---

# Episode: M2 filter-contract hazards: None-vs-empty and tag-key typos

## Prior State

M2 open_interest(filter_json) migration planned without awareness of two JSON-parsing traps: collapsing an empty array into 'unconstrained' vs 'matches nothing', and silent failure from bare tag keys without '#' prefix

## Trigger

Coracle-rust's 911-line filters chapter identifies that Option<BTreeSet<T>> preserves the None/Some(empty) distinction (load-bearing per NIP-01) and that TagMatch::Any/All API exists to prevent prefix typos like 't' vs '#t' that silently never match

## Decision

The kernel must either validate filter JSON kernel-side with loud errors on malformed tag keys, or NMP must ship a typed filter builder in the bindings so callers never hand-author '#' prefixes; also must not collapse empty arrays into 'unconstrained'

## Consequences

- The open_interest API contract needs explicit None-vs-empty semantics before the M2 surface freezes
- NIP-91 '&t' AND-matching must be checked against rust-nostr support before the filter JSON contract freezes
- Filter parsing becomes a validation surface that the kernel must defend

## Open Tail

- ADR for nmp_app_open_interest needs to specify None-vs-empty contract
- Decide whether to validate in kernel or force typed builder in bindings

## Evidence

- transcript lines 1211-1218

