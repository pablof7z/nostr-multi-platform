# Subsystem coverage matrix

> Part of the [Build & Validation Plan](../plan.md).

Cross-reference of which milestone delivers which user-specified concern.

| Concern | Milestone(s) | Notes |
|---|---|---|
| **Outbox routing (NIP-65)** | [M2](m2-subscription-compilation.md) | First-class as a planner stage, not a side feature. Diagnostics show per-relay coverage. |
| **NDK-style subscription aggregation** | [M2](m2-subscription-compilation.md) | Per `docs/design/ndk-applesauce-lessons.md` §7, the planner becomes a subscription compiler. Logical interests → per-relay plans → wire REQs, semantics-preserving merge/split. |
| **Reactivity as planned** | [M0](m0-fixture.md)–[M7](m7-interaction-loop.md) | Already validated by reactivity-bench run 002 against the model; M1 runs the same code path against real iOS; subsequent milestones add view modules that exercise the contract under varied loads. |
| **Non-Nostr data bridge** | [M0](m0-fixture.md) (substrate), [M10](m10-blossom.md) (long-running capabilities), [M11](m11-podcast.md) (podcast app proves it in production) | DomainModule trait + ADR-0007 bridge lanes; first proven by fixture-todo-core; production proof in podcast app. |
| **FFI hardening + empirical iOS proof** | [M10.5](m10.5-ffi-hardening.md) | Dedicated stress harness, real-device measurement, simulator-driven Sonnet-agent UI suite; hard gate before M11. |
| **UI parity to `../podcast`** | [M11](m11-podcast.md) (copy step) | Every Swift view copied verbatim, screenshot-diff gated. |
| **NIP-42 auth** | [M5](m5-nip42.md) | Per-relay auth state machine; integrates with diagnostics; works with both local-key and NIP-46 signers. |
| **Blossom** | [M10](m10-blossom.md) | Upload + download with resumable progress; long-running capability lifecycle. |
| **Multi-session clients** | [M8](m8-multi-account.md) | Per-account view-spec scoping; account switcher; isolation tests. |
| **NIP-77 negentropy** | [M4](m4-negentropy.md) | Sync engine with watermarks; planner consults before REQ; capability negotiation; bytes-saved diagnostic. |
| **Podcast-class apps** | [M11](m11-podcast.md) (proof), [M10](m10-blossom.md) (capabilities prerequisite) | AudioPlaybackCapability, BackgroundWorkCapability, BlossomDownloadCapability all generic; podcast-specific domain in `podcast-core` app crate. |

## NIP support roadmap at v1

> Note: M9 (NIP-17 DMs) and M12 (Wallet) were deferred to post-v1 per [scope-adjustments-2026-05-18](scope-adjustments-2026-05-18.md). NIP-29 is now v1 via [M11.5](m11.5-highlighter.md).

| NIP | Module | Milestone | Status |
|---|---|---|---|
| 01 | nmp-nip01 | [M1](m1-twitter-slice.md), [M6](m6-signers-write.md) | partial (reads in M1; writes in M6) |
| 02 | nmp-nip02 | [M2](m2-subscription-compilation.md) | follow-list parsing (contacts view) |
| 04 | not v1 | — | superseded by NIP-44/17; not implemented |
| 05 | nmp-nip01 | [M1](m1-twitter-slice.md) | NIP-05 verification in Profile module |
| 07 | nmp-nip07 | [M15](m15-cross-platform.md) | web-only browser signer |
| 09 | nmp-nip01 | [M3](m3-persistence.md) | kind:5 deletes (full handling) |
| 10 | nmp-nip10 | [M7](m7-interaction-loop.md) | reply markers in thread building |
| 17 | nmp-nip17 | [post-v1](post-v1.md) | DMs — deferred |
| 19 | nmp-nip19 | [M1](m1-twitter-slice.md) | bech32 utility used throughout |
| 23 | not v1 | — | long-form reader is post-v1 |
| 25 | nmp-nip25 | [M7](m7-interaction-loop.md) | reactions |
| 29 | nmp-nip29 | [M11.5](m11.5-highlighter.md) | groups (Highlighter rebuild) |
| 40 | nmp-nip01 | [M3](m3-persistence.md) | expiration scheduling |
| 42 | nmp-nip42 | [M5](m5-nip42.md) | relay auth |
| 44 | nmp-nip17 | [post-v1](post-v1.md) | encryption via NIP-17 — deferred |
| 46 | nmp-nip46 | [M6](m6-signers-write.md) | bunker signer |
| 47 | nmp-nwc | [post-v1](post-v1.md) | wallet connect — deferred |
| 49 | nmp-nip01 / nmp-nip46 | [M6](m6-signers-write.md) | encrypted-key import |
| 55 | nmp-nip55 | [M15](m15-cross-platform.md) | Android Amber bridge |
| 57 | nmp-nip57 | [post-v1](post-v1.md) | zaps — deferred |
| 59 | nmp-nip17 | [post-v1](post-v1.md) | gift wrap via NIP-17 — deferred |
| 60 | nmp-nip60 | [post-v1](post-v1.md) | Cashu — deferred |
| 61 | nmp-nip61 | [post-v1](post-v1.md) | nutzaps — deferred |
| 65 | nmp-nip65 | [M2](m2-subscription-compilation.md) | mailboxes + outbox |
| 77 | nmp-nip77 | [M4](m4-negentropy.md) | negentropy |
| Blossom BUD-01/02 | nmp-blossom | [M10](m10-blossom.md) | media |

NIPs not in v1 (e.g., NIP-23 long-form, NIP-71 video) become post-v1 extension modules; the kernel boundary makes them additive.
