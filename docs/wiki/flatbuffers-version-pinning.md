---
title: FlatBuffers Intentional Cross-Platform Version Pinning
slug: flatbuffers-version-pinning
summary: "CI enforces intentionally asymmetric FlatBuffers runtime version pins across platforms: Rust/Swift uses 25.12.19, Web/TypeScript uses 25.9.23, and Android/Kotli"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:37e351ee-aa2b-43eb-9793-482de338f883
  - session:485a5310-d073-41c9-b230-e6e77926a143
  - session:cd331450-f93f-48d0-960e-3c73e927775e
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:a09647f6-56f0-4df1-8c71-e10f20e010bb
---

# FlatBuffers Intentional Cross-Platform Version Pinning

## FlatBuffers Version Pinning

CI enforces intentionally asymmetric FlatBuffers runtime version pins across platforms: Rust/Swift uses 25.12.19, Web/TypeScript uses 25.9.23, and Android/Kotlin uses 25.2.10. The `KERNEL_SCHEMA_VERSION` is 1 in both Rust and Swift, and a mismatch causes the snapshot to be rejected. The CI version-pin check script (ci/check-flatbuffers-version-pins.sh) must cover all files under android/app/src/main/java/nmp/*. Generated FlatBuffers files in the nmp/ directory use the FLATBUFFERS_25_2_10() version guard to match the flatc/runtime version. Typed FlatBuffers projections are live and consumed on iOS, Android, and TUI platforms; JSON Value tree remains the mandatory primary format and there is no FullState/ViewBatch typed root.

<!-- citations: [^37e35-2] [^485a5-3] [^cd331-5] [^42908-6] [^a0964-1] -->
## See Also

