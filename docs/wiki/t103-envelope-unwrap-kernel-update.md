---
title: T103 Envelope Unwrap and KernelUpdate Decoding
slug: t103-envelope-unwrap-kernel-update
summary: "The T103 envelope wraps all actor output as `{\\\\\\\\\\\\\\\\\\\\\\\\"t\\\\\\\\\\\\\\\\\\\\\\\\":\\\\\\\\\\\\\\\\\\\\\\\\"snapshot\\\\\\\\\\\\\\\\\\\\\\\\",\\\\\\\\\\\\\\\\\\\\\\\\"v\\\\\\\\\\\\\\\\\\\\\\\\":{...}}` or `{\\\\\\\\\\\\\\\\\\\\\\\\"t\\\\\\\\\\\\\\\\\\\\\\\\":\\\\\\\\\\\\\\\\\\\\\\\\"update\\\\\\\\\\\\\\\\\\\\\\\\",\\\\\\\\\\\\\\\\\\\\\\\\"v\\\\\\\\\\\\\\\\\\\\\\\\":{...}}`, requiring Swift bridges to unwrap the envelope before de"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-22
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:582fca30-be51-4861-bb16-3788610c6fb7
  - session:e2d58641-a6c3-4f43-94c0-b018c8fbb893
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
---

# T103 Envelope Unwrap and KernelUpdate Decoding

## Envelope Unwrapping Requirement

The T103 envelope wraps all actor output as `{"t":"snapshot","v":{...}}` or `{"t":"update","v":{...}}`, requiring Swift and Kotlin bridges to unwrap the envelope before decoding `KernelUpdate` or accessing state fields. The snapshot callback envelope wraps projections at `v["v"]["projections"]["key"]`, not at `v["projections"]` directly. Swift's `try? decoder.decode` silently returns nil on failure, which was causing `apply(update:)` to never be called when envelope unwrapping was missing.

<!-- citations: [^582fc-12] [^e2d58-13] [^64c4f-2] -->
## See Also

