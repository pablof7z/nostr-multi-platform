//! V6 Stage 2 — `SnapshotProjections` dotted-projection-key registry.
//!
//! This module owns the single source of truth that replaces the hand-written
//! `SnapshotProjections` struct + `CodingKeys` enum at the bottom of
//! `apps/chirp/ios/Chirp/Bridge/KernelBridge.swift`. The renderer in
//! [`crate::swift`] reads this slice and emits the equivalent Swift.
//!
//! ## Why the registry lives in `nmp-codegen`, not `nmp-core`
//!
//! The Stage 2 registry is a list of `(json_key, swift_field, swift_type)`
//! triples — there is no Rust type to reflect via `schemars` (unlike Stage 1).
//! The natural home would have been `nmp-core::codegen_schema` alongside
//! Stage 1, BUT the registry MUST name dotted host-registered keys like
//! `"nmp.nip29.group_events"`, `"nmp.nip17.dm_inbox"`.
//! Those substrings would trip D0 doctrine-lint (`nip29` / `nip17` / `nip57`
//! tokens forbidden in `nmp-core` per `crates/nmp-testing/bin/doctrine-lint/
//! rules/d0.rs`). The substrings are legitimate here because *they are the
//! actual JSON wire keys the iOS shell consumes* — they are not Rust nouns
//! inlined into the kernel.
//!
//! `nmp-codegen` is exempt from D0 (it is a host-side tool crate, not the
//! kernel substrate), so the registry compiles cleanly here. The schema dump
//! binary in `nmp-core` already stays D0-clean — Stage 1 ships `Metrics` /
//! `RelayStatus` etc. by their Rust type names alone.
//!
//! ## What is *not* in this registry
//!
//! - The per-projection-value types themselves (`WalletStatusData`,
//!   `BunkerHandshake`, `PublishQueueEntry`, etc.). Those remain hand-written
//!   in `KernelBridge.swift` and are Stage 3 work. The generated
//!   `SnapshotProjections` only references them by their Swift type name —
//!   the reader must declare them somewhere reachable in the same module.
//! - The decoder configuration. The iOS shell's `KernelHandle.decode`
//!   continues to set `JSONDecoder.keyDecodingStrategy = .convertFromSnakeCase`
//!   — every `CodingKeys` raw value in the rendered enum is therefore the
//!   *post-transform* key (see `post_convert_from_snake_case` in
//!   [`crate::swift`]).
//!
//! ## Maintenance contract
//!
//! When a new snapshot projection is registered in Rust:
//!
//! 1. Add a new [`SnapshotProjectionEntry`] to [`SNAPSHOT_PROJECTIONS`] with
//!    the kernel-emitted JSON key, the Swift property name, and the Swift
//!    value type.
//! 2. Run `cargo run -p nmp-core --features codegen-schema --bin
//!    dump_projection_schemas | cargo run -p nmp-codegen -- gen swift` to
//!    regenerate `KernelTypes.generated.swift`. The CI gate
//!    (`.github/workflows/codegen-drift.yml`) fails any PR that forgets.
//! 3. If the new key's *value* type is not already declared in
//!    `KernelBridge.swift` (or in a previous Stage of the generator), add
//!    the Swift `Decodable` mirror there too — that work is Stage 3.

// ADR-0063 (#1671): the KEYED reference registry (`KeyedProjectionEntry` +
// `KEYED_PROJECTIONS`) and its Lane-C typed ROW-PAYLOAD descriptors live in a
// sibling module so this file stays under its 500-LOC cap. Re-exported so the
// existing `swift_projections_registry::{KeyedProjectionEntry, KEYED_PROJECTIONS}`
// import paths (the keyed-cache generators) are unchanged.
#[path = "keyed_projection_row_payload.rs"]
mod keyed_projection_row_payload;
pub use keyed_projection_row_payload::{
    KeyedProjectionEntry, KotlinRefRowPayload, RefRowPayload, KEYED_PROJECTIONS,
};

