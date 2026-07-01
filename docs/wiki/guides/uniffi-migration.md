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
updated: 2026-06-29
verified: 2026-06-29
compiled-from: conversation
sources:
  - session:898a41b5-68e0-4b0f-b16c-c6072454bd6a
---

# M14 UniFFI Native Surface Migration

## Objective

The M14 epic (#2125) collapses the native public binding surface to UniFFI: one public UniFFI surface serves iOS and Android, with FlatBuffers `Vec<u8>` bytes remaining as the payload encoding and wasm-bindgen staying separate for the browser. No durable `nmp_marmot_*` C ABI is permitted; Marmot must migrate to the #2125 UniFFI surface or be deleted. <!-- [^898a4-7ff7a] -->

## Benchmark Gate

Before committing to the full migration, #2125 requires a benchmark gate proving cost-per-frame for UniFFI foreign-trait push versus C callback. The M14 benchmark verdict is COLLAPSE: UniFFI's surcharged weighted-p99 delta is 1,323 ns — roughly 630× below the collapse threshold — resulting in zero internal C-ABI exceptions and 56 symbols slated for one UniFFI surface. <!-- [^898a4-280fa] -->

## Sequencing

The M14 UniFFI native surface collapse is post-v1 and should not compete with finishing the write door and DX proof for v1. <!-- [^898a4-826fb] -->

The M14 C-ABI deletion chain (D0-A → D0-B → D0-C) is a strict serial sequence that cannot be parallelized. <!-- [^898a4-0c987] -->
