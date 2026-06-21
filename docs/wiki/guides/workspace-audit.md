---
title: Workspace Audit
slug: workspace-audit
topic: developer-workflow
summary: "The architecture is confirmed in very good standing â codex verdict passed all 6 checks (D0, D6 production imports, D6 no display:: imports in kernel, doctrin"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-21
updated: 2026-06-19
verified: 2026-05-21
compiled-from: conversation
sources:
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:95d02563-5473-4d84-96e1-cd342e1b04d1
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
---

# Workspace Audit

## Workspace Audit

The architecture is confirmed in very good standing — codex verdict passed all 6 checks (D0, D6 production imports, D6 no display:: imports in kernel, doctrine lint 42/42, FFI header drift OK, C-ABI surface freeze OK) against commit 7213d7ba.

48 #[allow(dead_code)] annotations exist in nmp-core, all justified as cross-crate FFI callers, test helpers, or future lanes.

5 TODO/FIXME items exist across the workspace, covering coverage_hook, NIP-46/AuthSignerFn gap, bunker error disambiguation, and ViewDependencies limit field.

nmp-threading, nmp-nip59, and nmp-signer-iface originally had zero unit tests despite containing concurrency primitives and crypto adapters. PR-D (#199) added unit tests for nmp-signer-iface (20) and nmp-nip59 (3), but skipped nmp-threading due to mismatched scope. (Previously: zero unit tests.)

Memory was stale on 6 prior reviews: last_action_result was already a Vec, ActionPlan was removed, DmInboxProjection ships, nmp-reactions D0 is clean, ViewModule was deliberately deleted, and pending_mls_autopublish/actor_queue_depth are legitimate.

V-58 through V-60 were added to BACKLOG.md documenting reconnect-worker backoff blindness (V-58), EventStore missing kernel clock injection (V-59), and LMDB gc_step never evicting (V-60). V-57 P4 was expanded to document five concrete wasm publish-path gaps: AppAction variants not wired, NIP-46 bunker async transport missing, no native ActorCommand equivalent on wasm, unrecognized signer kinds, and zero wasm-bindgen-test coverage. V-38 received a new sub-item documenting the #[ignore] conformance test in nmp-nip47 blocked on Kernel::new_for_test() not being publicly exported. Four new Section 5 post-v1 rows were added: Chirp TUI unfinished interactions, nmp-content Phase-2 claim dependency channel, wasm32 test infrastructure, and web/registry CodeBlock placeholder. Three findings from the fallback search were folded into existing violations: J into V-43, L into V-14, X into V-57. The remote-signer unseal scaffolding finding folds into V-08 Stage 3 and is not a new violation. Eleven fallback findings were classified as D6-doctrined or dev-tool acceptable and skipped from BACKLOG addition.

Eight of ten scaffolding search domains (kernel, actor/ffi, store, router, signer-broker, marmot, nip29, app-chirp, iOS, web/wasm/gallery) returned zero unjustifiable scaffolding.

Two dead-code items (gift_wrap in nmp-nip59 and nip40_row in nmp-store) were identified as delete candidates not worth tracking as violations: they have zero callers and should be removed in a cleanup PR.

The #1517 audit found that shape_to_store_queries is already correct and complete per ADR-0045 E1–E3 — no production code changes were needed, only tests and documentation.

Issue #1493 tracks a 25-agent read-only audit of architectural exceptions where bespoke/special-case code crept into a framework that should have uniform architecture. It identifies 9 cross-cutting architectural-exception patterns (P1–P9) with LLM-authored code being too literal as the owner's thesis. P1 (presentation formatting in Rust projections, especially SF Symbol names in nmp-core) is ranked highest and P9 (hardcoded operator relays/pubkeys in generic layers) is the lowest-ranked actionable pattern. P2 findings are classified as stale/already-compliant: the repost triple-path is test-only (canonical nip18 exists), nip21.rs/tags.rs are compliant kind-agnostic protocol codecs (citing D0), longform/embed_registry already use named consts, and SELF_KINDS_TAILLING has an FFI override slot and kind:10050 is a deliberate OneShot.

The in-code 'thin-shell / glyph-stays-in-Rust (V-24)' and 'doctrine §6' comments are bogus violations — they cite aim.md §4.4 but §4.4 is actually about NIP-65 outbox routing and says nothing about presentation.

P4 Finding 4 (ExternalSignerCapabilityBridge transport + concurrent-Intent rejection) is NOT a violation — transport selection is a mechanical consequence of Rust-set fields and concurrent-Intent rejection is an OS Activity-Result launcher capacity constraint.

P8 (nmp-android-ffi as a detached workspace the lint never covers) was not assigned as a work lane in this campaign.

PR #1547's initial CI failure on wasm was caused by the branch being stale (predating #1572's wasm-safe split of ExternalEventSink dispatcher), not by p5's code changes.

<!-- citations: [^1c093-30] [^1c093-31] [^1c093-32] [^1c093-33] [^1c093-34] [^f2605-9] [^cd2b6-13] [^129d2-73] [^11850-89] [^11850-202] [^11850-234] -->
## Design Principles

Do not add feature flags to defer decisions; name the tradeoff and pick a side. <!-- [^1c093-35] -->

All violation claims must be verified against current master HEAD before being listed in the backlog, because some Opus review claims are already fixed on HEAD. <!-- [^95d02-17] -->

chats.rs and groups.rs must each stay under 300 LOC per the doctrine file-size gate. <!-- [^93c59-22] -->
