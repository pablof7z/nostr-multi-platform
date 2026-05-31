---
title: NMP Signer Broker & ADR-0031 Justification
slug: nmp-signer-broker
summary: "ADR-0031 justifies `nmp-signer-broker` existing instead of `nostr-connect` for five reasons: D0 compliance, async mismatch (mio/blocking vs tokio), multi-relay"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-28
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:156aa64b-42e1-4d3b-96ce-25b31fc06fec
  - session:1670fcb8-f275-498c-975b-8bd912331ded
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:594b7c34-efd1-4461-81ad-9fa33a6e76f9
---

# NMP Signer Broker & ADR-0031 Justification

## Rationale

ADR-0031 justifies `nmp-signer-broker` existing instead of `nostr-connect` for five reasons: D0 compliance, async mismatch (mio/blocking vs tokio), multi-relay broadcast, D12 progress telemetry, and verb extensibility. NMP cannot adopt nostr-relay-pool because it spawns tokio tasks with no external-step API — the async-loop mismatch is architectural, not a version issue. V-65 tracks the hardcoded NOSTRCONNECT_DEFAULT_RELAY_URL = wss://relay.damus.io in nmp-core, which violates D0 and creates a third-party dependency.

NIP-07 signing works on WASM via `window.nostr.signEvent`. NIP-46 bunker signing on WASM is blocked on a wasm-native async transport for bunker RPC. [^594b7-6]

<!-- citations: [^12b3f-21] [^1670f-13] [^cd2b6-17] -->
## Bunker DM Access & Local-Key Gate

Bunker users are currently locked out of NIP-17 DMs by an artificial gate at `commands/dm.rs:87`, despite `RemoteSignerHandle::nip44_encrypt/decrypt` already existing. To achieve bunker parity, `&Keys` consumers must be migrated to `&dyn RemoteSignerHandle`. Specifically, Theme C requires `nmp_nip59::gift_wrap_with_signer(signer: &dyn SignerForSeal, ...)` satisfied by both `nostr::Keys` and `&dyn RemoteSignerHandle`, which deletes the local-key gate at `dm.rs:87`. [^1c093-26]


The RemoteSignerForSeal adapter bridges RemoteSignerHandle to the SignerForSeal trait, reusing the bunker's nip44_encrypt directly for NIP-44 symmetric sealing. [^128] [^156aa-6]
## D13 Lint & ADR-0026 Key-Access Bans

D13 lint bans `identity.active_local_keys()` in DM/zap paths in `nmp-core` and bans `marmot_local_nsec` access outside `nmp-marmot`. ADR-0026 explicitly forbids reading `marmot_local_nsec` from DM/zap paths. [^1c093-27]


The A1 bunker NIP-44 adapter implementation closes ADR-0026 Phase 2. [^121] [^156aa-7]

V-78 tracks that Bunker (NIP-46) accounts cannot zap because kind:9734 requires local keys and ADR-0026 Phase 2 is not started. [^cd2b6-18]
## Bunker Async-Pending Modeling

Bunker async-pending modeling reuses the existing `PendingSign` machinery at `actor/pending_sign.rs`, extended to cover NIP-44 ops. PR-E Phase 2 for bunker DMs must extend `PendingSign` rather than use the OS-thread driver pattern. [^1c093-28]


IdentityRuntime.remote_signers uses Arc<dyn> internally while the boundary API remains unchanged so ActorCommand::AddRemoteSigner still takes Box<dyn> and converts on insertion. [^129] [^156aa-8]
## NIP-46 Timeout Considerations

The 5s timeout for NIP-46 is possibly too aggressive for the encrypt+sign chain, which may require approximately 10s. [^1c093-29]
## See Also

