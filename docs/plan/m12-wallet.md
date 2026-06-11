# M12 — Wallet (NWC + zaps + Cashu + nutzaps)

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — wallet/WoT + cross-platform + release.

## What shipped for v1 (owner decision 2026-06-12)

v1 ships the current zap capability; further zap work is explicitly post-v1:

- **`nmp-nip47`** — NWC client (`pay_invoice`, balance, receive).
- **`nmp-nip57`** — LUD-16 LNURL discovery + zap-request building.
- **kind:9735 ingest** + **`ZapsAggregateProjection`** — live aggregate in kernel.
- **E2E runtime harness** — NWC `pay_invoice` → kind:9735 → `ZapsAggregateProjection` verified (PR #1076, closes F-04 [#978](https://github.com/pablof7z/nostr-multi-platform/issues/978)).

## Post-v1 remaining scope (full M12)

**Demo product (post-v1):** Chirp gets a zap button on each post. Tapping it pays via NWC. Receiving zaps shows up in a zap-history view. Cashu nutzap claim works.

**Remaining subsystem deliverables.**

- `nmp-nip60` protocol module: Cashu wallet event types + proof state in domain store.
- `nmp-nip61` protocol module: Nutzap action module; pending-nutzap claim flow.
- `WalletBalance` view module; `ZapHistory` view module.
- Receipt `nostrPubkey` author verification — [#1043](https://github.com/pablof7z/nostr-multi-platform/issues/1043).
- `ZapRequestBuilder` sentinel-value API fix — [#610](https://github.com/pablof7z/nostr-multi-platform/issues/610).
- `zap_subscription` typed-sidecar shape decision — [#1022](https://github.com/pablof7z/nostr-multi-platform/issues/1022).

**Remaining exit gate (post-v1).**

- Pay a 100-sat zap via NWC to a real LUD-16 endpoint; receipt verifies (nostrPubkey checked); balance updates within one ViewBatch.
- Receive a zap (test via a separate device or simulated): zap-history view reflects within one ViewBatch.
- Nutzap claim from a Cashu mint: proofs land in the wallet; balance updates; retrying the same nutzap after restart returns before mint I/O.
- Wallet operations never block the UI thread.

**Runnable artifact (post-v1).** Chirp with full zap + Cashu UX. Report in `docs/perf/m12/wallet.md`.
