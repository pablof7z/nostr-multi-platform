# M8 — Multi-session (multi-account) clients

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** Twitter slice gets an account switcher. Logged-in users can add a second account, switch between them, and each account's timeline / contacts / reactions are correctly isolated.

**Scope.** Per spec doctrine D4 (single writer per fact) extended to account scope:

**Subsystem deliverables.**

- Session model in the kernel: `SessionState { accounts, active, status }` with N accounts simultaneously valid.
- View specs that depend on the active account (Timeline of "your follows", DMs inbox, zap history) get account-scoped composite keys.
- Account switch is an action with full rebuild semantics — open views for the new active account, close the prior ones, projection caches stay populated across switches when overlap exists.
- Per-account signer binding (each account has its own `IdentityId`).
- Per-account secure storage namespacing in `KeychainCapability`.

**Exit gate.**

- Bug-extinction #5 (account-context overlap): two accounts active, switch between them, assert no state bleed. `AppState` snapshot for account A never contains data scoped to account B's session-aware views.
- Switching accounts during an in-flight publish: the publish is account-tagged, completes correctly, lands in the originating account's timeline only.
- Per-account signer never signs an event for the wrong account (test forces dispatch through a wrong-account signer; the action ledger rejects).

**Runnable artifact.** Account switcher in iOS demo with two real test accounts. Report in `docs/perf/m8/multi-account.md`.
