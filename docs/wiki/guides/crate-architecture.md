---
title: Crate Architecture
slug: crate-architecture
topic: crate-architecture
summary: Relays do not generally reject large author REQ filters; no per-relay author-set sharding is needed as a base-layer fix.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-18
updated: 2026-06-26
verified: 2026-06-26
compiled-from: conversation
sources:
  - session:7c780fef-d33c-4d22-bcdb-2d9ab625a4f9
  - session:11850f79-923f-4a2a-a921-a4b9bec47c6c
  - session:019edbff-1d29-7533-99ab-0b8130b805dc
  - session:019edc05-2b24-72d3-88aa-2db67fdc57b5
  - session:019edc15-634d-7483-a42e-e9cb03e0a33e
  - session:019edc13-83b1-7143-8631-b0e695ea4afd
  - session:019edc4d-4175-7441-b5af-cb2012068335
  - session:129d2615-7195-4082-924e-9b96e3f1de8b
  - session:019edc92-b628-7ce1-be8a-c3d1013f2969
  - session:019edcba-b578-71f3-be33-f670962f11a7
  - session:e6b44a84-8cfc-48b2-863a-58382398b5df
---

# Crate Architecture

## Relay Filter Handling

Relays do not generally reject large author REQ filters; no per-relay author-set sharding is needed as a base-layer fix.

Hardcoded operator relays, seed pubkeys (including DEFAULT_FOLLOWS), and bootstrap relay URLs belong only in app-level code, not in NMP itself — including nmp-defaults and nmp-core, which must not contain operator relays, seed pubkeys, or bootstrap relay URLs. Since nmp-defaults is a reusable NMP composition library, operator-chosen relays and pubkeys must reside in leaf app crates (e.g., apps/chirp) rather than nmp-defaults.

Moving these hardcoded operator values out of NMP crates is a breaking change; in-repo consumers must be upgraded and external consumers must have migration documentation, with no compat aliases or shims.

The nmp-relay-kind sub-db key format is relay_url || 0x00 || kind(4 BE) || event_id(32), value is empty (presence-only); counts and coverage are computed via prefix scans, never via independently-mutated counters.

Kinds 4, 13, 14, 15, 1059, 1060 (NIP-04/17/59 private) are excluded from the relay-kind provenance index at write time and defensively at read time; the pre-existing nmp-relay-index is out of scope for this privacy gate.

The provenance::delete function must receive the kind parameter (available at insert/delete call sites via the event) so the relay-kind index can reconstruct exact keys for removal without an extra event load.

NMP_ADDITIONAL_DBS in open.rs must be bumped when adding sub-databases; it was 10, bumped to 11 for relay-kind.

PR #1535 (issue #1518) added the nmp-relay-kind provenance index with privacy gating, two new EventStore trait methods (relay_kind_coverage, relay_kind_count), backfill, Mem parity, and split mem/insert_kind5.rs from mem/insert.rs to stay under the 500-LOC cap.

NIP-17 named-three-levels-deep → relay_pin is NOT a defect: relay_pin is a static single-URL hard pin, while NIP-17 DmInboxRelay is a dynamic per-#p lookup that must fail-closed when unknown.

Nip17DmRelay was omitted from relay_bypasses_selection, causing DM inbox relays to be silently pruned under large follow sets; this correctness bug is fixed by PR #1532 (3 regression tests), merged to master.

DmInboxLookup on ProtocolCommandContextParts is left as-is — it is a Noop D15 capability, not a real D0 violation.

The repost triple-path in nmp-core is test-only and nmp-nip18::try_from_kernel_event is the canonical decoder already used by op_feed; no change needed.

Routing and follow list fixes must be implemented at the NMP level, not in the Chirp app layer. <!-- [^e6b44-5] -->

<!-- citations: [^129d2-82] [^129d2-83] [^129d2-84] [^129d2-85] [^129d2-86] [^7c780-1] [^11850-2] [^019ed-33] [^11850-52] [^11850-131] [^11850-183] -->
## Native Code Boundary

