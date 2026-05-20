# Subsystem Coverage Matrix

> Part of the [Build & Validation Plan](../plan.md). Chirp is the only active app proof during the current architecture-validation phase.

## Core Concerns

| Concern | Milestone(s) | Notes |
|---|---|---|
| Outbox routing (NIP-65) | [M2](m2-subscription-compilation.md) | Planner stage; diagnostics should show per-relay coverage. |
| NDK-style subscription aggregation | [M2](m2-subscription-compilation.md) | Logical interests compile into per-relay plans and wire REQs. |
| Reactivity contract | [M0](m0-fixture.md)–[M7](m7-interaction-loop.md) | Validate through Chirp and test harnesses. |
| Non-Nostr data bridge | [M0](m0-fixture.md), [M10](m10-blossom.md) | Proved first by `fixture-todo-core`; production proof deferred until Chirp is stable. |
| FFI hardening + empirical app proof | [M10.5](m10.5-ffi-hardening.md) | Stress harness, simulator/device evidence, and Chirp runtime validation. |
| NIP-42 auth | [M5](m5-nip42.md) | Per-relay auth state machine integrated with signers. |
| Blossom | [M10](m10-blossom.md) | Upload/download with resumable progress and capability lifecycle. |
| Multi-session clients | [M8](m8-multi-account.md) | Per-account view scoping and account-switch isolation. |
| NIP-77 negentropy | [M4](m4-negentropy.md) | Sync engine with watermarks and planner integration. |
| Marmot MLS encrypted groups | [post-v1 Marmot](marmot-mls.md) | Retained as generic Nostr group infrastructure. |

## NIP Support Roadmap

| NIP | Module | Milestone | Status |
|---|---|---|---|
| 01 | `nmp-nip01` | M1/M6 | Events, profiles, and write-path integration. |
| 05 | `nmp-nip01` | M1 | NIP-05 profile support. |
| 10 | `nmp-threading` | M7 | Reply/thread semantics. |
| 22 | `nmp-nip22` | M7 | Comments. |
| 23 | `nmp-nip23` | post-v1 or later v1 decision | Long-form support retained as generic code. |
| 25 | `nmp-reactions` | M7 | Reactions. |
| 29 | `nmp-nip29` | Generic group infrastructure | Retained; not tied to an app port. |
| 42 | `nmp-nip42` | M5 | Relay auth. |
| 46 | signer crates | M6 | Bunker signer. |
| 47 | `nmp-nwc` | scope decision | NWC code exists; broader wallet scope remains separate. |
| 57 | `nmp-nip57` | scope decision | Zap receipt parsing exists. |
| 59 | `nmp-nip59` | group/messaging infrastructure | Gift-wrap primitives retained. |
| 65 | planner / core | M2 | Mailboxes and outbox routing. |
| 77 | `nmp-nip77` | M4 | Negentropy. |
| Blossom BUD-01/02 | future `nmp-blossom` | M10 | Media capability lifecycle. |

Additive protocol modules are acceptable when they are reusable Nostr infrastructure. New product app surfaces are deferred until Chirp proves the architecture.
