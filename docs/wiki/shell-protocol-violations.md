---
title: Shell Protocol Violations
slug: shell-protocol-violations
topic: codebase-patterns
summary: Issue #1283 is resolved with the EmbedHost resolver moving to nmp-ffi (which sits above both nmp-core and nmp-content in the DAG), shipping a typed EmbedKindPro
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-15
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
  - session:019eca68-85c6-77e0-b237-e58f6c894f72
---

# Shell Protocol Violations

## Protocol Logic in Shells (D0 Violations)

Issue #1283 is resolved with the EmbedHost resolver moving to nmp-ffi (which sits above both nmp-core and nmp-content in the DAG), shipping a typed EmbedKindProjection on a claimed_event_embeds sidecar key that all shells decode, rather than enriching the kernel's claimed_events buffer. (Previously: iOS EmbedHost reimplemented the Rust embed-projection resolver, switching on raw Nostr kind integers and parsing kind:0 JSON and NIP-23 tags in Swift while the authoritative implementation lived in nmp-content.) The kernel (nmp-core) cannot resolve embeds because nmp-content depends on nmp-core, never the reverse; the correct home for embed resolution is nmp-ffi, which may depend on nmp-content without creating a cycle. The nmp-ffi embed_sidecar module uses a one-tick-lag design for resolving claimed events (D8-compliant, D6-compliant), reading KCEV FlatBuffer on each update frame and calling nmp_content::resolve_embed_projection. nmp-core depends on nmp-content in only one direction (nmp-content → nmp-core), so the kernel structurally cannot call resolve_embed_projection; the claimed_events.fbs schema documents that the kernel-owned projection stays protocol-agnostic (opaque kind, no branching). The claimedEventEmbeds field in KernelTypes.generated.swift is NOT codegen-declared — it's an nmp-ffi runtime JSON sidecar projection — and was correctly dropped from the generated Swift code. The #1283 migration is multi-phase: Phase 0 (Rust embed_sidecar + gallery iOS resolver deletion) is on master; remaining phases (Chirp iOS typed FlatBuffer sidecar swap, Android gallery decode, external consumer git-rev bumps) are tracked by #1335. Android has not yet duplicated the EmbedHost resolver (its EmbedEntry is pre-resolved in Rust), so landing #1283 now means Android writes a decode-only path from day one and the duplication never spreads. #1283 and #920 share the same architectural pattern: resolve protocol-specific branching one layer above the kernel, ship typed results, kernel stays D0-clean. #920's naive fix (move TimelineItem to nmp-nip01) would create a cycle because nmp-nip01 → nmp-core already exists; the architecturally-right fix is the snapshot-envelope cut. iOS ThreadNoteRow re-derives isRepost from raw kind:6 integer instead of the Rust-emitted typed boolean, diverging from NoteRowView and ModularBlockView which already consume the typed field. iOS relay seeding hardcodes relay URLs and parses JSON in Swift while Android delegates to Rust via nmp_chirp_config; no nmp_app_seed_default_relays FFI symbol exists for iOS to call. Workstream E enforces that shells render typed projections only: no raw event store or history crosses FFI, no raw kind switches in the shell for protocol meaning; Rust emits typed embed keys, repost flags, relay diagnostic facts, and action-stage state.

<!-- citations: [^02745-66] [^02745-92] [^02745-107] [^02745-120] [^02745-132] [^019ec-39] -->
## Unconsumed Projections & Dead Code

Android's SnapshotProjections had five typed sidecars (signer_state, action_lifecycle, action_stages, action_results, relay_diagnostics) that had no decoder and no Kotlin FlatBuffer binding, causing Marmot dialogs to never dismiss and the signer badge to show no status. Desktop chirp-desktop renders every note card body twice per frame (a plain ui.label followed by note_body rich rendering), and effective_content is dead code because the kernel already unwraps kind:6 at projection time. Desktop drops the signer_state, bunker_handshake, and nip46_onboarding projections silently, so bunker sign-in has no UI feedback beyond a static URI. Desktop never calls nmp_app_ack_action_stage, so action_stages entries accumulate indefinitely in the kernel's projection until the 1024-entry cap silently evicts old ones. The RelayerDiagnosticsInfo table + info field was missing from the checked-in Swift binding (stale after #1195/ADR-0051), silently preventing NIP-11 relay metadata from being decoded on iOS. iOS and Android hand-write separate typed decoders with nothing forcing cross-platform parity, so any projection wired on one platform can silently never reach the other. Swift FlatBuffers decoders use getCheckedRoot (Verifier) on every decode of snapshots produced by the same in-process Rust engine microseconds earlier; the unchecked getRoot would work and eliminate per-frame verification overhead on trusted data. Workstream F brings chirp-desktop to parity with mobile on signer/action projections, action-stage acks, secret handling matching platform capability and keyring model, elimination of duplicate rendering and parsing of kernel-owned content, and silent NIP-55 restore when persisted signer state is valid.

<!-- citations: [^02745-67] [^78c8e-57] [^02745-108] [^019ec-40] -->
## Secret-Management Failures

The desktop keyring stores nsec as plaintext in a mode-0600 file (bypassing macOS Keychain) and makes three intermediate plain-heap copies (fs::read_to_string, KeyringResult, JSON envelope) that are never zeroized before drop. <!-- [^02745-68] -->

## Composition Instability & Churn

Android NostrAvatar and NostrProfileName key their DisposableEffect on profileHost, which changes identity on every snapshot tick (because rememberKernelProfileHost uses remember(model, profiles) with a fresh map), causing an infinite claim/release churn loop. The fix for the Android claim churn is to remove profileHost from the DisposableEffect key and stabilize KernelProfileHost by keying remember on model only, threading the latest profiles via rememberUpdatedState.

Spawn-at-start makes nmp_app_new return a passive handle (owned config + pre-created channel) and nmp_app_start moves config into the spawned actor, deleting the preflight kernel, the #601 rev hack, and the first-command trap, and making late config inexpressible. No LateWiring runtime diagnostic is built for #618 because spawn-at-start makes the failure inexpressible. The five wrong-shaped queued fixes (#1090, V-08 Stage 3, the publish-auth M6 plan, #618's runtime diagnostic, ADR-0036's never-built topology) are re-reviewed, re-decided, and corrected before implementation.

ProtocolCommandContext::kernel_mut() is deleted and replaced with narrow capability traits (WalletKernelAccess, ZapProfileLookup), making ambient authority over the kernel unrepresentable. DispatchHostOp is merged into the Protocol seam with whole-body catch_unwind panic isolation preserved via a narrow HostOpHandlerAccess capability. All five process-global hook/runtime singletons (ACTIVE_WALLET_RUNTIME, GLOBAL_BROKER, bunker_hook, external_signer_hook, GLOBAL_DRIVER) are replaced by per-app ports.

NMP breaking releases land behind a minor version bump (0.7.x) with a BREAKING CHANGELOG for external consumers; external git-rev-pinning consumers (podcast-player, hl) must pin across the nmp-v0.7.0 behavioral change deliberately. win-the-day as checked out locally is not an NMP consumer and the external-consumers doc must be reconciled.

<!-- citations: [^02745-69] [^2e544-438] [^2e544-439] [^2e544-456] -->
