---
title: Wire Frame Format and Schema Versioning
slug: wire-frame-format
topic: wire-frame-format
summary: "The payload:Value field is deleted from the wire frame, reducing frame size from ~14,504B to ~3,384B (a 76.7% reduction)"
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
  - session:63af4b96-d3d3-45c3-ab96-9f899beafa1b
---

# Wire Frame Format and Schema Versioning

## Wire Frame Format

The typed-projections FlatBuffers migration completed with all built-in projections encoded as typed sidecars (Tier-2 kernel-owned and Tier-1 host-registered), and PR-B deletes payload:Value from the wire frame, reducing frame size from ~14,504 bytes to ~3,384 bytes (a 76.7% reduction). (Previously: payload:Value deletion was gated on migrating six remaining generic-payload consumers and tracked as issue #1065.) ADR-0044 chose option (a): Tier-3 envelope scalar fields (rev, last_tick_ms, metrics, relay_statuses, logs, error/diagnostic fields) are promoted to first-class typed fields on SnapshotFrame, not a nmp.kernel.status sidecar projection. ADR-0044 rejected the sidecar projection because schema_version/last_tick_ms/running must be readable before any projection decode—a version discriminator inside a versioned opaque payload is circular. The KERNEL_SCHEMA_VERSION remains at 1 following the deletion because zero readers decode the removed field (all platforms are typed-first; the slot stays vtable-reserved). The SnapshotEnvelope struct and decode_snapshot_envelope/decode_snapshot_typed_projections are promoted to the public nmp-core surface for in-process Rust shells to read Tier-3 fields (rev, running, metrics, error toasts) and typed sidecars without touching the deleted payload:Value. The builder guide (docs/builder-guide/15-codegen-and-ffi.md) contains post-0.3.0 snapshot consumption rules (Rules A–E): no generic payload tree, per-platform typed sidecar decoders, decode_snapshot_envelope for Tier-3 fields, register_snapshot_projection_gated with ChangeGate, and last_gc/last_gc_at_ms GC health signals. F-05 typed coverage (#979) and F-10 transport (#991) are v1-blockers with the scope of completing 28 flatc Swift bindings, Android/Kotlin decoders first, then PR-B deleting payload:Value.

<!-- citations: [^da6b1-23] [^63af4-5] [^da6b1-38] [^da6b1-72] [^da6b1-112] -->
