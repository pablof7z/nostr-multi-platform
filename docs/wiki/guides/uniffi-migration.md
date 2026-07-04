---
title: M14 UniFFI Native Surface Migration
slug: uniffi-migration
topic: uniffi-migration
summary: "The M14 epic (#2125) collapses the native public binding surface to UniFFI: one public UniFFI surface serves iOS and Android, with FlatBuffers `Vec<u8>` bytes r"
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
  - session:f308bb0b-7b74-4684-9a5b-1fce8ffcab35
  - session:d8bc6df1-32a3-48e1-8db6-3dbff7c4c0e5
---

# M14 UniFFI Native Surface Migration

## Objective

The M14 epic (#2125) collapses the native public binding surface to UniFFI: one public UniFFI surface serves iOS and Android, with FlatBuffers `Vec<u8>` bytes remaining as the payload encoding and wasm-bindgen staying separate for the browser. No durable `nmp_marmot_*` C ABI is permitted; Marmot must migrate to the #2125 UniFFI surface or be deleted. <!-- [^898a4-7ff7a] -->

## Benchmark Gate

Before committing to the full migration, #2125 requires a benchmark gate proving cost-per-frame for UniFFI foreign-trait push versus C callback. The M14 benchmark verdict is COLLAPSE: UniFFI's surcharged weighted-p99 delta is 1,323 ns — roughly 630× below the collapse threshold — resulting in zero internal C-ABI exceptions and 56 symbols slated for one UniFFI surface. <!-- [^898a4-280fa] -->

## Sequencing

The M14 UniFFI native surface collapse is post-v1 and should not compete with finishing the write door and DX proof for v1. <!-- [^898a4-826fb] -->

The M14 C-ABI deletion chain (D0-A → D0-B → D0-C) is a strict serial sequence that cannot be parallelized. <!-- [^898a4-0c987] -->

## FlatBuffer Payload Accessor Strategy

FlatBuffers byte-vector payloads in KernelUpdateFrameDecoder are converted to Data via the `withUnsafePointerToPayload` bulk-pointer accessor, not the generic per-element `FlatbufferVector<UInt8>` Sequence initializer. The bulk-pointer `withUnsafePointerToPayload` pattern (bulk memcpy) is used instead of `Data(typed.payload)` (per-byte generic Sequence copy) for extracting typed projection payloads. The `@available(*, unavailable)` annotation is preferred over deleting the slow accessor because it produces a minimal, greppable diff against fresh `flatc` output that a post-process script can reapply after every regeneration. The `@available(*, unavailable)` annotation is preferred over `@available(*, deprecated)` because `unavailable` produces a hard compile error while `deprecated` only produces a suppressible warning.

Every `public var <name>: FlatbufferVector<UInt8>` accessor with a sibling `withUnsafePointerToPayload` in `apps/ios/Chirp/Bridge/Generated/*.generated.swift` is annotated `@available(*, unavailable, message: "per-byte copy — use withUnsafePointerToPayload")` to make the slow per-byte accessor a compile error.

`KeyedRefCache.generated.swift` is the per-key reference-row cache (profile/event resolution, ADR-0063/#1671) that was also doing `Data(row.payload)` on every row of every incremental batch. The `nmp-codegen` crate — the Rust code-generation tool in the nostr-multi-platform monorepo that emits generated Swift bridge files including `KeyedRefCache.generated.swift` — uses the bulk-pointer `withUnsafePointerToPayload` pattern in its source template (`crates/nmp-codegen/src/swift_keyed_cache.rs`) instead of `Data(row.payload)`, so regenerated `KeyedRefCache.generated.swift` files do not reintroduce the slow accessor.

`TypedProjectionGlueEmbed.swift`'s `plainText(fromContentTree:)` accepts already-copied `Data` rather than a raw `FlatbufferVector<UInt8>`, with the bulk copy performed at each call site.

The eager copy of every typed projection's payload in every incoming kernel frame is the correct and final design — not a symptom being patched — because frame contents are already demand-shaped (unchanged rows dropped, idle ticks emit nothing, closed-view projections cleared), the copy is the mandatory host-side consume copy from the callback contract, and a lazy/zero-copy design would pin entire transport frames alive for unbounded memory.

<!-- citations: [^f308b-87970] [^f308b-a32e0] [^f308b-70160] [^f308b-2a18c] [^f308b-b1d01] [^f308b-67c78] -->
## FlatBuffer Accessor Enforcement

`apps/ios/scripts/neuter-slow-flatbuffer-accessors.sh` is an idempotent script that finds every `FlatbufferVector<UInt8>` accessor with a fast `withUnsafePointerToPayload` sibling across `Generated/*.generated.swift` and prepends the `@available(*, unavailable, message: "per-byte copy — use withUnsafePointerToPayload")` annotation. The script runs after every `flatc` invocation inside `regen-flatbuffers.sh` so regenerated files are automatically re-annotated. `regen-flatbuffers.sh` is the FlatBuffers Swift codegen entry point in chirp, created for chirp#35 because the generated files used to be hand-run `flatc`.

FlatBuffers regeneration is gated through `ci/regenerate-flatbuffers.sh`, not raw `flatc` — this is a repo rule.

`ci/check-flatbuffer-byte-vector-accessors.sh` plus a `flatbuffer-byte-vector-accessors` job in `codegen-drift.yml` fail CI if any `FlatbufferVector<UInt8>` accessor lacks the `@available` annotation, catching future hand-run flatc regens outside the regen script's coverage.

<!-- citations: [^f308b-87b2d] [^f308b-34baa] [^f308b-dde77] [^d8bc6-0d3a3] -->
