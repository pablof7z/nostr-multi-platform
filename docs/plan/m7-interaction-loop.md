# M7 — Reactions + Thread + Reply (the interaction loop)

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** Twitter slice user can like a post, reply to it, see the thread, and have the reply land in primal.

**Scope.** `nmp-nip25` (Reactions view module + React action), `nmp-nip10` (Thread view module with NIP-10 reply-marker handling), `SendNote` extended for `reply_to`.

**Subsystem deliverables.**

- Reactions view module with NIP-25 emoji normalization (`+` and missing content → "like"; deduplicate by `(pubkey, emoji)`).
- React action module on the action ledger.
- Thread view module with reply-marker handling (NIP-10 `marker = reply | root | mention` plus legacy positional fallback). Orphan support.
- iOS UI: like button on each timeline row; tap → thread screen with nested replies; reply composer.

**Exit gate.**

- Tap-to-thread → see reply tree built correctly; orphan storm test (1000 replies in random order, 50% parents arriving after children) builds tree identical to known-good single-pass; build time ≤ 50 ms.
- Reactions aggregation: 10k reactions over 30 s coalesce to ≤ 60 deltas/sec/view per ADR-0002.
- Reply published from iOS arrives back via the live tail and slots into the thread tree without flicker.

**Runnable artifact.** iOS Twitter slice with complete read/like/reply loop. Report in `docs/perf/m7/interaction-loop.md`.
