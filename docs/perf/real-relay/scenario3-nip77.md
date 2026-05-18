---
scenario: 3-nip77-negentropy-req-fallback
verdict: PASS
generated_at: 1779089139
relays: ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.nostr.band", "wss://relay.primal.net"]
---

# Scenario 3 — NIP-77 negentropy + graceful REQ fallback

## Verdict: PASS

Two legs prove the D2 contract end-to-end against live relays: negentropy genuinely saves wire vs REQ (Leg A), and a relay that cannot speak negentropy is classified so the planner falls back to plain REQ (Leg B).

## Leg A — negentropy works (relay.damus.io / strfry)

**PASS** — `wss://relay.damus.io` (strfry) reconciled to Done with need=200 (have=0) from an empty local set. Negentropy moved 6411 B of protocol payload vs a REQ floor of 51200 B (200 ids x 256 B) -> 44789 B saved (87.5%). Live D2 proof.

## Leg B — REQ-fallback signal (non-NIP-77 relay)

**PASS** — `wss://nos.lol` does NOT speak NIP-77 (classified via NOTICE: ["NOTICE","ERROR: bad msg: negentropy disabled"]). Plain-REQ fallback to the same relay returned a live EVENT within 8s -> graceful fallback proven.

## Overall

**PASS** — overall PASS requires BOTH legs to genuinely pass; any SKIP keeps the scenario SKIP. No leg fakes green.
