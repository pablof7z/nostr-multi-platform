---
title: Test Seams and Bypass Patterns
slug: test-seams
topic: test-seams
summary: Issue #2970 (NIP-17 wss-only gate blocks `nak serve`) must NOT have its parser gate relaxed
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

# Test Seams and Bypass Patterns

## NIP-17 DM Relay Cache Test Seam

Issue #2970 (NIP-17 wss-only gate blocks `nak serve`) must NOT have its parser gate relaxed. The correct test closure is seeding the DmRelayCache directly to bypass the parser — not a `cfg(test)` attribute or a cargo feature relaxation. <!-- [^d8bc6-d9869] -->

## KernelTypes.generated.swift Drift Check Skip

The KernelTypes.generated.swift drift check script is an honest tracked-skip: it exits 0 with a notice pointing to chirp#37 and NMP#2918, while preserving the old known-drift baseline in the header comment. <!-- [^dcc80-67937] -->