/// One entry in the dotted-projection-key registry.
///
/// The hand-written `SnapshotProjections` declaration in
/// `apps/chirp/ios/Chirp/Bridge/KernelBridge.swift` is the byte-for-byte target
/// the renderer must reproduce. Every field on that struct corresponds to
/// exactly one entry here, in declaration order.
pub struct SnapshotProjectionEntry {
    /// The projection's identity — the kernel-emitted JSON key as it appears in
    /// the `projections` map AND the `TypedProjection.key` the producer
    /// publishes for the typed sidecar (they are the same string for every
    /// projection; the deliberate split the op-feed / follow-list carry is
    /// `key` vs `schema_id`, both owned by the contract).
    ///
    /// #1723 (epic #1719): this is the SINGLE source of the projection's
    /// identity in this registry. It used to be spelled twice — once as
    /// `json_key` here and once as `TypedSidecar::key` — and both had to equal
    /// the [`crate::projection_contract::PROJECTION_CONTRACT`] row's `key`. The
    /// two duplicate spellings collapsed onto this one field, which is itself
    /// looked up against the contract by a fail-closed gate
    /// ([`crate::projection_contract::tests`]) so the registry's projection set
    /// can never drift from the contract's.
    ///
    /// Used to compute the `CodingKeys` raw value via Apple's
    /// `.convertFromSnakeCase` transform (split on `_` only — `.` is opaque).
    ///
    /// Examples:
    /// - `"wallet"` → no transform needed, post-transform is `"wallet"`.
    /// - `"action_stages"` → post-transform is `"actionStages"`.
    /// - `"nmp.nip29.group_events"` → post-transform is `"nmp.nip29.groupEvents"`
    ///   (the `.`-segments stay intact, only `group_events` camelises).
    pub key: &'static str,
    /// Swift property name on `SnapshotProjections`. Always lowerCamelCase.
    /// The renderer emits `let <swift_field>: <swift_type>?` on the struct
    /// and `case <swift_field>` (or `case <swift_field> = "<raw>"`) on the
    /// `CodingKeys` enum.
    pub swift_field: &'static str,
    /// Swift value type (without the trailing `?`). Every member of
    /// `SnapshotProjections` is Optional — the kernel omits keys when the
    /// projection is empty / not yet populated, and D1 forward-compat
    /// requires the shell tolerate that.
    ///
    /// Plain types pass through verbatim: `"WalletStatusData"`,
    /// `"GroupEventsSnapshot"`. Container types are written in their full
    /// Swift form: `"[PublishQueueEntry]"`, `"[String: [ActionStageEntry]]"`,
    /// `"[String: ProfileCard]"`, `"[String]"`. The renderer never
    /// composes these — what you write here is what appears on the line.
    pub swift_type: &'static str,

    /// Typed-FlatBuffer-sidecar identity for this projection, or `None` when
    /// the kernel does NOT emit a typed sidecar for this key (the JSON
    /// `payload` path is the only wire form).
    ///
    /// This is the V6 Stage 4 (consumer-side) addition: every projection now
    /// ships a typed FlatBuffer entry in the `SnapshotFrame.typed_projections`
    /// sidecar (ADR-0037/0044). The consumer-side decoder generated by
    /// [`crate::swift_typed_decoders`] locates the envelope by the entry's own
    /// `key` and needs only the `flatc --swift` reader struct name (to decode
    /// it) from here; the neutral `schema_id` / `file_identifier` it verifies
    /// against are sourced from the projection contract by the entry's `key`
    /// (#1723).
    ///
    /// Every entry in the registry MUST have `typed_sidecar: Some(...)`.
    /// A `None` value means the projection has no typed wire form and is
    /// therefore a JSON-era vestigial that should be removed from the registry.
    /// The `typed_sidecar_coverage_gate` test in `swift_projections_registry_tests.rs`
    /// enforces this invariant and will fail if any entry has `None`.
    ///
    /// `swift_reader_type: None` inside a `Some(TypedSidecar { ... })` is the
    /// acceptable interim state: the typed FlatBuffer sidecar exists on the wire
    /// but the `flatc --swift` binding has not yet been checked into the Chirp
    /// target. The generator skips those entries (no Swift decoder emitted) but
    /// they remain in the registry because the WIRE form is canonical.
    pub typed_sidecar: Option<TypedSidecar>,
}

