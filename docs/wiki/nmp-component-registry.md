---
title: NMP Component Registry & Lock File
slug: nmp-component-registry
summary: Components installed via `nmp add component` are copied source files, not linked packages, and the lock file records upstream SHA-256 hashes so a future `nmp up
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-29
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:45258890-9aa6-4063-8df0-bdf7021e9f72
  - session:e7a1d168-3c58-4438-a544-aa645850c388
  - session:f2fd46d3-1cbd-4f80-9469-0d8137d75478
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:9a2c7cd8-95ab-4291-bbc8-6f38c5941c0a
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
---

# NMP Component Registry & Lock File

## Component Installation

nmpui.f7z.io hosts the nmp component registry (web/registry), not chirp. Components installed via `nmp add component` are copied source files, not linked packages, and the lock file records upstream SHA-256 hashes so a future `nmp update component` can compute safe diffs against local edits. `nmp add component` installs app-owned native UI source that can be edited freely, with updates performed as interactive merges against a recorded baseline, never silent overwrites. `nmp add component` accepts the flags `--path DIR`, `--registry DIR`, and `--with ROLES`. It rejects path traversal via `safe_relative()`, checks for duplicate installs, and checks for pre-existing target files before writing anything. It checks only the explicit target for duplicate rejection and filters already-installed dependencies silently instead of erroring.

nmpui.f7z.io hosts the nmp component registry (web/registry), not chirp. Components installed via `nmp add component` are copied source files, not linked packages, and the lock file records upstream SHA-256 hashes so a future `nmp update component` can compute safe diffs against local edits. `nmp add component` installs app-owned native UI source that can be edited freely, with updates performed as interactive merges against a recorded baseline, never silent overwrites. `nmp add component` accepts the flags `--path DIR`, `--registry DIR`, and `--with ROLES`. It rejects path traversal via `safe_relative()`, checks for duplicate installs, and checks for pre-existing target files before writing anything. It checks only the explicit target for duplicate rejection and filters already-installed dependencies silently instead of erroring. The first NMP tagged release is nmp-v0.1.0, using tag pattern nmp-v{version}, where tagging triggers the release-readiness workflow (manifest check + package dry-run, not a crates.io publish). [^42908-13]

<!-- citations: [^45258-22] [^e7a1d-1] [^e7a1d-2] [^e7a1d-3] [^e7a1d-4] [^f2fd4-1] [^54ae9-14] -->
## Component Update

`nmp update component` compares current file content SHA-256 to the locked upstream hash: untouched files update silently, locally edited files print `conflict: <path> — local edits preserved` and are skipped. During `nmp update component`, the component version always advances to the registry revision regardless of conflicts, and per-file `source_sha256` is the divergence signal. A missing on-disk file during `nmp update component` counts as a conflict and is not silently overwritten. [^45258-23]


Chirp iOS uses copied registry content components that have drifted from the canonical registry versions, confirmed by diff. [^e7a1d-5]
## Dependency Resolution

The component registry's dependency resolution is a recursive DFS with a `seen` set to prevent revisits, providing implicit cycle safety. [^45258-24]

## Lock File Format

The lock file `nmp.components.lock` is read with `serde`/`toml` but hand-writes the output (only `Deserialize` derived) for predictable TOML formatting. The `quote()` function in `lock.rs` only escapes `\` and `"`, sufficient for current values but not a full TOML string escaper. The file has a `schema_version` field but no schema version validation on read — it reads whatever TOML is present without checking the version field it writes. [^45258-25]

## Builtin Registry

The builtin registry is embedded at compile time via `include_str!` entries in `BUILTIN_FILES` in `registry.rs`, requiring both a `registry.toml` entry and a new `include_str!` line when adding a new registry component. [^45258-26]

## jsrepo Export

The jsrepo export command `nmp export jsrepo [--output DIR] [--registry DIR]` generates per-component JSON files at `web/registry/public/r/<slug>.json` and a main index at `web/registry/public/registry.json`, with slug conversion like `swiftui/content-core` → `swiftui-content-core`. [^45258-27]

## Drift Tests

A drift test `committed_registry_json_matches_generated_output` ensures that `web/registry/public/registry.json` matches freshly generated output; editing a registry source file requires regenerating `web/registry/public/registry.json` via `cargo run -p nmp-cli --bin nmp -- export jsrepo --registry crates/nmp-cli/registry --output web/registry/public`. A drift test `web_registry_install_metadata_mirrors_cli_manifest` compares `installId` fields in `web/registry/src/registry.ts` against entries in `registry.toml`, requiring every `registry.toml` installId to have a matching component id defined in `content/user/relay.ts` (not just `embeds.ts`), and embed entries must reuse content-kind installIds (e.g. `desktop/content-kind-30023` not `desktop/embed-article`).

<!-- citations: [^45258-28] [^6a951-12] -->
## M16 Plan Steps

The 7-step M16 plan steps are: (1) land install+lock baseline, (2) build `nmp update component`, (3) freeze ContentTreeWire fixtures, (4) replace tiny SwiftUI kit with real iOS content renderer, (5) build Android Compose parity, (6) adopt in Chirp, (7) jsrepo export only after the registry model is proven. M16 is complete when there is a manifest format, `add`/`update` with lock file, offline local registry, iOS + Android kits, fixture tests, and at least one real app consuming copied components instead of private renderers.

<!-- citations: [^45258-29] [^54ae9-16] -->
## Identicon Divergence

The registry identicon algorithms diverge across platforms: SwiftUI uses `NostrIdenticonBox` (palette + initials), Compose uses a 6-color palette-based identicon, while the gallery apps use a 5×5 symmetric block canvas algorithm. If the framework requires cross-platform pixel parity, the registry must adopt the 5×5 symmetric block canvas identicon algorithm. [^e7a1d-6]

## Platform Adoption Status

Chirp iOS does not use any registry user components, replacing them with custom inline implementations. The iOS `NoteRowView` duplicates the registry `user-card` component functionality via an inline author header composition. The main Android app uses zero registry components, relying entirely on custom monolithic replacements. The Android gallery app has full registry component adoption and better component hygiene than the main Android app; its `Identicon.kt` and `MentionChip.kt` are registry-quality and should be upstreamed to the registry or adopted by the main app. [^e7a1d-7]

## Adoption Priorities

The nmp component adoption implementation follows this order: P0 (wire `NostrProfileHost`), P1 (sync `NostrContentRenderer`), P2 (sync diverged content components, add `NostrMinimalContentView`, kind registry + embeds), P3 (extract inline components).

<!-- citations: [^e7a1d-8] [^9a2c7-15] -->
## Upstream Candidates

The iOS `NoteActionsRow` (reply/repost/like/zap buttons) is a candidate for upstreaming to the registry as a new `content-actions` component. The iOS `NoteRowView` (user-card + content-view + actions) is a candidate for upstreaming to the registry as a new `note-row` component. The Android gallery's `NmpMediaRenderer` seam is a candidate for upstreaming to the registry as a `CompositionLocal`-based media extensibility component. [^e7a1d-9]

## Reference-First API & Ownership

The M16 Component Registry product promises a reference-first API (passing `pubkey`, `nevent`, `naddr`), component-owned reactivity via Rust projections, one shell-level registry host adapter per app, and Rust owning truth while native components render snapshots without policy. NMP owns protocol and projection contracts (`ContentTreeWire`, claim/release sinks), while apps own copied source, styling, and a single shell adapter. [^54ae9-15]
## See Also

