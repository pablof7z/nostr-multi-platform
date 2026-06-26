# ADR-0031 — `nmp-signer-broker` owns the NIP-46 relay transport; it does not use `nostr-connect`

- **Status:** Superseded by actor-lane design / PR-B2 #2119 (nmp-signer-broker deleted)
- **Date:** 2026-05-24
- **Resolves:** V-36.
- **Related:** ADR-0022 (NMP owns its relay transport), ADR-0026 (signer NIP-44 seal seam),
  ADR-0027 (unified `ActionModule` trait), ADR-0050 (signer-session capability port —
  decides the remote-signer verb-set extension shape)

## Context

`docs/aim.md` §3 names the `nostr-connect` crate (from the rust-nostr workspace) as the
intended NIP-46 dependency. NMP ships `crates/nmp-signer-broker/` instead — a hand-rolled
NIP-46 relay transport. No ADR was written at the time to justify this divergence.

`nostr-connect` does not appear anywhere in the workspace:

```
$ grep -rn "nostr-connect\|nostr_connect" Cargo.toml crates/*/Cargo.toml
(no matches)
```

The broker crate (`nmp-signer-broker/`) has four sub-modules: `broker`, `handshake`,
`relay_client`, and `transport`. `relay_client.rs` (~640 LOC) is the custom WebSocket client
that was the subject of V-13 (polling violation) and V-14 (no reconnect), both fixed in
PR #431.

## Why `nostr-connect` was not used

### 1. D0 — kernel must not import NIP-46 specifics

`nmp-core` is the substrate crate: every NIP-specific crate depends on it; it must not
depend on them. NIP-46 wiring requires the kernel actor to receive a signer object after
the handshake completes and to forward sign-requests to it. If `nmp-core` imported
`nostr-connect` directly, it would acquire a hard dependency on a NIP-specific crate —
the inversion D0 forbids.

The broker solves this by living _outside_ `nmp-core` and reaching back through the
`bunker_hook` indirection: `nmp_signer_broker_init` calls
`nmp_core::register_bunker_hook(...)` with a closure that captures the broker. The closure
pushes work onto a worker thread and returns immediately — the kernel actor never blocks
and never sees `nmp-signer-broker` in its import graph.

This pattern cannot be replicated with `nostr-connect` as it stands: the crate's public API
surface requires the caller to manage async task handles (tokio) and session state in the
same place, which would force either (a) `nmp-core` to depend on it or (b) significant
upstream changes to introduce the hook indirection.

### 2. Async model mismatch — NMP is synchronous/blocking; `nostr-connect` is tokio-first

The NMP actor thread owns all mutable state on one OS thread (D4). No tokio runtime exists
inside `nmp-core`; the relay-worker pool runs synchronous blocking I/O gated on `mio::Poll`
(ADR-0022, D8). Adopting `nostr-connect` would require either embedding a tokio runtime
for the broker session only (a different execution model from every other I/O path in the
binary) or blocking on `tokio::runtime::Runtime::block_on` (spawning a tokio runtime on a
background thread per session, with no shared scheduler).

`nmp-signer-broker` uses the same `mio`-based non-polling model as the kernel relay worker.
This keeps the threading model uniform: one OS thread per bunker session, one `mio::Poll`
that blocks until the OS notifies of socket readiness or a `mio::Waker` wakes it for a
control message. The D8 "no polling" rule is enforced by the same mechanism across all I/O
paths.

### 3. Multi-relay broadcast and the relay-role model

NIP-46 bunker URIs may list multiple relays. The broker's `BunkerBroker` connects to all
listed relays simultaneously and broadcasts outbound RPCs to all of them, collapsing
inbound responses by NIP-46 request ID. This is required for relay-level redundancy: if
one relay drops the session, the signer still receives the RPC via another.

`nostr-connect`'s session model maps one session to one relay URL. Adapting it to the
multi-relay pattern would require forking the session-management core.

### 4. NMP-specific progress reporting (D12 / action_stages)

