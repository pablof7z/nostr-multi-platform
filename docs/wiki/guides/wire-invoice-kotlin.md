---
title: Wire Invoice Kotlin
slug: wire-invoice-kotlin
topic: ffi-runtime
summary: The `WireInvoice` Kotlin type uses a flat data class with three optional fields (`bolt11`, `bolt12`, `cashu`) to match Rust's externally-tagged JSON serializati
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:53838558-81bd-433d-a46d-d117ecebb361
---

# Wire Invoice Kotlin

## Data Model

The `WireInvoice` Kotlin type uses a flat data class with three optional fields (`bolt11`, `bolt12`, `cashu`) to match Rust's externally-tagged JSON serialization, rather than a sealed class. <!-- [^45258-27] -->

The `npub` and `npubShort` fields in `ProfileWire` are always Rust-formatted (per aim.md §6.9) — Swift/Kotlin must never reformat or truncate npub strings locally. The `DEMO_NPUB_SHORT` constant must use the exact Rust `short_npub` output format (`npub1l2vyh…utajft`: 10 chars + U+2026 + 6 chars), not a Swift-style truncation, to comply with aim.md §6.9. <!-- [^53838-12] -->
