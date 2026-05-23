# ADR-0026: Signer NIP-44 seal seam — `RemoteSignerHandle` gains `nip44_encrypt` / `nip44_decrypt`

- Status: Implemented
- Date: 2026-05-21
- Supersedes / amends: none
- Related: ADR-0015 (M6 signer design), ADR-0025 (Marmot bespoke-FFI cluster),
  Phase 1 of the NIP-17 DM stack (`feat(nip17): signer-access seam for
  gift-wrapped DMs`, PR #122)

## Context — the gap this closes

NIP-17 private direct messages are delivered as NIP-59 gift-wraps. A gift-wrap
is two nested envelopes:

1. **Seal (kind:13)** — the inner rumor, NIP-44-encrypted *to the recipient* and
   signed *by the sender's account key*.
2. **Gift-wrap outer (kind:1059)** — the seal, NIP-44-encrypted *to the
   recipient* and signed *by a fresh ephemeral key*.

Step 1 needs the sender's key for two operations: a NIP-44 encrypt and an event
signature. The actor reaches signers exclusively through the
`RemoteSignerHandle` trait (`crates/nmp-core/src/remote_signer.rs`) — the
doctrine-**D0** boundary that keeps `nmp-core` from importing `nmp-signers`.

Before this ADR, `RemoteSignerHandle` exposed only `sign()`. It had no way to
ask a signer to NIP-44-encrypt. Consequently the Phase 1 DM-send path
(`SendGiftWrappedDm`, on PR #122) had to call `nmp_nip59::gift_wrap`, which
takes raw `nostr::Keys` — so it works for local-key accounts and **fails for
NIP-46 bunker accounts**, whose key material never enters the kernel.

`RemoteSignerHandle` was sign-only; it now also exposes NIP-44. That is the
entire scope of this ADR.

## Decision

Add exactly two methods to the `RemoteSignerHandle` trait:

```rust
fn nip44_encrypt(&self, recipient_pubkey: &str, plaintext: &str) -> SignerOp<String>;
fn nip44_decrypt(&self, sender_pubkey: &str,    ciphertext: &str) -> SignerOp<String>;
```

- `recipient_pubkey` / `sender_pubkey` are lowercase-hex strings. The trait
  takes `&str`, **not** `&nostr::PublicKey`, so `nmp-core` stays free of a
  `nostr` type in its public trait surface — matching `sign()`, which takes the
  substrate type `&UnsignedEvent` rather than a `nostr::Event`. Concrete signers
  in `nmp-signers` parse the hex (`PublicKey::from_hex`) at the impl site before
  delegating to their existing `Nip44` trait impls.
- The return type is `SignerOp<String>`. For in-memory signers
  (`LocalKeySigner`) the work is synchronous and resolves as
  `SignerOp::Ready(Ok(ciphertext))` / `Ready(Err(..))`. For NIP-46 bunkers the
  call is an asynchronous RPC and resolves as `SignerOp::Pending(rx)` — the
  actor awaits it through the same publish-queue plumbing it already uses for
  `sign()`.

> Note: the `SignerOp` variants are `Ready` and `Pending`
> (`crates/nmp-signer-iface/src/op.rs`). Earlier drafts of this work referred to
> `SignerOp::Local` / `SignerOp::Remote` — those names do not exist; the doc
> comments on the trait use the real variant names.

### Why only two methods

The seal (step 1) is the **only** step that needs the sender's key, so it is the
only step that needs the signer seam. The outer gift-wrap (step 2) is encrypted
and signed with a **fresh ephemeral key the actor generates itself** — no signer
involvement, by design: re-using or exposing the account key for the outer wrap
would defeat NIP-59 unlinkability. `nip44_decrypt` is the symmetric inbound
counterpart, used to open an inbound kind:13 seal on the DM receive path.

No `nip44_*_to_self`, no key-derivation accessor, no `Keys` handle escapes the
signer. Two methods, both NIP-44, both addressed to / from a counterparty
pubkey. That is the minimal surface that unblocks bunker DMs.

### `active_local_nsec` / `mls_local_nsec` must not be read by DM send code

The DM-send executor (Phase 1's `SendGiftWrappedDm` arm, and any future
migration of it) must obtain encryption capability **only** through this seam —
`signer.nip44_encrypt(..)` + `signer.sign(..)` — and through the actor's own
identity state. It must never read `NmpApp::active_local_nsec` or
`mls_local_nsec`. Reading a raw nsec would (a) exfiltrate key material out of
the signer boundary and (b) still exclude bunker users, who have no local nsec
at all. This constraint is forward-facing: it governs the Phase 2.5/Phase 3
migration of the `dm.rs` actor arm onto this seam.

### `nmp_nip59::gift_wrap` stays a local-keys-only primitive

`nmp_nip59::gift_wrap` (`crates/nmp-nip59/src/wrap.rs`) keeps its current
signature — it takes raw `&nostr::Keys` and remains a kernel-agnostic,
local-keys-only convenience used by tests and the Marmot path. This ADR does
**not** change it. The new bunker-capable DM seal path is built on the
`RemoteSignerHandle` seam instead; the two coexist. The seal-via-seam path
constructs the kind:13 with `nip44_encrypt` + `sign`, and the kind:1059 outer
wrap with an actor-local ephemeral key.

### kind:1059 multiplexing — out of scope here

Both Marmot (NIP-59-wrapped MLS Welcome messages) and the NIP-17 DM inbox
subscribe to kind:1059 gift-wraps addressed to the user. The inbound demux —
deciding whether an unwrapped rumor is an MLS Welcome (kind:444) or a DM chat
message (kind:14) and routing it to the right projection — is a **Phase 4
concern**. It is named here only so the dependency is visible; it is not decided
by this ADR.

## Scope of the change (this PR)

- `crates/nmp-core/src/remote_signer.rs` — two new required trait methods.
- `crates/nmp-signers/src/signers/nip46/handle.rs` — `Nip46Signer`'s
  `RemoteSignerHandle` impl gains the two methods, parsing hex then delegating to
  the existing `Nip44` impl (`nip46/mod.rs`).
- `crates/nmp-signer-broker/src/broker.rs` — `ArcRemoteSigner` forwards the two
  methods to the inner `Nip46Signer`.
- `crates/nmp-core/src/actor/commands/remote_signer_tests.rs` — the
  `StubRemoteSigner` test double implements the two methods with real NIP-44
  (so plumbing tests behave like a real signer).

The actor `dm.rs` arm is **not** migrated in this PR — that file lands with
Phase 1 (PR #122) and its migration onto this seam is Phase 3.

## Consequences

- Bunker (NIP-46) accounts become *capable* of building a NIP-17 seal once the
  Phase 3 `dm.rs` migration consumes the seam. This ADR only builds the seam.
- Every `RemoteSignerHandle` implementor must now provide `nip44_encrypt` /
  `nip44_decrypt`; the methods are required (no default). A default returning
  `Err` would silently mask a missed impl on a future signer kind.
- `LocalKeySigner` and `Nip46Signer` already had `Nip44` impls (ADR-0015); this
  ADR only routes them through the actor-facing trait. No new crypto is written.
