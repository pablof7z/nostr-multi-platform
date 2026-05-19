# M5 — NIP-42 auth

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** iOS app connects to an NIP-42-required relay (such as a private nostr.wine subscription) and successfully authenticates + receives content.

**Scope.** Per-relay auth state machine: relay sends `AUTH` challenge → kernel routes to active signer → signer produces kind:22242 → kernel sends `AUTH` back → relay acknowledges → subscriptions resume. Auth failures surface as `RelayAuthState::Failed` in diagnostics (ADR-0007 §1).

**Subsystem deliverables.**

- `nmp-nip42` protocol module: auth challenge handling, kind:22242 builder, per-relay auth state.
- Planner pauses subscriptions on a relay while it's in `ChallengeReceived` / `Authenticating` states.
- `KeyringCapability` minimal API used to sign auth events (full signer trait still [M6](m6-signers-write.md)).
- Diagnostics: `RelayAuthState` rendered per relay.

**Exit gate.**

- Test relay configured with NIP-42 required: connection completes with auth, subscriptions deliver events.
- Auth failure (wrong signer) produces a visible diagnostic state and a toast in the app; subscriptions stay paused until resolved.
- Re-authentication on reconnect works without re-issuing logical subscriptions.

**Runnable artifact.** iOS app working against an NIP-42-required relay. Report in `docs/perf/m5/nip42.md`.
