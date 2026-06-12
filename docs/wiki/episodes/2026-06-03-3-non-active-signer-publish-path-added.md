---
type: episode-card
date: 2026-06-03
session: d8869714-0ee5-4fe3-94db-1efd068b1c58
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/d8869714-0ee5-4fe3-94db-1efd068b1c58.jsonl
salience: product
status: active
subjects:
  - signer-pubkey
  - publish-unsigned-event
  - sign-with-account-nonblocking
supersedes: []
related_claims: []
source_lines:
  - 1837-1866
  - 1974-1996
  - 2218-2253
captured_at: 2026-06-11T22:59:26Z
---

# Episode: Non-active-signer publish path added

## Prior State

The only signing API was sign_active_nonblocking — every published event was signed by the active human account. Apps with agent keys (podcast feed publisher, bots, any non-user identity) had to hold raw secret key material themselves and bypass the kernel, violating D4/D7 (kernel owns signing state).

## Trigger

Podcast app was holding raw secp256k1 secret bytes and calling nostr::EventBuilder::sign_with_keys directly because there was no way to sign with a non-active signer through NMP. This was the root cause motivating the entire session.

## Decision

PublishUnsignedEvent and PublishUnsignedEventToRelays gained a signer_pubkey: Option<String> field. None uses the active account (preserving all existing behavior). Some(pubkey) resolves any registered signer — local keys synchronously, NIP-46 via the async broker — transparently. Added sign_with_account_nonblocking which looks up local keys and remote signers by pubkey, skipping the no-active-account guard when a specific signer is named.

## Consequences

- Agent keys can sign through the kernel without being the active human user
- Kernel resolves local vs NIP-46 transparently; the action module only knows a pubkey string
- All existing callers use signer_pubkey: None (the documented default), so no behavior change unless explicitly wired
- Adding the struct field broke every constructor and bind-all match arm across nmp-nip29/nip17/router/app-template/chirp — all fixed with None defaults

## Open Tail

- No consumer yet wires signer_pubkey: Some(...) — the plumbing is in place for future agent-key publishers

## Evidence

- transcript lines 1837-1866
- transcript lines 1974-1996
- transcript lines 2218-2253

