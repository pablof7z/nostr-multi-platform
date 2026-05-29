---
title: Chirp Desktop App Location (nmp-desktop Deleted)
slug: nmp-desktop-deleted-use-chirp-desktop
summary: The old crates/nmp-desktop egui shell is gone; the canonical desktop app is apps/chirp/chirp-desktop/.
tags:
  - chirp
  - desktop
  - project-structure
volatility: hot
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:d1ce3b4a-2a79-40f5-ba0e-fe608f5c7884
---

# Chirp Desktop App Location (nmp-desktop Deleted)

> The generic `crates/nmp-desktop` egui shell predated the Chirp-specific desktop app and has been permanently deleted. Any work on a Chirp desktop client must target the correct path.

## Details

- **Canonical path:** `apps/chirp/chirp-desktop/`
- **Deleted path:** `crates/nmp-desktop` — do **not** recreate this directory or any crate at this path.
- The old `nmp-desktop` was a generic egui shell with no Chirp-specific FFI wiring; it is fully superseded.
- When searching for desktop entry points, IDE indexing, or Cargo workspace members, always resolve to `apps/chirp/chirp-desktop/`.
- If a build or dependency error references `nmp-desktop`, treat it as a stale artifact (e.g., leftover `Cargo.lock` entry or workspace member line) and remove the reference rather than restoring the crate.

## See Also
- [[chirp-ffi-boot-and-callback-lifetime|Chirp FFI Boot Sequence & Callback Object Lifetimes]] — related guide
