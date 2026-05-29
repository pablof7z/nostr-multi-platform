---
title: A Migration Is Not Done Until the New Path Is Live — Dead-Code Decoders Are Incomplete Migrations
slug: half-landed-migration-is-not-done
summary: Typed decoders that compile but are never wired into the render/call path are incomplete migrations, not completed ones. Dual live code paths for the same concern violate the no-dual-seam doctrine.
tags:
  - migration
  - architecture
  - adr
  - flatbuffers
  - no-dual-seam
volatility: cold
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:cd331450-f93f-48d0-960e-3c73e927775e
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
---

# A Migration Is Not Done Until the New Path Is Live — Dead-Code Decoders Are Incomplete Migrations

> ADR-0038's typed FlatBuffers migration was marked complete and its follow-up items (V-84/V-85) were classified post-v1 MEDIUM — but the typed decoders existed only as dead code while the generic path remained the live preferred path. This is the canonical example of a half-landed migration being mislabelled as done.

## Details

- **Definition of done for a migration:** The new path must be the *live, preferred path* actually exercised at runtime. Code that compiles but is never called by the render or call path is not a completed migration — it is scaffolding.
- Dead-code decoders alongside a still-live generic path constitute a dual seam. The no-dual-seam doctrine prohibits two active code paths for the same concern regardless of how the backlog item is labelled or prioritised.
- When reviewing a migration PR or closing a migration ticket, explicitly verify:
  1. The new path is wired into the render/call path.
  2. The old path is either deleted or gated behind a clearly temporary feature flag with a removal date.
  3. No `#[allow(deprecated)]` or equivalent suppression is hiding continued use of the old path (see `allow-deprecated-scaffolding-tracks-migration-window`).
- Backlog labels such as "post-v1 MEDIUM" do not change the architectural state. If the old path is still live, the migration is still in progress and must be tracked as such.
- This applies equally to platform-side migrations: a Kotlin/Swift typed decoder that is never called is not a completed migration of the Android/iOS render path.


### Additional Rule


A `#[deprecated]` annotation with the old path still live is not an acceptable interim state. "In the middle of deprecating" IS the violation — the old path must be fully removed, not left annotated while the migration stalls. Every deprecated symbol with zero remaining callers must be deleted immediately. Every deprecated symbol with remaining `#[allow(deprecated)]` callers must have those callers migrated and the symbol then deleted. Half-landed is not done. [^d0690-37]
## Dual-Seam Classification Is a Blocking Issue, Not Polish

When a migration introduces a new code path but leaves the old path live and the new path as dead code, the migration is HALF-DONE — not complete with polish remaining. NMP doctrine prohibits dual seams; any backlog item that leaves two parallel paths for the same concern must be classified as **blocking completion**, not post-launch polish.

Context: ADR-0038 typed FlatBuffers work was marked complete and post-v1 items V-84/V-85 were classified MEDIUM priority, but the new typed decoders were never wired into the live render path — making the migration half-landed by definition. Do not close or downgrade such items until the old path is deleted and the new path is the sole live path.
## See Also
- [[allow-deprecated-scaffolding-tracks-migration-window|allow deprecated scaffolding tracks migration window]] — related guide
- [[android-stale-render-model-pre-v80|android stale render model pre v80]] — related guide
- [[nfct-native-decoder-not-ffi|nfct native decoder not ffi]] — related guide
- [[nfct-native-decoder-not-ffi|nfct native decoder not ffi]] — related guide
- [[android-stale-render-model-pre-v80|android stale render model pre v80]] — related guide
- [[actor-thread-blocking-highest-severity|Synchronous Blocking on the Kernel Actor Thread Is a Correctness Showstopper]] — related guide
- [[gallery-vs-production-app-distinction|Gallery App Implementations Do Not Satisfy Production Backlog Items]] — related guide
- [[bespoke-pull-symbol-cleanup-workflow|Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow]] — related guide
- [[bespoke-pull-symbol-cleanup-workflow|Bespoke Pull-Symbol Cleanup — Four-Phase Fan-Out Workflow]] — related guide
- [[v-107-bespoke-snapshot-consumer-migration|V-107 — Live Bespoke Snapshot Consumer Migration to Canonical Seam]] — related guide

- [allow-deprecated-scaffolding-tracks-migration-window](allow-deprecated-scaffolding-tracks-migration-window)
- [android-stale-render-model-pre-v80](android-stale-render-model-pre-v80)
- [nfct-native-decoder-not-ffi](nfct-native-decoder-not-ffi)
