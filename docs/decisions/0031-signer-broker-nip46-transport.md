# ADR-0031 — `nmp-signer-broker` owns the NIP-46 relay transport; it does not use `nostr-connect`

- **Status:** Accepted
- **Date:** 2026-05-24
- **Resolves:** V-36 in `docs/BACKLOG.md`
- **Related:** ADR-0022 (NMP owns its relay transport), ADR-0026 (signer NIP-44 seal seam),
  ADR-0027 (unified `ActionModule` trait)

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
`relay_client`, and `transport`. `relay_client.rs` (851 LOC) is the custom WebSocket client
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

### 5. V-06/V-08 substrate gaps — NIP-42 AUTH + NIP-17 gift-wrap for remote signers

Stages 2-3 of V-06 (broker `sign_auth_challenge` RPC) and Stage 3 of V-08 (`unwrap_gift_wrap`
via remote signer RPC) extend the broker's NIP-46 verb set with verbs `nostr-connect` may
not support. Owning the broker lets NMP extend the verb set without depending on upstream
merges.

## Decision

`nmp-signer-broker` is declared **canonical maintained infrastructure**. It is not a
stopgap: it exists to satisfy D0, the mio execution model, multi-relay redundancy, and
NMP-specific progress telemetry — none of which `nostr-connect` provides out of the box.

`aim.md` §3 is updated by this ADR: the `nostr-connect` reference is superseded. The
corollary "Use rust-nostr, not scratch crypto" applies to cryptographic primitives only
(NIP-44, bech32, key derivation); it does not require using rust-nostr's relay-transport
or session-management layers where they conflict with NMP's synchronous actor model.

## Long-term exit options

**Option A — upstream multi-relay + mio support to `nostr-connect`:** Contribute the
multi-relay broadcast model and blocking/mio integration to the rust-nostr project, then
migrate. Estimated effort: significant (multiple upstream PRs, coordination, waiting for
releases). Pre-condition: rust-nostr adopts a non-tokio relay-transport model. Risk:
upstream timeline is out of NMP's control.

**Option B — maintain `nmp-signer-broker` as-is:** Continue improving the broker in-tree,
extract the non-NMP-specific relay-client primitive into `crates/nmp-relay-conn/` (the
V-13 Stage 1 plan in BACKLOG.md), and share it with `nmp-core`'s relay worker.

**Current ruling:** Option B. The broker owns the NIP-46 relay transport. V-13's
`nmp-relay-conn` extraction (when it lands) eliminates the remaining duplicate-transport
code smell without changing the architectural decision.

## Consequences

- New contributors reading `aim.md` §3 will see a note pointing to this ADR.
- Every future NIP-46 verb extension (V-06 Stages 2-3, V-08 Stage 3) extends
  `nmp-signer-broker`'s verb set — no upstream dependency approval needed.
- V-13, V-14 fix tickets are properly framed as fixes to maintained infrastructure, not
  arguments that the crate should not exist.
- V-36 closes.
