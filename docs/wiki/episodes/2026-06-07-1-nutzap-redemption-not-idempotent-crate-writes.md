---
type: episode-card
date: 2026-06-07
session: b4497e9a-60f0-4e9b-b8bc-6d706ed1426c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/b4497e9a-60f0-4e9b-b8bc-6d706ed1426c.jsonl
salience: root-cause
status: active
subjects:
  - nmp-nip60
  - redeem-nutzap
  - nip61-history
  - nutzap-dedup
supersedes: []
related_claims: []
source_lines:
  - 8-18
  - 142-172
captured_at: 2026-06-11T23:07:01Z
---

# Episode: nutzap redemption not idempotent — crate writes redeemed markers but never reads them back

## Prior State

redeem_nutzap was assumed to be safe to call on any nutzap event; the crate writes kind:7376 history events with a redeemed tag marking which nutzaps were consumed, but the wallet handle has no redeemed-set field, load_from_relays never fetches kind:7376 events, and redeem_nutzap never checks whether a nutzap was already redeemed before attempting a mint swap.

## Trigger

Repeated redemption of the same nutzap event caused mint errors ("proofs already spent" code 11001). Code investigation confirmed the crate publishes the dedup marker but never consumes it — Nip60WalletHandle lacks a redeemed set, and redeem_nutzap is not idempotent.

## Decision

Redemption idempotency is a protocol-level (NIP-61) concern that belongs in nmp-nip60, not per-app. Fix: (1) add redeemed: Arc<Mutex<HashSet<EventId>>> to Nip60WalletHandle, (2) seed it in load_from_relays by fetching kind:7376 events and extracting their redeemed tags, (3) make redeem_nutzap short-circuit with AlreadyRedeemed before touching the mint. Source of truth is relay-published history, not process-local memory.

## Consequences

- redeem_nutzap becomes idempotent — no more repeated mint swaps on already-spent proofs
- Idempotency survives process restarts because the redeemed set is rehydrated from relay history on load
- A new Nip60Error::AlreadyRedeemed variant (or Ok(0) return) will be needed for consumers to distinguish first-redeem from already-done
- kind:7376 history events are promoted from write-only side-effects to load-bearing state that must be fetched at startup

## Open Tail

- Implementing the crate-level fix in nmp-nip60
- Deciding on the exact return type for already-redeemed nutzaps (error variant vs Ok(0))
- Whether wallet-poc should also keep a local seen-set as an optimization to avoid calling redeem_nutzap on known events

## Evidence

- transcript lines 8-18
- transcript lines 142-172

