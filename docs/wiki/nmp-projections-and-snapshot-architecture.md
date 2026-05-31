---
title: NMP Projections & Snapshot Architecture
slug: nmp-projections-and-snapshot-architecture
summary: For projections using `or_insert_with` + per-kind mutations (like `DiscoveredGroupsProjection`), new display fields must be populated in a finalize pass after a
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-19
updated: 2026-05-30
verified: 2026-05-19
compiled-from: conversation
sources:
  - session:12b3f443-3c2d-4e47-976a-7f4ceab75343
  - session:1c093fa5-0f0e-4dee-bf38-99781e763f13
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:eb342a0d-84e3-4289-9873-88a947ca8144
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:86221d39-67d3-484d-8979-b91cf75a5a72
  - session:53838558-81bd-433d-a46d-d117ecebb361
  - session:56db993b-6de7-49f9-82b1-a9416cef3294
  - session:54ae9075-be27-4b86-b69a-6955d9e79c3c
  - session:055efacc-c4f7-49a4-b5f4-644bcd80f294
  - session:47882225-939f-4978-bf5a-8feb9e5ef029
  - session:3a906f87-ee2b-4d3a-9d5f-e82ccab29349
  - session:42908d3a-983a-40e5-a8b0-917a990310e6
  - session:a09647f6-56f0-4df1-8c71-e10f20e010bb
  - session:6a951af3-7b08-4d8d-adfd-361609270d50
  - session:d0690875-a693-48ef-ac6f-31a92f5699cc
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
  - session:c9ae5a7c-0f5e-44ec-94d6-d9b5e31d8991
---

# NMP Projections & Snapshot Architecture

## Projection Finalization

For projections using `or_insert_with` + per-kind mutations (like `DiscoveredGroupsProjection`), new display fields must be populated in a finalize pass after all event kinds fold in — not at row-construction time. [^12b3f-15]

`AccountSummary` and `TimelineItem` in `KernelTypes.generated.swift` are auto-generated; adding new fields requires regenerating via `cargo run -p nmp-core --features codegen-schema --bin dump_projection_schemas | cargo run -p nmp-codegen -- gen swift --schemas - --out <path>`. [^12b3f-16]

Missing projection fields in generated Swift bindings must be fixed by adding the corresponding `SnapshotProjectionEntry` to the codegen registry, not by deleting the field from the generated struct.

D14 lint flags new `Arc<Mutex<Vec<...>>>` on `NmpApp`/`Kernel`/`Actor*` structs without a paired `SnapshotProjectionSlot`. [^1c093-22]

`KernelSnapshot` reserved-projection docs at `kernel/types.rs:654-668` are stale relative to `action_results`, `outbox_summary`, `relay_diagnostics`, and `mention_profiles`. [^1c093-23]

The `KernelSnapshot` projection must include a `profiles_by_pubkey` map so the Swift UI layer can look up and render kind:0 profiles for any referenced pubkey, not just visible timeline item authors. [^7b4ae-4]

Projection and snapshot structs must carry only raw pubkeys (hex), never pre-formatted display strings like `short_npub`, `avatar_initials`, or `avatar_color_hex`. When a kind:0 event is not known, the display name field must be `Option<String>` (absent), never a pre-formatted fallback like `short_npub`. [^175-176] [^398-400]

`MentionProfilePayload` must include a `pubkey` field so the presentation layer can recompute display attributes from it. [^214-216]

`MarmotMessageRow` must include a `sender_pubkey_hex` field because the raw hex pubkey was previously absent (only bech32 `sender_npub` existed). [^255-257]

`WalletStatus` must include a `wallet_pubkey_hex` field because `wallet_npub_short` was the only identifier crossing the wire and the raw hex was not serialized. [^240-241]

Timestamps must be sent as Unix integers, not pre-formatted display strings like `created_at_display` or `format_ago_secs`. [^227-228]

The `claimed_events` projection carries raw `created_at` as Unix seconds; the presentation layer formats it (e.g. '4d ago') per the display-separation doctrine. [^2513-2513]

