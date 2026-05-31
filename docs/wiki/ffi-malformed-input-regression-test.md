---
title: FFI Malformed Input Regression Test
slug: ffi-malformed-input-regression-test
summary: A regression test must feed malformed input through FFI to verify the replacement of lattice panics with Result returns.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-27
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
---

# FFI Malformed Input Regression Test

## Regression Test

A regression test must feed malformed input through FFI to verify the replacement of lattice panics with Result returns. V-70 tracks that hex_to_bytes32 returns all-zeros on malformed hex input, creating a silent data corruption path.

<!-- citations: [^57528-6] [^cd2b6-7] -->
## See Also

