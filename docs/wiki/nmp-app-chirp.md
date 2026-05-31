---
title: NMP App Chirp Crate
slug: nmp-app-chirp
summary: nmp-app-chirp is the per-app Rust crate that composes Nip10ModularTimelineView against the observer, following the ADR-0010 architecture where each app has its
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:423f3c56-7275-4e62-998e-e8f37be564da
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:64c4fde3-6f5e-456a-b4bb-9f17517e301c
  - session:f8543716-09b7-4884-8952-da52f571962e
  - session:485a5310-d073-41c9-b230-e6e77926a143
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:f3d8d762-5bb9-4db7-b127-667085e512bf
---

# NMP App Chirp Crate

## Architecture

nmp-app-chirp is the per-app Rust crate that composes Nip10ModularTimelineView against the observer, following the ADR-0010 architecture where each app has its own Rust crate. The crate's `marmot` feature is optional and off by default, gated as `marmot = ["dep:nmp-marmot", "nmp-marmot/ffi"]`; iOS Rust builds require `cargo build -p nmp-app-chirp --features marmot` to include the `nmp_marmot_*` FFI symbols in the static library. The `nmp_app_chirp_marmot_register_active` FFI function reads the active nsec from the NmpApp shared slot instead of requiring Swift to pass it, keeping the secret key entirely in Rust for the createAccount flow. The `ZapModule` must be registered in `apps/chirp/nmp-app-chirp/src/ffi.rs`. Chirp `lib.rs` must not re-export `nmp_app_chirp_register_group_chat` or `nmp_app_chirp_register_group_discovery` as Rust module items. The NIP-23 `ArticleDetailView` in Swift does not exist in Chirp; the Rust crate at `crates/nmp-nip23/src/view/detail.rs` is used by tests and `ArticlesDomain` and must be preserved. NMP owns session persistence abstractions; chirp-tui must not write its own config files for nsecs or relay lists. The typed nmp.feed.home closure is wired in nmp-app-chirp using encode_modular_timeline_snapshot. nmp-app-chirp integrates the typed projection closure for nmp.feed.home because it already depends on both nmp-core and nmp-feed. nmp_app_chirp_snapshot is marked as #[deprecated] diagnostics-only; actual removal is gated on all hosts adopting the NFTS typed schema. Unlike the `nmp-gallery` app, which uses `KernelEventClaimSink` via FFI for embed lifecycle, Chirp uses direct `claimVisibleNoteRelations` instead. Action envelopes must not be hand-rolled per shell; `nmp-app-chirp` must provide a `ChirpClient` typed API that wraps all action dispatch (publish, react, follow, DM, zap, accounts) with typed calls like `chirp.publish_note(content)` instead of JSON literals. Pure action envelope builders (`publish_note_action`, `react_action`, etc.) live in `nmp-app-chirp` for use by tests and shells. New bespoke FFI per verb must not be added. Snapshot structs must be public types in `nmp-app-chirp` rather than re-declared per shell, consumed directly by Rust shells and generated for FFI shells; shared snapshot types (`RelayStatus`, `ProfileCard`, `ActionResult`, etc.) are public types in `nmp-app-chirp` for all platforms to use.

<!-- citations: [^423f3-6] [^fe79b-8] [^1c093-11] [^64c4f-2] [^f8543-3] [^485a5-4] [^56db9-4] [^54ae9-11] [^f3d8d-13] -->
## See Also

