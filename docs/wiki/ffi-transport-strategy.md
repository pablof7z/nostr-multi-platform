---
title: FFI Transport Strategy & Migration Plan
slug: ffi-transport-strategy
summary: The transport strategy must define a concrete migration and prototyping order with identified risks and decision gates.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-26
updated: 2026-05-29
verified: 2026-05-26
compiled-from: conversation
sources:
  - session:53ffd7cf-32db-4347-b6ec-e2d6244f8e58
  - session:8a8aefe0-93b0-405d-99da-0c8ac39114c8
  - session:e4861768-9a00-4d83-b7a3-a39d07749d1c
  - session:cd331450-f93f-48d0-960e-3c73e927775e
---

# FFI Transport Strategy & Migration Plan

## Migration and Prototyping Order

V-84 and V-85 are reclassified as HIGH priority v1 items to complete a half-landed migration, rather than post-v1 rollout tail polish. The transport strategy must define a concrete migration and prototyping order with identified risks and decision gates. PR 582 is a standalone transport-switch only, not a combined typed-tables migration. The UniFFI sweep ships independently before FlatBuffers rollout to avoid combining two migrations at once. Projections must be stable for at least 30 days before being migrated to FlatBuffers to mitigate schema churn risk. Transport performance metrics (payload_bytes, decode_us, apply_us, make_update_us, serialize_us) are established as p50/p95/p99 under realistic load across Android and WASM before making format changes. A snapshot-delta protocol on top of current JSON is evaluated and measured before switching to a new binary format. A FlatBuffers prototype is built on a single, stable projection first, gated by achieving at least a 5x decode speedup or demonstrably simpler zero-copy access patterns. FlatBuffers is only rolled out to remaining projections and WASM consumers after the initial single-projection prototype holds up over a full week of dogfooding. Completion gates for the transport migration require p99 decode_us under 2 ms on a representative snapshot, zero new bespoke FFI symbols, one WASM consumer reading the same bytes, and a clean doctrine-lint.

<!-- citations: [^53ffd-1] [^8a8ae-1] [^e4861-1] [^cd331-4] -->
## Hot Path Format

FlatBuffers is used for the hot snapshot/projection path across Swift, Kotlin, and WASM consumers, providing zero-copy reads without a per-tick decode pass. FlatBuffers is used as the wire format for WASM snapshots, using a single schema and one set of generated readers across all platforms. Native shells render FlatBuffer data without interpreting or reasoning about its internal structure, enforced via linting. [^8a8ae-2]

## Debug and Inspection Format

JSON is retained as the debug and inspection format, kept behind a debug flag and used for ADR-fixture goldens. JSON is retired from the hot path and kept only behind a `--debug-transport=json` flag. [^8a8ae-3]

## Tooling and CI

CI must regenerate and diff generated code across four languages to manage the FlatBuffers tooling tax. [^8a8ae-4]
## See Also

