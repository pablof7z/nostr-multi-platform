# ADR-0026 — Signer NIP-44 encryption seam

- **Status:** Accepted / implemented
- **Date:** 2026-05-21
- **Relates to:** ADR-0015, ADR-0050

## Context

NIP-17 private direct messages and NIP-59 gift-wrap flows need NIP-44 encryption
and decryption with the user's signer. Local-key accounts can perform those
operations directly, but bunker/NIP-46 accounts cannot expose key material to the
kernel.

The actor-facing signer trait therefore needs encryption verbs, not raw key
access.

## Decision

`RemoteSignerHandle` exposes exactly two NIP-44 methods:

```rust
fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String>;
fn nip44_decrypt(&self, sender_pubkey: &str, ciphertext: &str) -> SignerOp<String>;
```

The methods use lowercase-hex pubkeys and return `SignerOp<String>` so local
signers can resolve immediately and remote signers can complete asynchronously.

The seam does not expose `nostr::Keys`, key-derivation handles, local nsecs, or
raw secret material.

## Gift-Wrap Execution

Gift-wrap construction uses signer-session/capability continuations for
sender-key operations and actor-local ephemeral keys for the outer wrap. The
outer wrap must not reuse the account key.

`nmp_nip59::gift_wrap` remains a local-keys convenience for tests and local-key
callers. Bunker-capable flows use the signer seam.

## Limits

- DM code must not read `active_local_nsec` or `mls_local_nsec`.
- Additional NIP-44 verbs need a separate decision; the current required surface
  is encrypt and decrypt.
- The inbound kind:1059 demux belongs to the protocol/app receive path, not this
  signer seam.

## Consequences

- Local and bunker signers share one actor-facing NIP-44 capability.
- NIP-17/NIP-59 flows do not require raw key escape hatches.
- Every `RemoteSignerHandle` implementation must explicitly support the two
  methods.