Numeric fields like `member_count` and `unread_count` must be sent as raw integers, not pre-formatted display strings like `members_display` or `unread_display`. [^227-228]

Rust display helpers (`short_npub`, `avatar_color_hex`, etc.) are legitimate only in TUI render code, CLI output, and test fixtures — never inside projection builders, snapshot types, or FFI serialization paths. [^232-234]

NMP must not decide how things are rendered; it must provide the raw event id, pubkey, or other protocol data and apps determine how they are rendered. [^391-392] [^86221-9]

The typed feed schema must be a parity schema for the existing `ModularTimelineSnapshot` rather than a simplified feed model. (Previously: a parity schema for the existing `ModularTimelineSnapshot`.) PR #747 replaces the `ModularTimelineSnapshot` producer with `RootFeedSnapshot` and removes `register_typed_snapshot_projection` entirely, leaving no typed-FB encoder for the new shape; a typed `RootFeedSnapshot` schema encoder is deferred to a follow-up task. [^56db9-11]

The projection key (e.g., `nmp.feed.home`) and the `schema_id` (e.g., `nmp.nip01.timeline`) are orthogonal design elements: the projection key routes the payload, while the `schema_id` tells hosts how to decode it. [^56db9-12]

`register_typed_snapshot_projection` is exposed on `NmpApp` in `nmp-ffi` to register typed projection closures. [^56db9-13]

Runtime measurements for typed projections are captured via `typed_encode_us` and `typed_payload_bytes` metrics. [^56db9-14]

Apps can register custom snapshot projections via `nmp_app_register_snapshot_projection`, which must be cheap and non-blocking (doctrine D8) because they execute on the kernel actor thread during each tick. [^47-47]

The Core Snapshot Registry allows host apps to inject custom JSON keys into the kernel snapshot without modifying the sealed social wire schema, executing registered projection closures safely inside every actor tick. [^48-51] [^304-315]

Snapshot projection closures must be cheap and non-blocking (D8), and panics are caught and swallowed so a bad host projection cannot kill the kernel. [^128-131] [^314-314]

nmp-core's `encode_value` sorts all object keys alphabetically when encoding generic snapshot Values into FlatBuffers. [^230-236]

The `home_feed` Value in the typed NOFS sidecar path remains in struct-field order (not alphabetically sorted). [^246-247]

Parity-by-construction between typed and generic encoding paths guarantees only semantic identity, not identical key order. [^254-255]

<!-- citations: [^12b3f-15] [^12b3f-16] [^1c093-22] [^1c093-23] [^7b4ae-4] [^175-176] [^398-400] [^214-216] [^255-257] [^240-241] [^227-228] [^2513-2513] [^232-234] [^391-392] [^86221-9] [^56db9-11] [^56db9-12] [^56db9-13] [^56db9-14] [^47-47] [^48-51] [^304-315] [^128-131] [^314-314] [^230-236] [^246-247] [^254-255] [^222-224] [^226-227] [^eb342-10] [^86221-7] [^54ae9-21] [^055ef-1] [^47882-3] [^3a906-5] [^6a951-13] [^d0690-5] -->
## Snapshot Broadcast Model

Projections must be published one way: by registering them through the `register_snapshot_projection` seam, which appends them to `KernelSnapshot::projections` on every tick and rides the reactive push frame.

The generic baseline emission from the projection registry is the mandatory primary transport for all projections; typed sidecars (ADR-0037) are NMP-internal per-key optimizations added by coordinated cross-host migration, not an app-facing encoding choice.

Bespoke per-app `nmp_app_*_snapshot` pull accessor symbols are the deprecated anti-pattern (ADR-0025/ADR-0037) that forces apps into polling and must be deleted, not left `#[deprecated]`. The ADR-0025 §25 migration guide must use snapshot projection terminology instead of ViewModule references.

ADR-0039 ratifies the push projection seam as the single canonical projection path, rejects the generic pull accessor as a polling anti-pattern, obviates V-37, and rescinds the ADR-0025 Step-12 read-leg sanction.

