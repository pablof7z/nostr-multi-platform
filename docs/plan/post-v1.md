# Post-v1 milestones

> Part of the [Build & Validation Plan](../plan.md).

These milestones were deferred from the v1 ladder per [scope adjustments 2026-05-18](scope-adjustments-2026-05-18.md). They are not dropped — they are sequenced after [M17](m17-release.md).

## Post-v1 M9 — NIP-17 DMs + NSE

See [`m9-messaging.md`](m9-messaging.md) for the full milestone spec (scope, subsystem deliverables, exit gate). **Deferred reason:** DMs add NSE, gift-wrap, NIP-44, App Groups, and a whole capability lane that are not load-bearing for v1 doctrine proofs. The outbox planner's structural ban on routing private events to non-inbox relays is implemented in [M2](m2-subscription-compilation.md) regardless — so the routing contract is already enforced at v1; DMs slot in cleanly when this milestone runs post-v1.

## Post-v1 M12 — Wallet (NWC + zaps + Cashu + nutzaps)

See [`m12-wallet.md`](m12-wallet.md) for the full milestone spec. **Deferred reason:** Wallet is large surface area (NWC, NIP-47, NIP-57, NIP-60, NIP-61) and not load-bearing for v1 kernel-boundary proofs. When wallet lands post-v1, NIP-57 ships with it. LUD-16 zaps remain possible via an extension before this milestone.
