---
type: episode-card
date: 2026-05-21
session: f9938ae5-cc1b-4aaa-a6cb-6212e31dacf6
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f9938ae5-cc1b-4aaa-a6cb-6212e31dacf6.jsonl
salience: architecture
status: active
subjects:
  - chirp-tui
  - testing-doctrine
  - rexpect
supersedes: []
related_claims: []
source_lines:
  - 272-516
captured_at: 2026-06-18T05:06:36Z
---

# Episode: chirp-tui e2e testing doctrine: rexpect with real relays mandatory

## Prior State

No formal testing requirement or documented approach for chirp-tui; testing was ad-hoc or absent

## Trigger

User directive to mandate e2e testing with rexpect and real relays for all new features, followed by empirical live-testing that revealed critical PTY interaction constraints

## Decision

All new chirp-tui features must be tested e2e using rexpect with real relays; documented in AGENTS.md along with hard-won constraints: PTY dimensions must be explicitly set (ratatui renders empty on 0-column terminals), status bar is the primary assertion point (synchronous, predictable strings), and local relay fixture is needed for deterministic content tests

## Consequences

- PTY rows/cols must be explicitly configured before spawning chirp-tui in tests or the app renders an empty frame
- Status bar text is the reliable assertion surface — note IDs and pubkeys are non-deterministic
- NMP runtime delivers at least one snapshot without explicit relay arg, so navigation tests don't require network
- No local relay binary exists in-repo yet; deterministic content tests block on strfry/nostr-rs-relay fixture setup
- TCL expect confirmed working for ad-hoc interaction; rexpect is the mandated crate for automated tests

## Open Tail

- Local relay fixture (strfry or nostr-rs-relay) not yet set up — deterministic content tests cannot be written until this exists
- Input handler action-key paths still require a RuntimeActions trait seam for unit-level testing

## Evidence

- transcript lines 272-516

