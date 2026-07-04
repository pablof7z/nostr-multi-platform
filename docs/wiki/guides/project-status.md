---
title: "NMP Project Status: NIP Scope and ADR Spine"
slug: project-status
topic: project-status
summary: The v1 public name for the project is kept as NMP
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-29
updated: 2026-07-04
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
  - session:3c942260-311d-4e00-8bcc-204045ea87b3
  - session:91a86fdf-624c-446e-9b38-0fb02085121f
  - session:5ad70acc-1442-4343-92a7-f79b2fc59071
  - session:04745411-a0c1-4523-ac83-71dc983f410b
  - session:1c293d33-5ec2-4689-b6c2-cd159d8b6bb7
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
---

# NMP Project Status: NIP Scope and ADR Spine

## Governing Epic

The v1 public name for the project is kept as NMP. The npm org scope for the project is `@nmpis`; the `@nmp → @nmpis` scope rename is applied across the release manifest, all four web packages (runtime-web, components-web, gallery-web, registry-web), the regenerated lockfile, doctrine-lint's boundary-token gate, nmp-codegen doc-comments, the CI typecheck workflow, vercel.json, and docs. The `@nmp` npm org scope-squatting risk is flagged in issue #2690 as a live risk until the org is claimed.

EPIC-NS-001 (#2340) is the governing p0 north-star epic for the clean-break NMP app architecture migration; all active slices trace back to it. The migration eliminates `register_defaults()`, raw `open_interest`, old projection tiers, and per-feature native ABI choices, replacing them with typed read sessions, a composable write door, an explicit composition root, and one UniFFI public surface for native. EPIC-NS-001 is approximately 70% complete, not ~100% as previously reported; migrate-readiness is approximately 60–65%.

The migration-readiness gate list before existing apps can move over is: product-read cutover (#2399, #2418), DX clean-room proof (#2256), and app-defined kinds first-class (#2408/#2413/#2414). App migration can begin before the entire EPIC-NS-001 epic closes; the M14 C-ABI deletion and perf-signal decision are epic-close cleanup that do not block the migrate-readiness gate (#2256).

<!-- citations: [^898a4-289cb] [^898a4-b541d] [^898a4-9a8e2] [^04745-19c9f] -->
## ADR Spine

The clean-break refactor is governed by ADR spine 0069–0073, applied to native,
browser, and starter targets in lockstep, with doctrine-lint and doc ratchets
locking each slice shut behind it. The ADR directory is current-only: obsolete
decision files are deleted after surviving rules move to their current owners.

<!-- citations: [^898a4-eaad2] [^3c942-d9519] [^898a4-bc2c6] -->
## Protocol Scope: NIP Status

NIP-57/zaps and NIP-47/NWC are formally post-v1; NIP-96 is never. Zap semantics are removed from the nmp-relations classifier.

Issue #1001 is closed in favor of epic #2864 and serves only as historical context. Epic #2864 is the tracking epic for NIP-60/61 wallet work and is labeled phase:post-v1 — the wallet is a feature app on top of the framework, not a v1 framework surface. Phase 0 is delivered — nmp-nip60 was reactivated and PR #2866 merged — and Phase 1 is the `nmp-wallet` composition crate, which remains out of scope for Phase 0 and is not started during Phase 0. The nips.md matrix note for NIP-60/61 was flipped from "Requires product/architecture decision before activation" to "Requires activation work before any support claim," erasing the deferral marker (PR #2854). The nips.md NIP-60/61 row no longer says nmp-wallet is "not yet added" or that #2870 is open — both are resolved by #2876 and #2874. A one-line addition to docs/nips.md links the new builder guide from the NIP-60/61 support-matrix row. The `nwc.*` names (nmp.wallet.nwc.connect/nwc.disconnect) from the design doc's Product Surface list were not renamed because nothing implements that name today — nmp-nip47 is the only real backend under the current names; renaming requires moving nmp-nip47's ActionModule + wire-schema registration, which is epic #2864 Phase 2 (NWC consolidation). Issue #2880 was filed to support NIP-87 (Ecash Mint Discoverability) — kind:38172 announcements / kind:38000 recommendations, scoped post-v1 as a future enhancement.

Issue #2882 was filed to release-classify nmp-wallet ([[private_packages]]) and nmp-nip60 (not a [[public_crates]] entry) so external consumers can git-rev pin them — this is not covered by #2864 and is the single most important blocker to a green end-to-end nutsack run. The natural gate for flipping #2882's release classification is once Wave C (W4–W7) gives nmp-wallet a real action/projection surface.

Issue #2872 covers NIP-60/61 builder documentation and is safe to start before Phase 1 lands.

Issue #2927 (NIP-AD) is closed as delivered — the full crate `crates/nmp-nip-ad` exists on master with wired components. Issue #2927's rich-render follow-up tails (#2979 and #2981) are tracked as post-v1.

<!-- citations: [^898a4-bfe17] [^91a86-bf80b] [^5ad70-d096f] [^5ad70-3a9ef] [^91a86-2db86] [^91a86-e34f7] [^1c293-74c5b] [^1c293-cc80c] [^d8bc6-3dc6c] -->
## Active Pre-V1 Workstreams

The profile-claim loop stash (#2298) is active pre-v1 correctness work, not post-v1 deferral. <!-- [^898a4-f7531] -->

NIP-29 owns only the h-tag routing concern, not kinds; the kinds filter was deliberately removed and GroupEventsProjection reads consumer-declared kinds. <!-- [^898a4-4d4bb] -->

The `nmp-native-runtime` extraction is incomplete and retains dual-ownership, so drift becomes a recurring tax when master moves fast and extractions leave the old path behind. <!-- [^3c942-83d7b] -->

Issue #2993 (split NIP-55 onboarding out of signer_state) is deferred to post-v1 — it is a real FlatBuffers wire change with zero user-visible gain near 1.0, and has a reconciliation prerequisite with existing `bunker_handshake` and `nip46_onboarding` projections that risks a duplicate-path violation.

Issues #2918 and #2981 are consumer-repo work explicitly designated to be implemented in Chirp, not NMP.

<!-- citations: [^898a4-f7531] [^898a4-4d4bb] [^3c942-83d7b] [^d8bc6-709aa] -->
## Post-V1 Sequencing

The group-chat epic (#2695) is sequenced for post-v1 first minor, not as a v1 blocker. Issues #2864 (wallet), #2858 (X-Ray), and #2974 (Marmot MLS) are labeled phase:post-v1 and placed in Backlog — per docs/nips.md these are feature-app / dev-tool / non-v1-claimed surfaces, not framework blockers. Issue #2974 (Marmot MLS keyring CredentialStore never wired) is a genuine correctness defect — set_default_store has zero matches in the codebase, so Marmot identity registration is permanently false on every platform — but it stays open as post-v1 because MLS groups are not a v1-claimed feature. Issues #2995 and #2979 are deferred wallet/NIP-AD follow-ups on the Backlog.

v1 is defined as the owner-gated publish act: name → rc rehearsal → 1.0.0 tag → crates.io/npm → external-consumption proof, gated only on the owner's go, not on any pending code work. docs/nips.md is the v1 pre-release truth source that classifies which NIPs are in-scope vs post-v1. Issue #2690 is the v1 release train epic that defines what pre-v1 means and gates the v1 publish act. <!-- [^d8bc6-0841c] -->

<!-- citations: [^d8bc6-0841c] [^04745-5f4f8] [^d8bc6-caec7] [^d8bc6-4649c] -->
