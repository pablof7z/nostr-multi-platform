---
title: V1 Release Train and Publish Gate
slug: v1-release-train
topic: project-status
summary: Issue #2690 is the v1 release-train epic
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-07-04
updated: 2026-07-04
verified: 2026-07-04
compiled-from: conversation
sources:
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
  - session:dcc80382-bcc0-45ea-8b9c-1a2fc741f872
---

# V1 Release Train and Publish Gate

## v1 Release-Train Epic (#2690)

Issue #2690 is the v1 release-train epic. It defines v1 as the owner-gated publish act: bump to 1.0.0-rc, cargo publish the `nmp-*` crates to crates.io, publish `@nmp/*` packages to npm, and prove external consumption. The v1 release is published as a release candidate (rc), not a final release. This sequence is gated on the public-name decision and the owner's go-ahead, not on any pending framework code.

The publish of 1.0.0-rc to public registries is an irreversible, owner-gated act. Agents must not perform it autonomously; it requires explicit owner go-ahead.

There is zero genuine framework code work blocking v1. The only remaining pre-v1 items are the owner-gated publish act tracked in #2690 and the upstream-blocked RUSTSEC advisory #2711 (quick-xml). The four ambiguous p2 epics — #2864 (wallet), #2858 (X-Ray), #2974 (Marmot MLS), and #2927 (NIP-AD) — are all post-v1 or already delivered.

Issue #2711 (RUSTSEC quick-xml) is blocked on an upstream wayland-scanner release that would allow quick-xml ≥ 0.41, with a deadline of 2026-09-30. It is not completable by the project itself.

Issue #1626, the prior gating tail for v1, is now closed.

Issue #2970 (NIP-17 wss-only gate blocks `nak serve`) is deferred post-v1 as a test-harness task, not a defect.

<!-- citations: [^d8bc6-ac31c] [^d8bc6-06bce] [^d8bc6-5bce1] [^d8bc6-d7bf1] [^d8bc6-97260] [^d8bc6-c4bdc] [^dcc80-7f4d9] -->
