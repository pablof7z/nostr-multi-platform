---
title: Chirp Cross-Platform Business Logic & Rust Side
slug: chirp-cross-platform-business-logic
summary: Chirp must achieve consistency across all platforms (iOS, TUI, Android, desktop) by implementing business logic in the shared Rust side.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# Chirp Cross-Platform Business Logic & Rust Side

## Cross-Platform Business Logic

Chirp must achieve consistency across all platforms (iOS, TUI, Android, desktop) by implementing business logic in the shared Rust side. [^f3d8d-4]


## Parallel Dispatch

The Android dispatch door (B2) must run in parallel with A1 (typed action facade). [^f3d8d-5]

## Shell Boilerplate Deduplication

Shared runtime/session boilerplate duplicated across shells must be addressed — a typed action API alone does not fix duplicated boot/register/start/drop/update-bridge boilerplate. [^f3d8d-6]

## Keyring Capability Parity

TUI installs a keyring capability; desktop and Android must also install it as a prerequisite for account persistence and write parity. [^f3d8d-7]

## Acceptance Test Checkpoints

Acceptance tests must reference the byte-identical checkpoint snapshot gates defined in `docs/plan/m15-cross-platform.md:32+37` per rung. [^f3d8d-8]

## iOS Relay Defaults

iOS relay defaults must flow from the Rust kernel with no Swift hardcodes. [^f3d8d-9]
## See Also

