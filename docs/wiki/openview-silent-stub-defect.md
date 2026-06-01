---
title: OpenView Silent Stub Defect (V-110)
slug: openview-silent-stub-defect
summary: The OpenView silent-stub defect (silently no-ops instead of failing loudly) is filed separately as V-110, not bundled with the Marmot fix.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-31
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
  - session:34d8cff3-a7d4-4b49-a912-d2f465f53a29
---

# OpenView Silent Stub Defect (V-110)

## Defect Tracking

The OpenView silent-stub defect (silently no-ops instead of failing loudly) is filed separately as V-110, not bundled with the Marmot fix. OpenView must fail loudly rather than silently no-op (V-110). The key-package fetch mis-wire that caused OpenView to silently no-op is fixed by routing through push_interest.

<!-- citations: [^4edd4-29] [^34d8c-5] -->
