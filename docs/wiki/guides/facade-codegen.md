---
title: "Facade Codegen: Registry Fields and Accessor Shapes"
slug: facade-codegen
topic: uniffi-migration
summary: The codegen `FacadeRow` registry carries a `rust_module` field with a serde default of `"facade"`, so existing registries that omit the field produce byte-ident
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
---

# Facade Codegen: Registry Fields and Accessor Shapes

## Configurable Facade Module Path

The codegen `FacadeRow` registry carries a `rust_module` field with a serde default of `"facade"`, so existing registries that omit the field produce byte-identical output. `validate_module_path` accepts `::`-separated lower_ident segments, allowing nested module paths, and rejects uppercase values. `render_imports` emits `use crate::{registry.facade.rust_module}::{rust_type};` instead of the previously hardcoded `crate::facade::` path. This resolves issue #3004 via PR #3011.

Because the default is `"facade"`, any registry without an explicit `rust_module` value generates output identical to the pre-#3011 hardcoded path.

<!-- citations: [^d8bc6-0084a] [^d8bc6-f95c4] [^d8bc6-ef983] [^d8bc6-3b91c] -->
## Runtime Accessor Shape

A `facade.runtime_accessor_shape` registry field controls how generated facades invoke their accessors at runtime, accepting either `"ref"` (the default) or `"closure"`. This mode is introduced by PR #3013 to address issue #3005, which requested a closure/guarded-accessor mode for Android facades.

In ref mode the generator emits `self.<accessor>()`; in closure mode it emits `self.<accessor>(|app| <concept_fn>(app, ...))`, mapping an accessor value of `None` (a dead or inert handle) to `OpenFailed` on open and `false` on close, preserving idempotent close behavior (D6). The generator remains concept-crate-dependency-free, stamping `app` as the door's first argument symmetric to ref mode.

Ref-mode output is byte-identical to the output produced before the closure-mode addition, so no existing iOS facade drifts when the new field is introduced.

<!-- citations: [^d8bc6-a9232] [^d8bc6-3d86b] [^d8bc6-f72cf] [^d8bc6-e3831] [^d8bc6-3d404] [^d8bc6-18d86] -->
## Codegen Overview

The concept-read codegen exists to eliminate hand-patching by generating platform facades from a registry. FlatBuffers regeneration is gated through `ci/regenerate-flatbuffers.sh`, not raw `flatc`, per repo rule. <!-- [^d8bc6-1f75c] -->