/// Typed-FlatBuffer-sidecar PRESENTATION identity for one projection key — the
/// Swift-specific facts the consumer-side decoder generator needs that are NOT
/// neutral: the producer envelope `key` and the `flatc --swift` reader struct
/// name.
///
/// #1723 (epic #1719): the neutral `schema_id` / `file_identifier` fields were
/// REMOVED from this struct (they are owned by the neutral
/// [`crate::projection_contract::PROJECTION_CONTRACT`] row, looked up by the
/// owning entry's `key`), and so was the redundant `key` field — the producer
/// envelope key is the SAME string as the entry's kernel-emitted `key`, so it is
/// no longer spelled a second time here. This struct now carries ONLY the one
/// genuinely-Swift presentation fact the contract cannot hold: the `flatc
/// --swift` reader struct name. The host-decoder generator
/// ([`crate::swift_typed_decoders`]) sources the neutral facts from the contract
/// by the entry's `key`. The host-side sidecar consumer still matches an
/// envelope by `envelope.key == <entry key> && envelope.schemaId == <contract
/// schema_id>`, then decodes via `getCheckedRoot(fileId: <contract
/// file_identifier>)` into the `swift_reader_type` struct.
pub struct TypedSidecar {
    /// The `flatc --swift` generated reader struct name
    /// (`namespace`-prefixed: `nmp_kernel_AccountsSnapshot`,
    /// `nmp_nip47_WalletStatus`), or `None` when the `flatc --swift` binding
    /// for this schema has NOT yet been generated + checked into the Chirp
    /// target.
    ///
    /// Only **six** `flatc --swift` bindings ship in
    /// `apps/chirp/ios/Chirp/Bridge/Generated/` today (op_feed, timeline_snapshot,
    /// content_tree, feed_home, nmp_update — plus the two proof-key bindings
    /// this PR adds: `accounts`, `active_account`). The remaining ~29 sidecar
    /// schemas have no Swift reader yet, so their `swift_reader_type` is `None`
    /// and the generator emits NO typed decoder for them — referencing a type
    /// the Chirp target cannot see would not compile. Generating those
    /// bindings (+ a binding-drift gate) is the named follow-up that unblocks
    /// the full sweep.
    pub swift_reader_type: Option<&'static str>,
}