Native code (Swift, Kotlin, TypeScript, etc.) is allowed to do exactly three things (see aim.md §2 #4): render (translate Rust-produced state snapshots into UI); execute capabilities (call OS APIs and report raw results back to Rust, never deciding policy, retrying, or caching); and hold ephemeral presentation state — purely local throwaway state that no other platform would have to reimplement to behave correctly (spinners, scroll position, focus, input-buffer text, animation state, per-platform icon/color choices). The discriminating test: would a second platform have to reimplement this to stay correct? If yes → Rust (domain logic); if it is only how this platform shows or stages something → shell (presentation).

No native domain logic is permitted: if an if statement in Swift, Kotlin, or any native language decides what the app should do (not how it should look), that logic belongs in Rust.

All domain logic — state, business rules, derived data, routing decisions, error recovery, protocol logic — must live in Rust, not in native code.

Every external effect must be represented as typed data crossing the Rust/native boundary: Rust requests a capability, native reports a raw result, Rust decides the next state.

New nondeterministic inputs (time, randomness, network, OS callbacks, capability completions) must enter the actor as explicit actions/events or injected seams; reducers must remain replayable from message history.

Debug/history surfaces must use log-safe action tags and correlation ids; they must never record secrets, raw nsecs, plaintext DMs, or bearer tokens.

Auto-following accounts is pure app product policy, not an NMP concern; NMP provides the mechanism but must not choose who to follow. DEFAULT_FOLLOWS must be removed from nmp-core; ActorCommand::CreateAccount gains an initial_follows: Vec<String> parameter originating from the leaf app, and an empty list means no auto-follow and no initial kind:3 contacts event is published.

Rust must send raw data (pubkeys as hex, timestamps as Unix integers, display names verbatim); display helpers like short_npub/avatar_initials/format_ago_secs are legitimate only in TUI/CLI/test fixtures, never inside projection builders, snapshot types, or FFI serialization paths (aim.md §2). In-code comments citing 'doctrine §4.4' or 'V-24' to justify keeping presentation formatting in Rust are bogus; §4.4 governs outbox routing, not presentation.

Signer state labels (signer_state_label, stage_label_for) must move from Rust to the shells, removing the English strings from nmp-core projections (aim.md §2 violation).

KeyPackageStatus must emit raw fields (published/age_secs/stale/is_registered) and remove computed presentation fields (bucket_age/render_subtitle/action_label) from Rust projections; shells own the formatting.

DiscoveredGroup must emit raw fields (name/group_id/public/open/member_count) and remove display_name/initials/subtitle/finalize_display_fields from Rust projections.

Nip10ReplyAttribution and AuthorDisplay must remove redundant flat mirrors (author_display_name, author_picture_url) and the npub bech32-encoding from Rust projections; shells use the nested authorDisplay and nmp_app_encode_profile path.

The relay_diagnostics projection emits only raw tokens (role/connection/auth/state and reason kind); its `*_tone` hue selectors were removed (#1802, along with the prose/label/formatting functions short_url/short_id/role_label/format_bytes/compact_count) — shells derive their own color from the raw tokens.

P4 Finding 4 (ExternalSignerCapabilityBridge transport selection and concurrent-Intent rejection) is not a violation; transport selection is mechanical from Rust-set fields and concurrent-Intent rejection is an OS capacity constraint.

When a PR needs fixture/decoder updates after removing projection fields, all golden test fixtures and shell decoder sites (including JSON-string-literal fixtures and typed-projection glue) must be regenerated before declaring CI green.

P4's old `follow_feed_kinds` repair is historical. The current architecture is
that active-user follows are one ReducedSource/dependent-interest feed source,
with source reduction and recompilation owned by Rust rather than by native
post-identity `openTimeline` calls.

FullState/full snapshot is the correctness path; granular ViewBatch or delta variants are added only when profiling proves the snapshot path is the bottleneck and the delta is lossless. <!-- [^019ed-151] -->

<!-- citations: [^11850-28] [^019ed-2] [^019ed-34] [^11850-9] [^11850-27] [^019ed-119] [^11850-184] -->
## Crate Ownership

A feature belongs in an NMP crate (crates/) when it is a general building block that any Nostr app — or a meaningful subset of Nostr apps — could use directly (relay management, signing, NIP implementations, event storage, timeline projection, encrypted messaging, identity); the test is 'would this crate be useful to a completely different Nostr app?' App-specific logic belongs in app Rust crates (apps/<app>/).

The line between NMP crates and app crates is not protocol vs. product but generic Nostr building block vs. this app's proprietary domain; product-level features like NIP-29 group chat or Marmot MLS encrypted groups belong in NMP crates if other Nostr apps would use them.

Module organization must co-locate by owner (feature, page, view module, protocol module, or domain type), not by technical role (model/, update/, view/, state/, actions/).

The 500-LOC file-size hard cap must never be raised; files exceeding it must be split into peer modules declared in mod.rs.

Rust mod foo; inside bar.rs looks for parent/bar/foo.rs, not parent/foo.rs; peer modules must be declared in the parent mod.rs.

The 500-author cap was retired in #1497; follow feeds now use one AuthorsKind multi-author interest; all surfaces use ContactsLookup (never read follow_set directly).

Top-level actor/router must be kept flat until a screen or module has genuinely self-contained state; native/local component state must not be introduced just to avoid plumbing.

Issue #1561 search belongs in NMP with an ADR first, and should likely be split into a search primitive and kind:10007 relay-list routing.

Protocol crates must not import one another; cross-protocol composition belongs in the app crate.

<!-- citations: [^019ed-132] [^019ed-136] [^129d2-79] [^129d2-80] [^019ed-3] [^019ed-86] [^129d2-100] [^019ed-120] [^019ed-152] -->
## Replaceable & Addressable Kind Predicates

Kind integer constants and the `is_replaceable` / `is_addressable` predicates have a single canonical definition in `nmp-kinds` (Layer-0, zero dependencies); `nmp-core::kinds` re-exports via `pub use nmp_kinds::*`, and all downstream crates must use the re-export rather than re-declaring literals.

The prior local `nmp_core::kinds::is_replaceable` predicate returned `true` for kinds 0–9999 (treating regular events like kind:1, 6, 7 as replaceable), which was the opposite of `nostr::Kind::is_replaceable` and of the `nmp-store` / `nmp-nostr-lmdb` predicates — a latent correctness hazard (#1493). The consolidated definition in `nmp-kinds` and the `pub use nmp_kinds::*` re-export in `nmp-core::kinds` eliminate this divergence and the buggy local definitions.

The canonical `is_replaceable` predicate matches `nostr::Kind::is_replaceable` bit-for-bit: replaceable kinds are 0 (metadata), 3 (contacts), 41 (NIP-28 channel metadata special case), and the range 10000..20000 (exclusive). Kinds 1, 6, and 7 are NOT replaceable.

The canonical `is_addressable` predicate matches `nostr::Kind::is_addressable`: true only for 30000..40000 (exclusive). The ephemeral range 20000..30000 is NOT addressable; the prior hand-rolled `nmp-core` copy wrongly included it.

`nmp-kinds` must NOT take a dependency on the nostr crate; the predicates must be encoded directly in `nmp-kinds` with a higher-layer parity test against `nostr::Kind` if desired.

The Marmot key-package and group-message kind constants (`KIND_MARMOT_KEY_PACKAGE = 30443`, `KIND_MARMOT_KEY_PACKAGE_LEGACY = 443`, `KIND_MARMOT_GROUP_MESSAGE = 445`) are defined once in `nmp-kinds` and re-exported through `nmp-core::kinds`, eliminating the prior u16/u32 type split across `interest.rs`, `service.rs`, and `projection/state.rs` flagged in #1493.

The NIP-01 short text note constant is named `KIND_SHORT_TEXT_NOTE` (not `KIND_SHORT_NOTE`), sourced from the canonical `nmp-kinds` definition, and re-exported via `nmp-core::kinds` into `nmp-nip01::kinds`.

NIP-60/61/88 kind constants (`KIND_NIP60_WALLET`, `KIND_NIP60_TOKEN`, `KIND_NIP60_HISTORY`, `KIND_NIP60_QUOTE`, `KIND_NIP61_NUTZAP_INFO`, `KIND_NIP61_NUTZAP`, `KIND_MINT_ANNOUNCE`) are defined as `u32` in `nmp-kinds` and re-exported by `nmp-nip60::kinds` under legacy short names; `EventBuilder` call sites cast to `u16` where `nostr::Kind::from` requires it.

The `nmp-nip60` crate depends on `nmp-kinds` so it declares zero kind literals locally, eliminating the u16 local copy flagged in the #1493 fragmentation audit.

`KIND_MUTE_LIST` (kind 10000) is now in the canonical `nmp-kinds` registry and re-exported via `nmp-core::kinds`; the prior local `u32 = 10_000` literal in `nmp-wot` is replaced by the re-export.

The DM ciphertext replay fixture must verify that kind 4 and kind 14 events round-trip unmodified through the store and that is_relay_provenance_private returns true for these kinds. <!-- [^129d2-87] -->

<!-- citations: [^019ed-4] [^019ed-55] [^019ed-68] -->
## Relay Declaration Typestate Builder

DEFAULT_APP_RELAYS must be deleted from nmp-defaults and nmp-core; relay declaration becomes a typestate-enforced builder step where with_relay/with_relays advances to RelaysDeclared, and start() does not compile until the app makes an explicit relay decision. An .without_initial_relays() builder method advances to RelaysDeclared for offline/test/local apps, and runtime network operations surface NoConfiguredRelays rather than substituting a fallback relay set.

The nmp-defaults builder uses type-state (.with_relays() or .without_initial_relays()) so start() won't compile without an explicit relay decision — no silent fallback to hardcoded relays.

The nmp-cli generated app must emit an app-owned relay config stub or require the operator to provide relays at init time, and relay URLs must not live in nmp-cli's reusable NMP template code as hidden defaults.

PR #1550 (relays/pubkeys out of NMP) merges to master as the P9 headline breaking change.

<!-- citations: [^019ed-35] [^11850-30] [^11850-94] [^11850-115] [^11850-186] -->
## nmp-defaults Documentation & Migration

docs/architecture/crate-boundaries.md §9 and the builder guide must be updated so that nmp-defaults is documented as a reusable NMP composition library — NOT a leaf app — that may wire generic mechanisms but must not own operator policy facts such as relay URLs, bootstrap relay URLs, seed pubkeys, auto-follow lists, or signer permission batches.

This is a correctly breaking change with no compat aliases or shims; migration involves moving Chirp relay URLs, bootstrap relay, seed follows, and signer permissions into apps/chirp/crates/nmp-app-chirp, updating nmp-ffi/actor command surfaces to require app-provided values, and updating nmp-cli to create app-owned config placeholders.

<!-- citations: [^019ed-36] [^11850-31] [^11850-132] [^11850-187] -->
## Deferred Findings (Post-v1)

P4 Finding 5 (web client.ts keep-last-good caches re-implementing ProjectionMergeCache) and P4 Finding 6 (chirpConfig.ts relay defaults duplicating Rust) are filed as follow-up issue #1546 for post-v1 web work.

<!-- citations: [^11850-10] [^11850-29] [^11850-185] -->
## Crate Architecture

Binary feature-gating must use an inner #[cfg(feature="lmdb-backend")] fn run() with a main() that conditionally calls it, never #![cfg] at the top of a binary (which causes link errors).

The classify_kind per-NIP routing table in the router must be removed and replaced with threading EventClass through RoutingContext from the NIP-aware caller, eliminating the D0 per-kind branch.

The router lib.rs doc claiming only 2–6 of 7 lanes are implemented was a doc-lie; all 7 are implemented and the doc must be corrected.

The p9 core-config lane is granted full vertical ownership (option A) for the three coupled breaking changes (relays/pubkeys, known-signers, signer-labels), absorbing P4 F3 and F6.

Branches must not be rebased onto master while they predate the wasm-safe ExternalEventSink dispatcher split (#1572), or the wasm build will fail with an E0432 unresolved import error. <!-- [^11850-204] -->

<!-- citations: [^129d2-81] [^11850-32] [^11850-53] [^11850-133] [^11850-188] -->
