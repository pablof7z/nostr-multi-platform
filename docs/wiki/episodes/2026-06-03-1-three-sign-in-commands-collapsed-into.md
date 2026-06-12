---
type: episode-card
date: 2026-06-03
session: d8869714-0ee5-4fe3-94db-1efd068b1c58
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/d8869714-0ee5-4fe3-94db-1efd068b1c58.jsonl
salience: reversal
status: active
subjects:
  - actor-command-signin
  - add-signer
  - signer-source
supersedes: []
related_claims: []
source_lines:
  - 1-1
  - 1250-1267
  - 1837-1866
  - 1974-1996
  - 2218-2262
captured_at: 2026-06-11T22:59:26Z
---

# Episode: Three sign-in commands collapsed into AddSigner

## Prior State

Three separate ActorCommand variants — SignInNsec, SignInBunker, AddRemoteSigner — each with their own handler, C ABI entry point, and iOS/Swift caller. CreateAccount was also conflated with sign-in because there was no way to add a signer without making it active.

## Trigger

Need for agent-key signing: the podcast app held raw secp256k1 secret bytes because the only signing API was sign_active_nonblocking (always the active human user). Adding a non-active signer required collapsing the three commands into one primitive with an explicit make_active flag.

## Decision

Replace SignInNsec, SignInBunker, and AddRemoteSigner with a single ActorCommand::AddSigner { source: SignerSource, make_active: bool }. SignerSource is LocalNsec | BunkerUri | RemoteHandle. make_active: false adds to roster without switching the active account or publishing kind:0/10002. CreateAccount remains the sole command that publishes kind:0/10002. No backward compatibility at the Rust command layer.

## Consequences

- All callers (Rust dispatch, C ABI, Swift/iOS) must migrate to AddSigner
- make_active must survive the async NIP-46 bunker handshake — stored on IdentityRuntime.pending_bunker_make_active rather than on the serialized BunkerHandshakeDto to avoid leaking to UI snapshots
- C ABI symbols nmp_app_signin_nsec/nmp_app_signin_bunker kept byte-stable as internal implementation details, not public API
- The nsec sign-in path deliberately preserves MLS/Marmot key registration via nmp_app_chirp_identity_sign_in_nsec internally
- SignerSource re-exported through nmp-core for downstream crate access

## Open Tail

- The TODO in KernelBridge.swift to swap the bunker wrapper body to the new add_signer C ABI once the Rust symbol lands
- signer_pubkey: Some(...) publish path is plumbed but no caller yet wires it

## Evidence

- transcript lines 1-1
- transcript lines 1250-1267
- transcript lines 1837-1866
- transcript lines 1974-1996
- transcript lines 2218-2262