The broker emits `ActorCommand::BunkerHandshakeProgress` snapshots as the handshake
proceeds (`"connecting"` → `"awaiting_pubkey"` → `"ready"` | `"failed"`). These snapshots
are consumed by the kernel to update the `bunker_handshake` projection, which the host UI
polls to render live feedback. This is NMP's action-stages protocol (D12) applied to the
handshake path — it has no counterpart in `nostr-connect`.

### 5. NIP-46 verb-set extension for remote signers

Remote-signer flows (NIP-42 AUTH challenges, DM decryption) need NIP-46 verbs
that `nostr-connect` may not support. Owning the broker lets NMP extend the
verb set without depending on upstream merges. **ADR-0050** landed the scalar
signer-session port (`sign | nip44_encrypt | nip44_decrypt`) and the bounded
remote-signer DM fallback. **ADR-0066** decides the optional delegated
batch-decrypt session for bunkers that negotiate it. The older per-envelope
`unwrap_gift_wrap` sketch was rejected as unviable, since each kind:1059 unseal
is two sequential interactive NIP-46 decrypts.

> **SUPERSEDED (#2119, 2026-06-27).** The standalone `nmp-signer-broker` crate has been
> **deleted**. NIP-46 no longer owns a bespoke socket/dispatcher: it rides the actor's
> shared `Pool` relay lane, driven by the `nmp-nip46-runtime` crate (the pure reducer
> lives in `nmp-nip46`). D0 is still satisfied — `nmp-core` names neither `nmp-signers`
> nor any NIP-46 type; the wiring lives in `nmp-ffi` (above `nmp-core` in the DAG) behind
> the `signer-broker` cargo feature. The original decision below is retained verbatim for
> historical context; wherever it says "the broker owns the transport," read
> "`nmp-nip46-runtime` drives the transport over the actor `Pool` lane."

## Decision (historical — superseded)

`nmp-signer-broker` was declared **canonical maintained infrastructure**. It was not a
stopgap: it existed to satisfy D0, the mio execution model, multi-relay redundancy, and
NMP-specific progress telemetry — none of which `nostr-connect` provides out of the box.
(#2119 later achieved the same D0 + multi-relay + telemetry guarantees by folding the
transport onto the shared actor `Pool` lane, removing the separate crate entirely.)

`aim.md` §3 is updated by this ADR. The corollary "Use rust-nostr, not scratch crypto" applies to cryptographic primitives only
(NIP-44, bech32, key derivation); it does not require using rust-nostr's relay-transport
or session-management layers where they conflict with NMP's synchronous actor model.

## Long-term exit options

**Option A — upstream multi-relay + mio support to `nostr-connect`:** Contribute the
multi-relay broadcast model and blocking/mio integration to the rust-nostr project, then
migrate. Estimated effort: significant (multiple upstream PRs, coordination, waiting for
releases). Pre-condition: rust-nostr adopts a non-tokio relay-transport model. Risk:
upstream timeline is out of NMP's control.

**Option B — maintain `nmp-signer-broker` as-is:** Continue improving the broker in-tree,
optionally extracting the non-NMP-specific relay-client primitive into a shared
`nmp-relay-conn` crate and sharing it with `nmp-core`'s relay worker.

**Current ruling (as of #2119):** Neither Option A nor Option B. The duplicate-transport
code smell was eliminated outright by deleting `nmp-signer-broker` and having
`nmp-nip46-runtime` drive NIP-46 over the actor's existing shared `Pool` relay lane — the
same socket layer the kernel uses for all outbound Nostr traffic. No separate
relay-connection crate was needed; the actor `Pool` IS the shared transport.

## Consequences

- New contributors reading `aim.md` §3 will see a note pointing to this ADR.
- Every future NIP-46 verb extension (AUTH challenges, DM decryption via the
  ADR-0050 session port) extends `nmp-signer-broker`'s verb set — no upstream
  dependency approval needed.
- V-13, V-14 fix tickets are properly framed as fixes to maintained infrastructure, not
  arguments that the crate should not exist.
- V-36 closes.
