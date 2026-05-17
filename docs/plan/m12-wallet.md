# M12 — Wallet (NWC + zaps + Cashu + nutzaps)

> Part of the [Build & Validation Plan](../plan.md). Arc 3 — wallet/WoT + cross-platform + release.

**Demo product:** Twitter slice gets a zap button on each post. Tapping it pays via NWC. Receiving zaps shows up in a zap-history view. Cashu nutzap claim works.

**Scope.** Per spec §7.9:

**Subsystem deliverables.**

- `nmp-nwc` protocol module: NIP-47 client; pay/receive/balance.
- `nmp-nip57` protocol module: LUD-16 discovery + zap request building + receipt verification.
- `nmp-nip60` protocol module: Cashu wallet event types + proof state in domain store.
- `nmp-nip61` protocol module: Nutzap action module; pending-nutzap claim flow.
- `WalletBalance` view module; `ZapHistory` view module.
- Zap action module: `Zap { target, sats, comment }` on the action ledger.

**Exit gate.**

- Pay a 100-sat zap via NWC to a real LUD-16 endpoint; receipt verifies; balance updates within one ViewBatch.
- Receive a zap (test via a separate device or simulated): zap-history view reflects within one ViewBatch.
- Nutzap claim from a Cashu mint: proofs land in the wallet; balance updates.
- Wallet operations never block the UI thread.

**Runnable artifact.** iOS Twitter slice with working zaps. Report in `docs/perf/m12/wallet.md`.