/// The Stage 2 registry — every entry on the hand-written
/// `SnapshotProjections` struct in `KernelBridge.swift`, in declaration
/// order. Order is load-bearing (the generated file is byte-diffed against
/// the committed copy by the `codegen-drift` CI gate).
///
/// This slice has 31 entries (locked by `registry_size_is_locked`). Adding or
/// removing a member here changes the generated Swift — the CI gate will refuse
/// stale output until the regenerated file is committed.
///
/// #1610: removed the five JSON-era vestigial sidecar-less entries —
/// `timeline`, `inserted`, `updated`, `removed`, and `last_action_result`.
/// The coverage gate (`typed_sidecar_coverage_gate` test) now enforces that
/// every future entry carries `typed_sidecar: Some(...)` — no JSON-only slots.
pub const SNAPSHOT_PROJECTIONS: &[SnapshotProjectionEntry] = &[
    // Built-in NWC wallet projection. `projections["wallet"]`.
    SnapshotProjectionEntry {
        key: "wallet",
        swift_field: "wallet",
        swift_type: "WalletStatusData",
        // FLIPPED (Wave B Tier-1): the live typed `wallet` producer
        // (`apps/chirp/.../wallet_runtime.rs`:
        // `register_typed_snapshot_projection("wallet", …)` →
        // `wallet_typed_projection`) now carries the `wallet_pubkey_hex` field the
        // Swift `WalletStatusData.walletPubkeyHex` (`WalletView.swift:98`)
        // requires. The producer field-add landed it on BOTH the Rust
        // `WalletStatus` struct + the JSON projection
        // (`serde_json::to_value(WalletStatus)`) AND the `wallet_status.fbs`
        // wire (tail-appended, wire-compatible), so JSON + typed stay byte-
        // identical (no fabrication, additive parity preserved).
        //
        // NOTE on identity: the producer publishes ENVELOPE key `"wallet"`
        // (NOT `schema_id`); the decoder matches on `key == "wallet"` AND
        // `schema_id == "nmp.nip47.wallet"`. So `key` here is `"wallet"`
        // (the envelope key the producer emits) while `schema_id` stays
        // `"nmp.nip47.wallet"` — key ≠ schema_id for this entry.
        // `TypedProjectionGlue.wallet` maps the `NWST` reader to
        // `WalletStatusData`; consumed typed-first in `KernelModel.apply`
        // (`typedWallet ?? snapshot?.walletStatus`).
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_nip47_WalletStatus"),
        }),
    },
    // NIP-46 bunker handshake projection. `projections["bunker_handshake"]`.
    SnapshotProjectionEntry {
        key: "bunker_handshake",
        swift_field: "bunkerHandshake",
        swift_type: "BunkerHandshake",
        // Tier-1 actor projection: producer sets `key == schema_id`.
        typed_sidecar: Some(TypedSidecar {
            // Per-key sidecar flip: the `flatc --swift` reader
            // (`nmp_kernel_BunkerHandshake`) ships with this batch from
            // `crates/nmp-core/schema/bunker_handshake.fbs`. Flat field copy with
            // the `has_message` companion → `String?` mapping; the always-present
            // wire bools surface as the domain type's forward-compat `Bool?`
            // (non-nil from the typed path). See `TypedProjectionGlue.bunkerHandshake`.
            swift_reader_type: Some("nmp_kernel_BunkerHandshake"),
        }),
    },
    // NIP-46 typed onboarding read model. Always populated by the kernel;
    // optional only so an older kernel build that predates the projection
    // still decodes (D1).
    SnapshotProjectionEntry {
        key: "nip46_onboarding",
        swift_field: "nip46Onboarding",
        swift_type: "Nip46Onboarding",
        // Tier-1 actor projection: producer sets `key == schema_id`.
        typed_sidecar: Some(TypedSidecar {
            // Per-key sidecar flip: the `flatc --swift` reader
            // (`nmp_kernel_Nip46Onboarding`, wrapping `nmp_kernel_SignerApp`) ships
            // with this batch from `crates/nmp-core/schema/nip46_onboarding.fbs`.
            // `signer_apps` nested-vector copy; the `has_stage_kind` /
            // `has_progress_message` companions → `StageKind?` / `String?`; the
            // snake_case `stage_kind` wire token re-types to the same `StageKind`
            // enum the JSON path decodes (`unknown` forward-compat fallback). See
            // `TypedProjectionGlue.nip46Onboarding`.
            swift_reader_type: Some("nmp_kernel_Nip46Onboarding"),
        }),
    },
    // Unified remote-signer health (ADR-0048 D6 — generalises the V-14 step b
    // `bunker_connection_state` projection; hard-break rename, no compat key).
    // `projections["signer_state"]` — null when no remote-signer session is
    // active; `{ signer_kind, state, reason, is_ready, is_awaiting_approval,
    // is_reconnecting, is_unavailable, is_failed }` when one is live. Covers
    // BOTH NIP-46 bunker sessions and NIP-55 external-signer (Amber) sessions;
    // shells surface ONE status badge on the active remote account row keyed
    // by `signer_kind`, and a non-blocking alert banner when degraded.
    // Tier-1 actor projection: producer sets `key == schema_id`.
    SnapshotProjectionEntry {
        key: "signer_state",
        swift_field: "signerState",
        swift_type: "SignerState",
        // Typed FlatBuffers sidecar (KSST). The `flatc --swift` reader
        // (`nmp_kernel_SignerState`) ships from
        // `crates/nmp-core/schema/signer_state.fbs`. Field-for-field copy:
        // `{ signer_kind, state, has_reason, reason, is_ready,
        // is_awaiting_approval, is_reconnecting, is_unavailable, is_failed }`.
        // Only emitted when a remote-signer session is active (the slot is
        // `Some`) — mirrors the JSON closure's `null`-while-idle behaviour.
        // See `TypedProjectionGlue.signerState`.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_SignerState"),
        }),
    },
    // Publish-cluster outbox feeds — kernel-owned `publish_queue` and
    // `publish_outbox` arrays driven by the actor publish path.
    SnapshotProjectionEntry {
        key: "publish_queue",
        swift_field: "publishQueue",
        swift_type: "[PublishQueueEntry]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_PublishQueueSnapshot`) ships in this PR. The Chirp
            // domain `[PublishQueueEntry]` is a field-SUBSET of the wire
            // (eventId/kind/targetRelays/status only); the glue maps the
            // subset (see `TypedProjectionGlue.publishQueue`).
            swift_reader_type: Some("nmp_kernel_PublishQueueSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "publish_outbox",
        swift_field: "publishOutbox",
        swift_type: "[PublishOutboxItem]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_PublishOutboxSnapshot`) ships in this PR. Field-for-
            // field copy of each item + nested `[PublishOutboxRelay]` rows; the
            // glue widens `targetRelays` (uint → Int). See
            // `TypedProjectionGlue.publishOutbox`.
            swift_reader_type: Some("nmp_kernel_PublishOutboxSnapshot"),
        }),
    },
    // §6/AP1 pre-formatted outbox header — kernel-owned strings the shell
    // renders verbatim.
    SnapshotProjectionEntry {
        key: "outbox_summary",
        swift_field: "outboxSummary",
        swift_type: "OutboxSummary",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_OutboxSummarySnapshot`) ships in this PR. Single-table
            // field-for-field copy (kernel owns title/subtitle strings). See
            // `TypedProjectionGlue.outboxSummary`.
            swift_reader_type: Some("nmp_kernel_OutboxSummarySnapshot"),
        }),
    },
    // Relay-edit settings cluster — pre-rolled rows + role pick options.
    SnapshotProjectionEntry {
        key: "configured_relays",
        swift_field: "configuredRelays",
        swift_type: "[AppRelay]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_ConfiguredRelaysSnapshot`) ships in this PR. Two-field
            // (url/role) copy. See `TypedProjectionGlue.configuredRelays`.
            swift_reader_type: Some("nmp_kernel_ConfiguredRelaysSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "relay_role_options",
        swift_field: "relayRoleOptions",
        swift_type: "[RelayRoleOption]",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_RelayRoleOptionsSnapshot`) ships in this PR. Two-field
            // (value/isDefault) copy. See
            // `TypedProjectionGlue.relayRoleOptions`.
            swift_reader_type: Some("nmp_kernel_RelayRoleOptionsSnapshot"),
        }),
    },
    // D0 identity output. `accounts` enriches AccountSummary rows with
    // kind:0 metadata; `active_account` is the active pubkey scalar.
    SnapshotProjectionEntry {
        key: "accounts",
        swift_field: "accounts",
        swift_type: "[AccountSummary]",
        // PROOF KEY (thin glue): the `accounts` sidecar IS emitted (Tier-2
        // built-in, `key == schema_id`) AND its `flatc --swift` reader
        // (`nmp_kernel_AccountsSnapshot`) ships in this PR. The generator emits
        // a typed decoder; the hand-written glue maps the FB rows (two
        // `has_*` companion-bool optional strings) to `[AccountSummary]`.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_AccountsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "active_account",
        swift_field: "activeAccount",
        swift_type: "String",
        // PROOF KEY (thinnest glue): the `active_account` sidecar IS emitted
        // (Tier-2 built-in, `key == schema_id`) AND its `flatc --swift` reader
        // (`nmp_kernel_ActiveAccountSnapshot`) ships in this PR. The glue is a
        // single line — `fb.hasActiveAccount ? fb.pubkey : nil` — mapping the
        // companion-bool to the optional `String` the JSON path yields.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_ActiveAccountSnapshot"),
        }),
    },
    // Action lifecycle cluster — see kernel/update.rs::snapshot_projections_with_publish_cluster.
    // `action_results` is a per-tick drain; `action_stages` is the
    // per-correlation_id stage mirror; `action_lifecycle` is the V5
    // collapsed view (`in_flight` + `recent_terminal` w/ TTL eviction).
    // `last_action_result` (the deprecated sticky scalar) was removed in #1610:
    // `action_results` is the canonical typed source.
    SnapshotProjectionEntry {
        key: "action_results",
        swift_field: "actionResults",
        swift_type: "[LastActionResult]",
        typed_sidecar: Some(TypedSidecar {
            // Wave C: `flatc --swift` reader (`nmp_kernel_ActionResultsSnapshot`)
            // generated in this PR. Each `ActionResult` row maps
            // `correlation_id`, `status`, `has_error`/`error` to the existing
            // `LastActionResult` Swift type. See `TypedProjectionGlue.actionResults`.
            swift_reader_type: Some("nmp_kernel_ActionResultsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "action_stages",
        swift_field: "actionStages",
        swift_type: "[String: [ActionStageEntry]]",
        typed_sidecar: Some(TypedSidecar {
            // Wave C: `flatc --swift` reader (`nmp_kernel_ActionStagesSnapshot`)
            // generated in this PR. The outer snapshot carries a vector of
            // `ActionStagesEntry` (one per correlation_id), each with its own
            // `ActionStageEntry` vector. See `TypedProjectionGlue.actionStages`.
            swift_reader_type: Some("nmp_kernel_ActionStagesSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "action_lifecycle",
        swift_field: "actionLifecycle",
        swift_type: "ActionLifecycleSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #3: the `flatc --swift` reader
            // (`nmp_kernel_ActionLifecycleSnapshot`) ships in this PR. The
            // `{ in_flight, recent_terminal }` struct maps field-for-field to
            // the Chirp `ActionLifecycleSnapshot`; each `LifecycleEntry` row
            // reconstructs the `ActionLifecycleStage` enum from
            // `stage` + `has_reason`/`reason`. See
            // `TypedProjectionGlue.actionLifecycle`.
            swift_reader_type: Some("nmp_kernel_ActionLifecycleSnapshot"),
        }),
    },
    // D0 views cluster — `profile` (typed). V-112 (ADR-0042): author_view /
    // thread_view deleted. #1610: the JSON-era `timeline`, `inserted`,
    // `updated`, `removed` per-tick delta slots deleted — the typed feed ships
    // via `nmp.feed.home` (`OpFeedSnapshot`) which is the canonical typed form.
    SnapshotProjectionEntry {
        key: "profile",
        swift_field: "profile",
        swift_type: "ProfileCard",
        typed_sidecar: Some(TypedSidecar {
            // Profile-cluster batch: the `flatc --swift` reader
            // (`nmp_kernel_ProfileSnapshot`, wrapping the SHARED
            // `nmp_kernel_ProfileCard` from `ProfileCard.generated.swift`) ships
            // with this batch. Single-card copy with `has_*`→`String?` companion
            // mapping. See `TypedProjectionGlue.profile`.
            swift_reader_type: Some("nmp_kernel_ProfileSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.feed.home",
        swift_field: "homeFeed",
        // Framework/protocol type name: `OpFeedSnapshot` mirrors the Rust
        // `nmp_note_feed::op_feed::OpFeedSnapshot` (`RootFeedSnapshot<…>`) without
        // embedding any app name.
        swift_type: "OpFeedSnapshot",
        // The op-feed pilot — the ONLY case where producer `key` (here
        // `"nmp.feed.home"`) differs from `schema_id` (`"nmp.note_feed.opfeed"`).
        // Already consumed by the hand-written `TypedHomeFeedDecoder` (nested
        // NFWM/NFCT sub-buffer decode = thick bespoke glue), so the generator
        // does NOT emit a decoder for it: `swift_reader_type: None` keeps it
        // out of generated scope and avoids colliding with the existing wiring.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: None,
        }),
    },
    // Host-registered dotted-key projections. The `.` in the JSON key is
    // opaque to `.convertFromSnakeCase` (it splits on `_` only), so the
    // post-transform key keeps the `nmp.<nip>.<verb>` shape but with the
    // tail camelised.
    SnapshotProjectionEntry {
        key: "nmp.nip29.group_events",
        swift_field: "groupEvents",
        swift_type: "GroupEventsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip29_GroupEventsSnapshot`) ships in this PR. Host-registered
            // producer is the NIP-29 group-events typed read session (#2187):
            // descriptor open → `register_typed_snapshot_projection`.
            // Flat field-for-field copy: `{ events: [GroupEvent] }`,
            // each row `{ id, pubkey, content, created_at, kind }`. See
            // `TypedProjectionGlue.groupEvents`.
            swift_reader_type: Some("nmp_nip29_GroupEventsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip17.dm_inbox",
        swift_field: "dmInbox",
        swift_type: "DmInboxSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // NIP-17 DM cluster batch: the `flatc --swift` reader
            // (`nmp_nip17_DmInboxSnapshot`, wrapping `nmp_nip17_DmConversation` /
            // `nmp_nip17_DmMessage`) ships with this batch from
            // `crates/nmp-nip17/schema/dm_inbox.fbs`. Conversations/messages
            // nested-vector copy preserving the Rust newest-first order. See
            // `TypedProjectionGlue.dmInbox`.
            swift_reader_type: Some("nmp_nip17_DmInboxSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.follow_list",
        swift_field: "followList",
        swift_type: "FollowListSnapshot",
        // The registry/projection key (`nmp.follow_list`) differs from the
        // buffer's `schema_id` (`nmp.nip02.follow_list`); verify the producer's
        // actual `(key, schema_id)` push before generating its decoder.
        typed_sidecar: Some(TypedSidecar {
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip02_FollowListSnapshot`) ships in this PR. Host-registered
            // producer in `apps/chirp/.../ffi/register.rs`
            // (`register_typed_snapshot_projection("nmp.follow_list", …)` →
            // `follow_list_typed_projection`). Note the deliberate key/schema_id
            // split: the envelope KEY is `nmp.follow_list`, the payload
            // SCHEMA_ID is `nmp.nip02.follow_list` — the generated decoder
            // matches on BOTH (verified against the producer). Flat copy:
            // `{ follows: [FollowEntry{pubkey}] }`. See
            // `TypedProjectionGlue.followList`.
            swift_reader_type: Some("nmp_nip02_FollowListSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip29.discovered_groups",
        swift_field: "discoveredGroups",
        swift_type: "DiscoveredGroupsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip29_DiscoveredGroupsSnapshot`) ships in this PR.
            // Host-registered producer is the NIP-29 group-discovery typed read
            // session (#2088): descriptor open → typed snapshot projection.
            // Flat copy: `{ host_relay_url, groups: [DiscoveredGroup] }`. The
            // `name`/`picture`/`about` wire strings are bare (absent == None) and
            // map to the domain's `String?` preserving nil — NOT `?? ""` — so
            // typed and JSON are byte-identical. See
            // `TypedProjectionGlue.discoveredGroups`.
            swift_reader_type: Some("nmp_nip29_DiscoveredGroupsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip29.group_defaults",
        swift_field: "groupDefaults",
        swift_type: "GroupDefaultsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // #626: the crate-owned NIP-29 public-group create defaults (the
            // suggested host relay URL). Host-registered producer in
            // `apps/chirp/.../ffi/register.rs`
            // (`nmp_nip29::register::wire_group_defaults` →
            // `register_typed_snapshot_projection("nmp.nip29.group_defaults", …)`).
            // The `flatc --swift` reader (`nmp_nip29_GroupDefaultsSnapshot`) ships
            // from `crates/nmp-nip29/schema/group_defaults.fbs`. Flat copy:
            // `{ suggested_relay_url }`. See `TypedProjectionGlue.groupDefaults`.
            swift_reader_type: Some("nmp_nip29_GroupDefaultsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        key: "nmp.nip17.dm_relay_list",
        swift_field: "dmRelayList",
        swift_type: "DmRelayListSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // NIP-17 DM cluster batch: the `flatc --swift` reader
            // (`nmp_nip17_DmRelayListSnapshot`) ships with this batch from
            // `crates/nmp-nip17/schema/dm_relay_list.fbs`. Flat field-for-field
            // copy with `has_active_pubkey`→`String?` companion mapping. This key
            // has NO current Swift read consumer (the `dmRelayList` accessor is
            // the only seam, added for parity); flipping it is purely additive.
            // See `TypedProjectionGlue.dmRelayList`.
            swift_reader_type: Some("nmp_nip17_DmRelayListSnapshot"),
        }),
    },
    // Diagnostics roll-up + pre-merged resolved-profile map + settings hub
    // view. All single-component snake_case keys that pass through
    // `.convertFromSnakeCase` cleanly.
    SnapshotProjectionEntry {
        key: "relay_diagnostics",
        swift_field: "relayDiagnostics",
        swift_type: "RelayDiagnosticsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Wave B batch #3: the `flatc --swift` reader
            // (`nmp_kernel_RelayDiagnosticsSnapshot`) ships in this PR. Pure
            // field-for-field copy of the rolled-up relay rows + nested
            // wire-sub rows + logical-interest rows; every `has_*` companion
            // bool maps the optional `String?` (nil when absent). See
            // `TypedProjectionGlue.relayDiagnostics`.
            swift_reader_type: Some("nmp_kernel_RelayDiagnosticsSnapshot"),
        }),
    },
    // Pre-resolved embed-envelope map over authoritative `refs.event` rows
    // (issue #1283 / ADR-0034 §embed-sidecar) — keyed by `primary_id`, one
    // `EmbeddedEventEnvelope` (the kind-dispatched `EmbedKindProjection`) per
    // currently resolved event ref. Produced by `crates/nmp-ffi/src/embed_sidecar.rs`,
    // which materialises `refs.event` rows through
    // `nmp_content::resolve_embed_projection` and emits the typed `NEMB`
    // FlatBuffer (this entry, Chirp typed-frame shell). Decoding the typed
    // sidecar is what lets Chirp delete its in-Swift `match kind` embed resolver
    // (the EmbedHost D0 violation #1283 closes). The Swift value type
    // `EmbeddedEventEnvelope` is hand-declared in
    // `ios/.../Components/NostrContent/EmbedKindProjection.swift`; the glue
    // builds it from the typed reader. Drives `EmbedHost.update(envelopes:)`.
    SnapshotProjectionEntry {
        key: "refs.event.envelopes",
        swift_field: "refEventEnvelopes",
        swift_type: "[String: EmbeddedEventEnvelope]",
        typed_sidecar: Some(TypedSidecar {
            // Producer sets `key == schema_id == "refs.event.envelopes"`
            // (`embed_sidecar::install_embed_sidecar_projection`).
            // `flatc --swift` reader from `crates/nmp-content/schema/embed_sidecar.fbs`
            // (`apps/chirp/ios/Chirp/Bridge/Generated/RefEventEnvelopes.generated.swift`).
            // The `[EmbeddedEventEnvelope]` (key-sorted on `primary_id`) →
            // `[String: EmbeddedEventEnvelope]` map + the kind-discriminated
            // `EmbedKindProjection` mapping is `TypedProjectionGlue.refEventEnvelopes`.
            swift_reader_type: Some("nmp_embed_RefEventEnvelopes"),
        }),
    },
    SnapshotProjectionEntry {
        key: "settings_hub",
        swift_field: "settingsHub",
        swift_type: "[String: Int]",
        // FLIPPED: the kernel-built-in typed sidecar (`KSHB` /
        // `nmp_kernel_SettingsHubSnapshot`, Tier-2 `builtin_typed_projections`)
        // carries `relay_count:uint`, encoded from the SAME
        // `configured_relays_snapshot().len()` the JSON path reads. The FB table
        // (`{ relay_count }`) does not literally equal the Chirp domain type
        // (`[String: Int]`), so `TypedProjectionGlue.settingsHub` rebuilds the
        // single-key map `["relay_count": Int(reader.relayCount)]` — the EXACT
        // dict the JSON `projections["settings_hub"]` yields (byte-identical, no
        // fabrication). Consumed typed-first in `KernelModel.apply`
        // (`typedSettingsHub ?? update.projections?.settingsHub`); the existing
        // `SettingsHubSummary(relayCount:)` wrap (KernelBridge) stays untouched.
        typed_sidecar: Some(TypedSidecar {
            swift_reader_type: Some("nmp_kernel_SettingsHubSnapshot"),
        }),
    },
    // V-107 / ADR-0039: Marmot (MLS-over-Nostr) push projections. Both are
    // host-registered in `nmp_marmot::ffi::register_with_keys` on every
    // Marmot sign-in; the projection slot emits empty objects on sign-out
    // (D1 forward-compat: `nil` on a kernel build that predates registration
    // OR an empty `{}` when the slot is None — both decode safely because all
    // fields are optional).
    //
    // `nmp.marmot.snapshot` has no `_` segment, so post-convertFromSnakeCase
    // the key is identical: `"nmp.marmot.snapshot"`. The CodingKeys case
    // still needs an explicit raw value because it is a dotted key that the
    // synthesised decoder (which would pick the property name) cannot match
    // (the generated CodingKeys enum covers ALL cases, not just dotted ones).
    SnapshotProjectionEntry {
        key: "nmp.marmot.snapshot",
        swift_field: "marmotSnapshot",
        swift_type: "MarmotSnapshot",
        typed_sidecar: Some(TypedSidecar {
            // Marmot push-projection batch: the `flatc --swift` reader
            // (`nmp_marmot_MarmotSnapshot`, wrapping `nmp_marmot_MarmotGroupRow` /
            // `nmp_marmot_PendingWelcomeRow` / `nmp_marmot_KeyPackageStatus`) ships
            // with this batch from `crates/nmp-marmot/schema/marmot_snapshot.fbs`.
            // Host-registered typed producer in `crates/nmp-marmot/src/ffi.rs`
            // (`register_typed_snapshot_projection("nmp.marmot.snapshot", …)` →
            // `crate::wire::snapshot_fb::typed_projection`). Nested-vector copy of
            // `groups`/`pendingWelcomes` plus the `keyPackage` sub-table; every
            // `has_*` companion bool maps the optional `String?`/`UInt32?`/`UInt64?`
            // (nil when absent) so the typed value is byte-identical to the JSON
            // path's `null`. The wire's `orphanedCommitCount` diagnostic is NOT
            // carried by the Chirp `MarmotSnapshot` domain type; #1651 the
            // `initErrorKind`/`initErrorDetail` service-init diagnostic (which
            // replaced the former `keyringUnavailable` bool) IS now carried.
            // Consumed by `MarmotStore.apply` via the `KernelModel.swift` fan-out
            // (`result.typedMarmotSnapshot ?? update.projections?.marmotSnapshot`).
            // See `TypedProjectionGlue.marmotSnapshot`.
            swift_reader_type: Some("nmp_marmot_MarmotSnapshot"),
        }),
    },
    // `nmp.marmot.messages` projects a JSON object keyed by `group_id_hex`
    // → newest-N `MarmotMessageRow` array (all groups in one map).
    // Post-convertFromSnakeCase the key is `"nmp.marmot.messages"` (no `_`).
    SnapshotProjectionEntry {
        key: "nmp.marmot.messages",
        swift_field: "marmotMessages",
        swift_type: "[String: [MarmotMessage]]",
        typed_sidecar: Some(TypedSidecar {
            // Marmot push-projection batch: the `flatc --swift` reader
            // (`nmp_marmot_MarmotMessages`, wrapping `nmp_marmot_MarmotGroupMessages`
            // / `nmp_marmot_MarmotMessageRow`) ships with this batch from
            // `crates/nmp-marmot/schema/marmot_messages.fbs`. Host-registered typed
            // producer in `crates/nmp-marmot/src/ffi.rs`
            // (`register_typed_snapshot_projection("nmp.marmot.messages", …)` →
            // `crate::wire::messages_fb::typed_projection`). FlatBuffers has no map
            // type, so the producer flattens the `group_id_hex -> [MarmotMessageRow]`
            // JSON map to a `group_id_hex`-sorted `[MarmotGroupMessages]` vector;
            // the glue rebuilds the domain `[String: [MarmotMessage]]` dict
            // (mirroring the `claimed_profiles`/`zaps` flattened-map precedent).
            // `epoch` carries a `has_epoch` companion → `UInt64?` (nil when absent).
            // Consumed by `MarmotStore.apply` via the `KernelModel.swift` fan-out
            // (`result.typedMarmotMessages ?? update.projections?.marmotMessages`).
            // See `TypedProjectionGlue.marmotMessages`.
            swift_reader_type: Some("nmp_marmot_MarmotMessages"),
        }),
    },
];

#[cfg(test)]
#[path = "swift_projections_registry_tests.rs"]
mod tests;
