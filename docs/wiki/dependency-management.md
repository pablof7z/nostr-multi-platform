---
title: Dependency Management and Versioning
slug: dependency-management
topic: dependency-management
summary: nmp-feedback was bumped to nmp-v0.3.0 directly (commit a6794d6) to resolve the diamond NmpApp type conflict
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-11
updated: 2026-06-12
verified: 2026-06-11
compiled-from: conversation
sources:
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:bbd5fe79-cd71-4de0-ba9f-f3684520a03f
  - session:cf071d35-ee9b-4a1f-a3b8-885c651e8cce
  - session:f1b740a8-d601-4b63-8633-072c83a6de22
  - session:65edf39e-4cfd-4bfc-9b65-ec4dc1944b1e
  - session:954c56b2-d292-4021-8b55-977d3fd8df4d
---

# Dependency Management and Versioning

## Dependency Management

nmp-feedback was bumped to v0.3.0-tracking rev directly (matching that repo's direct-commit convention) to resolve the diamond NmpApp type conflict that would have occurred if podcast-player bumped NMP independently. podcast-player is migrated to nmp-v0.3.0 using pure git-rev pins (no `[patch]` block, no path deps) with a lockstep rule: bump nmp-feedback to the same NMP rev whenever NMP is bumped, ensuring a single NmpApp type, typed SnapshotEnvelope decode, and ChangeGate on the episode-library projection. The podcast-player migration has a signed_events typed sidecar bridge backlogged as P2 because the push path does not yet decode the nmp.signedEvents sidecar. nmp-v0.3.0 was tagged and released with a full CHANGELOG and migration guide covering the payload:Value removal, SnapshotEnvelope API, ChangeGate pattern, and GC health signals. The release following v0.3.0 is versioned v0.4.0 (not v0.3.1) because the C-ABI symbol removals are breaking changes requiring a minor version bump. (Previously: nmp-v0.3.0 was tagged and released.) nmp-v0.4.0 is tagged at commit 535f6f99. Android consumers must skip v0.3.0 entirely and pin v0.4.0 directly because v0.3.0 shipped with Android completely dark. podcast-player PR #382 is a clean pin-bump to v0.4.0 with single-rev Cargo.lock resolution (27 entries at 535f6f99, zero stragglers), and its Android decode path is Rust-side (already typed) with no Kotlin-side KernelUpdateFrameDecoder analogue. podcast-player PR #377 has a hold-warning because it pins v0.3.0 which has the Android-dark defect and the GC ceiling risk. hl only consumes nmp-feedback with default-features=false (protocol-only, no nmp-core) and needs no migration; it is unaffected by the v0.4.0 transport break and needs only a lockstep rev bump. win-the-day-app has no NMP dependency.

NMP should not drop rust-nostr to hand-roll crypto primitives. NIP-91 AND-matching (`&t` tag prefix style) should be checked for rust-nostr support before the M2 filter JSON contract is frozen. NMP should not switch to UniFFI/KMP because it cannot express NMP's FlatBuffers snapshot model; the existing C-ABI with typed-projection codegen is the better fit.

<!-- citations: [^954c5-2] [^da6b1-5] [^bbd5f-1] [^bbd5f-2] [^bbd5f-3] [^cf071-1] [^cf071-2] [^f1b74-1] [^65edf-1] [^da6b1-26] [^da6b1-46] [^da6b1-61] [^da6b1-73] [^da6b1-86] [^954c5-11] -->
