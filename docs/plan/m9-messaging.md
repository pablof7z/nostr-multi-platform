# M9 — NIP-17 DMs + NSE

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** Chirp gets a DMs tab. End-to-end NIP-17 gift-wrapped messages between two test accounts. Background push triggers iOS Notification Service Extension decryption; opening the app shows the message already in place.

**Scope.** Per spec §7.10 and §7.14:

**Subsystem deliverables.**

- `nmp-nip17` protocol module: Conversation view module + ConversationList view module; SendDm action module; NIP-44 encryption / NIP-59 gift-wrapping; outbox routing for DMs (recipient inbox relays only — never public).
- `nmp-nip17-nse` companion crate: `decrypt_push()` API with bounded memory (≤ 24 MB peak, ≤ 200 ms p99), reading from shared keychain and shared App Group storage.
- iOS NSE target wiring: silent push from APNs → NSE invokes `decrypt_push` → notification posted with decrypted preview.
- Action atomicity for `SendDm`: gift-wrap → publish to all recipient inboxes → insert locally — atomic.

**Exit gate.**

- Bug-extinction #4 (DM to public): no API path can send a DM to a non-inbox relay; planner refuses non-inbox relays for `p`-tagged-only events.
- DM round-trip in `MockRelay` (alice ↔ bob): content matches; no plaintext crosses FFI other than as `ConversationMessage.body`.
- NSE decrypt of an incoming gift-wrap: p99 ≤ 200 ms, peak memory ≤ 24 MB.
- Backgrounded app receives a push, NSE decrypts and posts notification, app foregrounded shows the message in place (no re-fetch from relay).

**Runnable artifact.** Chirp with working DMs + push notifications. Report in `docs/perf/m9/messaging.md`.