The `store_open_failure` field is set at kernel construction with `changed_since_emit: true`, so it is designed to ride the first post-Start reactive push frame; failure to receive it at bare launch indicates an app-shell listener subscribe-timing or rev-guard bug, not a missing NMP capability.

The snapshot fan-broadcast model (14 projections emitted at configurable Hz regardless of open views) is the architectural bet that should be reversed — aim.md §7 Q1+Q2 lists this as unresolved. V-46 confirms this violation persists: the projections file now has 20 unconditional keys (was 17+), 3 action keys are null-gated, and the cluster remains unbounded. D5 snapshot bounding gates projection keys on whether a view is open (follow_feed_kinds is non-empty). The c13 contract test must open a timeline view (set follow_feed_kinds) before expecting projections.timeline to contain events, consistent with the D5-honest model.

PR #873 fixes S2 (snapshot scaling) by making `estimated_store_bytes` O(1) via a memoized `Cell<Option<usize>>` invalidated at all 5 store-mutation sites.

SnapshotFrame contains a generic payload field (`payload: Value`) as the main payload and a `typed_projections` sidecar field (`[TypedProjection]`) for typed schemas. Two typed projection schemas (NOFS and NFTS) are deployed as sidecars covering feed views; typed projections are registered via `nmp-app-template/src/op_feed_defaults.rs`. The generic Value tree payload is worse than JSON in size and speed (873 KB vs 480 KB, 42ms vs 18ms) because it uses a recursive table tree without compact typed schemas.

The F-10 acceptance gap includes the lack of a typed FullState root table, the lack of a ViewBatch delta schema, incomplete isolation of legacy JSON callback code, and typed sidecar coverage limited to feed views.

Profile data arrives via the push callback (`nmp_app_set_update_callback`), not via `nmp_app_gallery_snapshot` which only returns a minimal status envelope. The snapshot JSON from the kernel is wrapped as `{"t":"snapshot","v":<KernelSnapshot>}`; profile data surfaces at `projections.author_view.profile` when `nmp_app_open_author` is used.

Projection keys use the `nmp.*` prefix rather than bare `nip17.*` or `nip29.*` prefixes.

PR-B schema version is intentionally not bumped because `SNAPSHOT_SCHEMA_VERSION` is for snapshot-field changes and the `nip29.*` → `nmp.nip29.*` rename is on the dispatch input wire.

docs/aim.md §2 must prescribe that the backend sends raw data only and presentation layers own all formatting decisions.

Reactivity architecture key ADRs mandate a composite-keyed reverse index (ADR-0001), per-view delta budget ≤60Hz (ADR-0002), working-set memory budget ≤100MB/10k events (ADR-0003), and zero-allocation steady-state target (ADR-0004).

iOS `TypedHomeFeedDecoder` gracefully falls back to JSON when a typed sidecar is missing, so removing the old sidecar code does not cause a crash.

The merge conflict resolution for PR #747 drops the old typed sidecar block in `register.rs` (since it references a removed `projection`) and keeps both the new doc comment and the `#[deprecated]` attribute in `snapshot.rs`.

<!-- citations: [^12b3f-17] [^1c093-24] [^222-226] [^355-360] [^54ae9-22] [^055ef-2] [^055ef-3] [^47203-6] [^86221-8] [^53838-8] [^42908-21] [^a0964-2] [^d0690-6] [^c3f75-12] [^c9ae5-7] -->
## Second-App Framework Thesis (PD-033-A)

PD-033-A (the second-app proof of the framework thesis) is buildable today with zero new affordances: the existing push projection seam already provides kernel-owned projections, handshake-gated sign-in, and D3 outbox routing read off the pushed frame. The thesis is unblocked but not yet demonstrated — no second app has been built against the seam. The podcast player is the live candidate. See ADR-0039. [^12b3f-19]

NIP-29 group discovery/join UI requires an `nmp.nip29.discover` action and projection. [^1c093-25]

<!-- citations: [^12b3f-18] [^12b3f-19] [^12b3f-20] [^1c093-25] [^d0690-7] -->
## See Also

