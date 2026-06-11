//! V6 Stage 2 — `SnapshotProjections` dotted-projection-key registry.
//!
//! This module owns the single source of truth that replaces the hand-written
//! `SnapshotProjections` struct + `CodingKeys` enum at the bottom of
//! `ios/Chirp/Chirp/Bridge/KernelBridge.swift`. The renderer in
//! [`crate::swift`] reads this slice and emits the equivalent Swift.
//!
//! ## Why the registry lives in `nmp-codegen`, not `nmp-core`
//!
//! The Stage 2 registry is a list of `(json_key, swift_field, swift_type)`
//! triples — there is no Rust type to reflect via `schemars` (unlike Stage 1).
//! The natural home would have been `nmp-core::codegen_schema` alongside
//! Stage 1, BUT the registry MUST name dotted host-registered keys like
//! `"nmp.nip29.group_chat"`, `"nmp.nip17.dm_inbox"`, `"nmp.nip57.zaps"`.
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

/// One entry in the dotted-projection-key registry.
///
/// The hand-written `SnapshotProjections` declaration in
/// `ios/Chirp/Chirp/Bridge/KernelBridge.swift` is the byte-for-byte target
/// the renderer must reproduce. Every field on that struct corresponds to
/// exactly one entry here, in declaration order.
pub struct SnapshotProjectionEntry {
    /// Kernel-emitted JSON key as it appears in the `projections` map. Used
    /// to compute the `CodingKeys` raw value via Apple's
    /// `.convertFromSnakeCase` transform (split on `_` only — `.` is opaque).
    ///
    /// Examples:
    /// - `"wallet"` → no transform needed, post-transform is `"wallet"`.
    /// - `"action_stages"` → post-transform is `"actionStages"`.
    /// - `"nmp.nip29.group_chat"` → post-transform is `"nmp.nip29.groupChat"`
    ///   (the `.`-segments stay intact, only `group_chat` camelises).
    pub json_key: &'static str,
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
    /// `"GroupChatSnapshot"`. Container types are written in their full
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
    /// [`crate::swift_typed_decoders`] needs the sidecar's `(key, schema_id,
    /// file_identifier)` triple to locate + verify the buffer, plus the name
    /// of the `flatc --swift` reader struct to decode it.
    ///
    /// `None` for the handful of registry keys whose JSON projection has NO
    /// typed sidecar counterpart on the wire today:
    /// - `last_action_result` — sticky scalar; no dedicated sidecar (the
    ///   per-tick `action_results` array carries the typed form instead).
    /// - `timeline` / `inserted` / `updated` / `removed` — the per-tick
    ///   timeline-delta arrays have no standalone typed sidecar; the typed
    ///   feed ships via `nmp.feed.home` (`OpFeedSnapshot`).
    ///
    /// The renderer skips `None` entries entirely — it never emits a typed
    /// decoder that references a non-existent sidecar OR a `flatc` Swift
    /// reader type that is not present in the Chirp target.
    pub typed_sidecar: Option<TypedSidecar>,
}

/// Typed-FlatBuffer-sidecar identity for one projection key — the data the
/// consumer-side decoder generator needs to locate, verify, and decode the
/// sidecar buffer into its `flatc --swift` reader struct.
///
/// ## Why these three fields
///
/// The host-side sidecar consumer matches an envelope by
/// `envelope.key == key && envelope.schemaId == schema_id` (see
/// `ios/Chirp/Chirp/Bridge/TypedHomeFeedDecoder.swift`, the hand-written
/// precedent), then decodes `envelope.payload` via
/// `getCheckedRoot(byteBuffer:, fileId: file_identifier)` to obtain the
/// `swift_reader_type` struct.
pub struct TypedSidecar {
    /// `TypedProjection.key` as the producer sets it. For Tier-2 kernel
    /// built-ins and Tier-1 actor projections the producer sets
    /// `key == schema_id` (see `Kernel::builtin_typed_projections` and
    /// `crate::actor::typed_projections`). For the op-feed pilot the producer
    /// sets `key = "nmp.feed.home"` while `schema_id = "nmp.nip01.opfeed"`.
    /// Always the EXACT string the producer pushes — do not derive it from
    /// `json_key` (they differ for `wallet`, `nmp.feed.home`, `nmp.follow_list`).
    pub key: &'static str,
    /// `TypedPayload.schema_id` — the buffer's stable schema identity
    /// (`*_SCHEMA_ID` constant on the producer crate, e.g.
    /// `"nmp.nip47.wallet"`, `"accounts"`, `"nmp.nip01.opfeed"`).
    pub schema_id: &'static str,
    /// FlatBuffers `file_identifier` (the 4-byte `*_FILE_IDENTIFIER` constant,
    /// e.g. `"KACC"`, `"NWST"`, `"NOFS"`). Passed to `getCheckedRoot(fileId:)`.
    pub file_identifier: &'static str,
    /// The `flatc --swift` generated reader struct name
    /// (`namespace`-prefixed: `nmp_kernel_AccountsSnapshot`,
    /// `nmp_nip47_WalletStatus`), or `None` when the `flatc --swift` binding
    /// for this schema has NOT yet been generated + checked into the Chirp
    /// target.
    ///
    /// Only **six** `flatc --swift` bindings ship in
    /// `ios/Chirp/Chirp/Bridge/Generated/` today (op_feed, timeline_snapshot,
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
/// This slice has 35 entries (locked by `registry_size_is_locked`). Adding or
/// removing a member here changes the generated Swift — the CI gate will refuse
/// stale output until the regenerated file is committed.
pub const SNAPSHOT_PROJECTIONS: &[SnapshotProjectionEntry] = &[
    // Built-in NWC wallet projection. `projections["wallet"]`.
    SnapshotProjectionEntry {
        json_key: "wallet",
        swift_field: "wallet",
        swift_type: "WalletStatusData",
        // DEFERRED (Wave B Tier-1): a live typed `wallet` producer DOES exist
        // (`apps/chirp/.../wallet_runtime.rs`:
        // `register_typed_snapshot_projection("wallet", …)` →
        // `wallet_typed_projection`), but the read-flip is BLOCKED by a domain
        // mismatch the read-side PR cannot fix additively:
        //   * the Swift `WalletStatusData` has a non-optional `walletPubkeyHex`
        //     (`WalletView.swift:98`) that NEITHER the JSON projection
        //     (`serde_json::to_value(WalletStatus)`) NOR the `wallet_status.fbs`
        //     wire carries — there is no producer for it. A typed flip would
        //     have to fabricate it (npub→hex = thin-shell violation; or `""` =
        //     divergence from the JSON path), breaking the additive parity
        //     contract.
        //   * also the producer publishes ENVELOPE key `"wallet"` (not
        //     `"nmp.nip47.wallet"` recorded below); the decoder's `key` is
        //     sourced from this `TypedSidecar.key`, so it must be corrected to
        //     `"wallet"` when wallet is wired.
        // The fix is producer-side (add `wallet_pubkey_hex` to the Rust
        // `WalletStatus` struct + `.fbs`, then flip key→`"wallet"` and
        // `swift_reader_type`). Tracked as a scoped follow-up; left `None` here
        // so the generator emits no decoder for the absent-field sidecar.
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.nip47.wallet",
            schema_id: "nmp.nip47.wallet",
            file_identifier: "NWST",
            swift_reader_type: None,
        }),
    },
    // NIP-46 bunker handshake projection. `projections["bunker_handshake"]`.
    SnapshotProjectionEntry {
        json_key: "bunker_handshake",
        swift_field: "bunkerHandshake",
        swift_type: "BunkerHandshake",
        // Tier-1 actor projection: producer sets `key == schema_id`.
        typed_sidecar: Some(TypedSidecar {
            key: "bunker_handshake",
            schema_id: "bunker_handshake",
            file_identifier: "KBHS",
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
        json_key: "nip46_onboarding",
        swift_field: "nip46Onboarding",
        swift_type: "Nip46Onboarding",
        // Tier-1 actor projection: producer sets `key == schema_id`.
        typed_sidecar: Some(TypedSidecar {
            key: "nip46_onboarding",
            schema_id: "nip46_onboarding",
            file_identifier: "KN46",
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
    // NOTE: `bunker_connection_state` (V-14 step b) is emitted by the kernel
    // but deliberately absent from this registry until the iOS/Android follow-up
    // PR adds the Swift Decodable stub and regenerates KernelTypes.generated.swift.
    // The kernel emits the key regardless; shells that have not yet added the
    // field simply ignore it (D1 forward-compat). Mirrors the precedent set by
    // `mention_profiles` (emitted, not registered here).
    // Publish-cluster outbox feeds — kernel-owned `publish_queue` and
    // `publish_outbox` arrays driven by the actor publish path.
    SnapshotProjectionEntry {
        json_key: "publish_queue",
        swift_field: "publishQueue",
        swift_type: "[PublishQueueEntry]",
        typed_sidecar: Some(TypedSidecar {
            key: "publish_queue",
            schema_id: "publish_queue",
            file_identifier: "KPBQ",
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_PublishQueueSnapshot`) ships in this PR. The Chirp
            // domain `[PublishQueueEntry]` is a field-SUBSET of the wire
            // (eventId/kind/targetRelays/status only); the glue maps the
            // subset (see `TypedProjectionGlue.publishQueue`).
            swift_reader_type: Some("nmp_kernel_PublishQueueSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "publish_outbox",
        swift_field: "publishOutbox",
        swift_type: "[PublishOutboxItem]",
        typed_sidecar: Some(TypedSidecar {
            key: "publish_outbox",
            schema_id: "publish_outbox",
            file_identifier: "KPBO",
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
        json_key: "outbox_summary",
        swift_field: "outboxSummary",
        swift_type: "OutboxSummary",
        typed_sidecar: Some(TypedSidecar {
            key: "outbox_summary",
            schema_id: "outbox_summary",
            file_identifier: "KOXS",
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_OutboxSummarySnapshot`) ships in this PR. Single-table
            // field-for-field copy (kernel owns title/subtitle strings). See
            // `TypedProjectionGlue.outboxSummary`.
            swift_reader_type: Some("nmp_kernel_OutboxSummarySnapshot"),
        }),
    },
    // Relay-edit settings cluster — pre-rolled rows + role pick options.
    SnapshotProjectionEntry {
        json_key: "configured_relays",
        swift_field: "configuredRelays",
        swift_type: "[AppRelay]",
        typed_sidecar: Some(TypedSidecar {
            key: "configured_relays",
            schema_id: "configured_relays",
            file_identifier: "KCRL",
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_ConfiguredRelaysSnapshot`) ships in this PR. Two-field
            // (url/role) copy. See `TypedProjectionGlue.configuredRelays`.
            swift_reader_type: Some("nmp_kernel_ConfiguredRelaysSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "relay_role_options",
        swift_field: "relayRoleOptions",
        swift_type: "[RelayRoleOption]",
        typed_sidecar: Some(TypedSidecar {
            key: "relay_role_options",
            schema_id: "relay_role_options",
            file_identifier: "KRRO",
            // Wave B batch #2: the `flatc --swift` reader
            // (`nmp_kernel_RelayRoleOptionsSnapshot`) ships in this PR. Four-field
            // (value/label/tint/isDefault) copy. See
            // `TypedProjectionGlue.relayRoleOptions`.
            swift_reader_type: Some("nmp_kernel_RelayRoleOptionsSnapshot"),
        }),
    },
    // D0 identity output. `accounts` enriches AccountSummary rows with
    // kind:0 metadata; `active_account` is the active pubkey scalar.
    SnapshotProjectionEntry {
        json_key: "accounts",
        swift_field: "accounts",
        swift_type: "[AccountSummary]",
        // PROOF KEY (thin glue): the `accounts` sidecar IS emitted (Tier-2
        // built-in, `key == schema_id`) AND its `flatc --swift` reader
        // (`nmp_kernel_AccountsSnapshot`) ships in this PR. The generator emits
        // a typed decoder; the hand-written glue maps the FB rows (two
        // `has_*` companion-bool optional strings) to `[AccountSummary]`.
        typed_sidecar: Some(TypedSidecar {
            key: "accounts",
            schema_id: "accounts",
            file_identifier: "KACC",
            swift_reader_type: Some("nmp_kernel_AccountsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "active_account",
        swift_field: "activeAccount",
        swift_type: "String",
        // PROOF KEY (thinnest glue): the `active_account` sidecar IS emitted
        // (Tier-2 built-in, `key == schema_id`) AND its `flatc --swift` reader
        // (`nmp_kernel_ActiveAccountSnapshot`) ships in this PR. The glue is a
        // single line — `fb.hasActiveAccount ? fb.pubkey : nil` — mapping the
        // companion-bool to the optional `String` the JSON path yields.
        typed_sidecar: Some(TypedSidecar {
            key: "active_account",
            schema_id: "active_account",
            file_identifier: "KACT",
            swift_reader_type: Some("nmp_kernel_ActiveAccountSnapshot"),
        }),
    },
    // Action lifecycle cluster — see kernel/update.rs::snapshot_projections_with_publish_cluster.
    // `action_results` is a per-tick drain; `last_action_result` is the
    // sticky scalar for backward compat; `action_stages` is the
    // per-correlation_id stage mirror; `action_lifecycle` is the V5
    // collapsed view (`in_flight` + `recent_terminal` w/ TTL eviction).
    SnapshotProjectionEntry {
        json_key: "action_results",
        swift_field: "actionResults",
        swift_type: "[LastActionResult]",
        typed_sidecar: Some(TypedSidecar {
            key: "action_results",
            schema_id: "action_results",
            file_identifier: "KARS",
            swift_reader_type: None,
        }),
    },
    SnapshotProjectionEntry {
        json_key: "last_action_result",
        swift_field: "lastActionResult",
        swift_type: "LastActionResult",
        // No dedicated typed sidecar — the sticky scalar reuses the per-tick
        // `action_results` array's typed form; the JSON `payload` path is the
        // only wire form for this key.
        typed_sidecar: None,
    },
    SnapshotProjectionEntry {
        json_key: "action_stages",
        swift_field: "actionStages",
        swift_type: "[String: [ActionStageEntry]]",
        typed_sidecar: Some(TypedSidecar {
            key: "action_stages",
            schema_id: "action_stages",
            file_identifier: "KAST",
            swift_reader_type: None,
        }),
    },
    SnapshotProjectionEntry {
        json_key: "action_lifecycle",
        swift_field: "actionLifecycle",
        swift_type: "ActionLifecycleSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "action_lifecycle",
            schema_id: "action_lifecycle",
            file_identifier: "KALC",
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
    // D0 views cluster — `profile`, `timeline`, `author_view`,
    // `thread_view`, plus the per-tick `inserted` / `updated` / `removed`
    // timeline deltas.
    SnapshotProjectionEntry {
        json_key: "profile",
        swift_field: "profile",
        swift_type: "ProfileCard",
        typed_sidecar: Some(TypedSidecar {
            key: "profile",
            schema_id: "profile",
            file_identifier: "KPRF",
            // Profile-cluster batch: the `flatc --swift` reader
            // (`nmp_kernel_ProfileSnapshot`, wrapping the SHARED
            // `nmp_kernel_ProfileCard` from `ProfileCard.generated.swift`) ships
            // with this batch. Single-card copy with `has_*`→`String?` companion
            // mapping. See `TypedProjectionGlue.profile`.
            swift_reader_type: Some("nmp_kernel_ProfileSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "timeline",
        swift_field: "timeline",
        swift_type: "[TimelineItem]",
        // No standalone typed sidecar — the typed feed ships via
        // `nmp.feed.home` (`OpFeedSnapshot`); this generic timeline array has
        // only the JSON `payload` wire form.
        typed_sidecar: None,
    },
    SnapshotProjectionEntry {
        json_key: "nmp.feed.home",
        swift_field: "homeFeed",
        swift_type: "ChirpTimelineSnapshot",
        // The op-feed pilot — the ONLY case where producer `key` (here
        // `"nmp.feed.home"`) differs from `schema_id` (`"nmp.nip01.opfeed"`).
        // Already consumed by the hand-written `TypedHomeFeedDecoder` (nested
        // NFWM/NFCT sub-buffer decode = thick bespoke glue), so the generator
        // does NOT emit a decoder for it: `swift_reader_type: None` keeps it
        // out of generated scope and avoids colliding with the existing wiring.
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.feed.home",
            schema_id: "nmp.nip01.opfeed",
            file_identifier: "NOFS",
            swift_reader_type: None,
        }),
    },
    SnapshotProjectionEntry {
        json_key: "author_view",
        swift_field: "authorView",
        swift_type: "AuthorProfileSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "author_view",
            schema_id: "author_view",
            file_identifier: "KAVW",
            swift_reader_type: None,
        }),
    },
    SnapshotProjectionEntry {
        json_key: "thread_view",
        swift_field: "threadView",
        swift_type: "ThreadView",
        typed_sidecar: Some(TypedSidecar {
            key: "thread_view",
            schema_id: "thread_view",
            file_identifier: "KTVW",
            swift_reader_type: None,
        }),
    },
    SnapshotProjectionEntry {
        json_key: "inserted",
        swift_field: "inserted",
        swift_type: "[TimelineItem]",
        // Per-tick timeline delta — no standalone typed sidecar (JSON only).
        typed_sidecar: None,
    },
    SnapshotProjectionEntry {
        json_key: "updated",
        swift_field: "updated",
        swift_type: "[TimelineItem]",
        // Per-tick timeline delta — no standalone typed sidecar (JSON only).
        typed_sidecar: None,
    },
    SnapshotProjectionEntry {
        json_key: "removed",
        swift_field: "removed",
        swift_type: "[String]",
        // Per-tick timeline delta — no standalone typed sidecar (JSON only).
        typed_sidecar: None,
    },
    // Host-registered dotted-key projections. The `.` in the JSON key is
    // opaque to `.convertFromSnakeCase` (it splits on `_` only), so the
    // post-transform key keeps the `nmp.<nip>.<verb>` shape but with the
    // tail camelised.
    SnapshotProjectionEntry {
        json_key: "nmp.nip29.group_chat",
        swift_field: "groupChat",
        swift_type: "GroupChatSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.nip29.group_chat",
            schema_id: "nmp.nip29.group_chat",
            file_identifier: "NGCS",
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip29_GroupChatSnapshot`) ships in this PR. Host-registered
            // producer in `apps/chirp/.../crates/nmp-nip29/src/register.rs`
            // (`register_typed_snapshot_projection("nmp.nip29.group_chat", …)`).
            // Flat field-for-field copy: `{ messages: [GroupChatMessage] }`,
            // each row `{ id, pubkey, content, created_at, kind }`. See
            // `TypedProjectionGlue.groupChat`.
            swift_reader_type: Some("nmp_nip29_GroupChatSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "nmp.nip17.dm_inbox",
        swift_field: "dmInbox",
        swift_type: "DmInboxSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.nip17.dm_inbox",
            schema_id: "nmp.nip17.dm_inbox",
            file_identifier: "NDMI",
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
        json_key: "nmp.follow_list",
        swift_field: "followList",
        swift_type: "FollowListSnapshot",
        // The registry/projection key (`nmp.follow_list`) differs from the
        // buffer's `schema_id` (`nmp.nip02.follow_list`); verify the producer's
        // actual `(key, schema_id)` push before generating its decoder.
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.follow_list",
            schema_id: "nmp.nip02.follow_list",
            file_identifier: "NF02",
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
        json_key: "nmp.nip29.discovered_groups",
        swift_field: "discoveredGroups",
        swift_type: "DiscoveredGroupsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.nip29.discovered_groups",
            schema_id: "nmp.nip29.discovered_groups",
            file_identifier: "NDGS",
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip29_DiscoveredGroupsSnapshot`) ships in this PR.
            // Host-registered producer in `crates/nmp-nip29/src/register.rs`
            // (`register_typed_snapshot_projection("nmp.nip29.discovered_groups", …)`).
            // Flat copy: `{ host_relay_url, groups: [DiscoveredGroup] }`. The
            // `name`/`picture`/`about` wire strings are bare (absent == None) and
            // map to the domain's `String?` preserving nil — NOT `?? ""` — so
            // typed and JSON are byte-identical. See
            // `TypedProjectionGlue.discoveredGroups`.
            swift_reader_type: Some("nmp_nip29_DiscoveredGroupsSnapshot"),
        }),
    },
    // `nmp.nip57.zaps` has no `_`, so the post-transform key is identical
    // — but declaring the `CodingKeys` enum overrides synthesised raw
    // values, so the case still needs the explicit literal.
    SnapshotProjectionEntry {
        json_key: "nmp.nip57.zaps",
        swift_field: "zaps",
        swift_type: "ZapsAggregateSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.nip57.zaps",
            schema_id: "nmp.nip57.zaps",
            file_identifier: "NZAP",
            // Wave B Tier-1 #4: the `flatc --swift` reader
            // (`nmp_nip57_ZapsSnapshot`) ships in this PR. Host-registered
            // producer in `apps/chirp/.../ffi/register.rs`
            // (`register_typed_snapshot_projection("nmp.nip57.zaps", …)` →
            // `zaps_typed_projection`). FlatBuffers has no map type, so the wire
            // is a flattened `[ZapTotal{target_event_id, total_msats, count}]`
            // vector; the glue rebuilds the domain `totals: [String: ZapCount]`
            // dict. See `TypedProjectionGlue.zaps`.
            swift_reader_type: Some("nmp_nip57_ZapsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "nmp.nip17.dm_relay_list",
        swift_field: "dmRelayList",
        swift_type: "DmRelayListSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.nip17.dm_relay_list",
            schema_id: "nmp.nip17.dm_relay_list",
            file_identifier: "NDRL",
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
        json_key: "relay_diagnostics",
        swift_field: "relayDiagnostics",
        swift_type: "RelayDiagnosticsSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "relay_diagnostics",
            schema_id: "relay_diagnostics",
            file_identifier: "KRDG",
            // Wave B batch #3: the `flatc --swift` reader
            // (`nmp_kernel_RelayDiagnosticsSnapshot`) ships in this PR. Pure
            // field-for-field copy of the rolled-up relay rows + nested
            // wire-sub rows + logical-interest rows; every `has_*` companion
            // bool maps the optional `String?` (nil when absent). See
            // `TypedProjectionGlue.relayDiagnostics`.
            swift_reader_type: Some("nmp_kernel_RelayDiagnosticsSnapshot"),
        }),
    },
    // Pre-merged profile map (PR #812) — replaces the per-shell merge of
    // `claimed_profiles` / `author_view.profile` / `mention_profiles`. Keyed
    // by pubkey, one `ProfileCard` per profile the kernel can resolve, applying
    // the canonical precedence (claimed > author_view > mention) once in Rust
    // (`kernel/update/projections.rs`). Same Rust type as `claimed_profiles`
    // (`BTreeMap<String, ProfileCard>`), so it round-trips through the existing
    // Swift `ProfileCard` exactly like `claimed_profiles` does. Chirp reads
    // this instead of the narrower `mention_profiles` projection, which is no
    // longer in this registry (the kernel still emits it as a building block
    // for this merge — Swift just stops decoding it directly).
    SnapshotProjectionEntry {
        json_key: "resolved_profiles",
        swift_field: "resolvedProfiles",
        swift_type: "[String: ProfileCard]",
        typed_sidecar: Some(TypedSidecar {
            key: "resolved_profiles",
            schema_id: "resolved_profiles",
            file_identifier: "KRPR",
            // Profile-cluster batch: the `flatc --swift` reader
            // (`nmp_kernel_ResolvedProfilesSnapshot`, entries each carrying the
            // SHARED `nmp_kernel_ProfileCard`) ships with this batch. Flattened
            // `[{key,value}]` → `[String: ProfileCard]` with the same `has_*`
            // companion mapping as `claimed_profiles`. See
            // `TypedProjectionGlue.resolvedProfiles`.
            swift_reader_type: Some("nmp_kernel_ResolvedProfilesSnapshot"),
        }),
    },
    // Reference-first claimed-profile map — keyed by pubkey, one
    // `ProfileCard` per currently claimed UI profile. Built in
    // `kernel/update/projections.rs::snapshot_projections_with_publish_cluster`
    // by iterating `profile_claims` and calling `profile_card_for`; missing
    // kind:0 data still emits a placeholder card (D1 honest fallback).
    // Consumed by `KernelModel.profile(forPubkey:)` for the NostrProfileHost
    // conformance (`ios/Chirp/Chirp/Bridge/KernelModel.swift`).
    SnapshotProjectionEntry {
        json_key: "claimed_profiles",
        swift_field: "claimedProfiles",
        swift_type: "[String: ProfileCard]",
        typed_sidecar: Some(TypedSidecar {
            key: "claimed_profiles",
            schema_id: "claimed_profiles",
            file_identifier: "KCPR",
            // Profile-cluster batch: the `flatc --swift` reader
            // (`nmp_kernel_ClaimedProfilesSnapshot`, entries each carrying the
            // SHARED `nmp_kernel_ProfileCard`) ships with this batch. Flattened
            // `[{key,value}]` → `[String: ProfileCard]` with `has_*`→`String?`
            // companion mapping. See `TypedProjectionGlue.claimedProfiles`.
            swift_reader_type: Some("nmp_kernel_ClaimedProfilesSnapshot"),
        }),
    },
    // Reference-first claimed-event map (ADR-0034 / F-CR-06) — keyed by
    // `primary_id` (hex-64 event id for nevent/note, `kind:pubkey:d_tag`
    // coordinate for naddr), one `ClaimedEventDto` per currently claimed
    // embed/kind-registry event. Built in
    // `kernel/update/projections.rs::snapshot_projections_with_publish_cluster`
    // from the kernel's claimed-event set (see
    // `crates/nmp-core/src/kernel/types.rs::ClaimedEventDto`). The Swift
    // value type `ClaimedEventDto` is hand-declared (Stage-3 value types are
    // not schema-reflected) in `ios/Chirp/Chirp/Bridge/EmbedHost.swift`, its
    // sole consumer. Drives `EmbedHost.update(from:)` for the NMP embed
    // system.
    SnapshotProjectionEntry {
        json_key: "claimed_events",
        swift_field: "claimedEvents",
        swift_type: "[String: ClaimedEventDto]",
        typed_sidecar: Some(TypedSidecar {
            key: "claimed_events",
            schema_id: "claimed_events",
            file_identifier: "KCEV",
            // NIP-17 DM cluster batch (claimed-event map): the `flatc --swift`
            // reader (`nmp_kernel_ClaimedEventsSnapshot`, entries each carrying
            // `nmp_kernel_ClaimedEvent` + `nmp_kernel_TagRow`) ships with this
            // batch from `crates/nmp-core/schema/claimed_events.fbs`. Flattened
            // `[{key,value}]` → `[String: ClaimedEventDto]` map, mirroring the
            // `claimed_profiles` precedent; the wire's author display/picture
            // fields are NOT mapped (the hand-declared `ClaimedEventDto` in
            // `EmbedHost.swift` ignores them — field-aligned, not thick). See
            // `TypedProjectionGlue.claimedEvents`.
            swift_reader_type: Some("nmp_kernel_ClaimedEventsSnapshot"),
        }),
    },
    SnapshotProjectionEntry {
        json_key: "settings_hub",
        swift_field: "settingsHub",
        swift_type: "[String: Int]",
        // Sidecar IS emitted, but the FB table (`{ relay_count: uint }`) does
        // NOT field-align with the Chirp domain type (`[String: Int]`) — an
        // outlier shape that needs bespoke glue. Recorded for completeness;
        // `swift_reader_type: None` keeps it out of generated scope.
        typed_sidecar: Some(TypedSidecar {
            key: "settings_hub",
            schema_id: "settings_hub",
            file_identifier: "KSHB",
            swift_reader_type: None,
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
        json_key: "nmp.marmot.snapshot",
        swift_field: "marmotSnapshot",
        swift_type: "MarmotSnapshot",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.marmot.snapshot",
            schema_id: "nmp.marmot.snapshot",
            file_identifier: "NMMS",
            swift_reader_type: None,
        }),
    },
    // `nmp.marmot.messages` projects a JSON object keyed by `group_id_hex`
    // → newest-N `MarmotMessageRow` array (all groups in one map).
    // Post-convertFromSnakeCase the key is `"nmp.marmot.messages"` (no `_`).
    SnapshotProjectionEntry {
        json_key: "nmp.marmot.messages",
        swift_field: "marmotMessages",
        swift_type: "[String: [MarmotMessage]]",
        typed_sidecar: Some(TypedSidecar {
            key: "nmp.marmot.messages",
            schema_id: "nmp.marmot.messages",
            file_identifier: "NMMG",
            swift_reader_type: None,
        }),
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Locks the registry size. Anyone adding or removing an entry changes
    /// the generated Swift; this test makes that change explicit rather than
    /// silent.
    #[test]
    fn registry_size_is_locked() {
        // 35 entries: 33 prior + 2 Marmot push projections (V-107 / ADR-0039):
        // `nmp.marmot.snapshot` and `nmp.marmot.messages`. Bump this (and add
        // a new SnapshotProjectionEntry above) when a new projection is wired.
        // `bunker_connection_state` (V-14 step b) is intentionally absent —
        // the follow-up iOS PR adds it alongside the Swift Decodable stub and
        // the KernelTypes.generated.swift regen to avoid a codegen-drift CI failure.
        assert_eq!(
            SNAPSHOT_PROJECTIONS.len(),
            35,
            "registry size changed — regenerate KernelTypes.generated.swift and update this test"
        );
    }

    /// Every Swift field name must be a unique lowerCamelCase identifier.
    /// A duplicate would emit two `let` lines with the same name (Swift
    /// compile error in the generated file) — this guards against an
    /// accidental copy/paste regression.
    #[test]
    fn swift_field_names_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in SNAPSHOT_PROJECTIONS {
            assert!(
                seen.insert(entry.swift_field),
                "duplicate swift_field {:?} in SNAPSHOT_PROJECTIONS",
                entry.swift_field
            );
        }
    }

    /// Every JSON key must be unique. The kernel registers one closure per
    /// key; declaring the same key twice on the Swift side would silently
    /// shadow one decoder case with another.
    #[test]
    fn json_keys_are_unique() {
        let mut seen = std::collections::BTreeSet::new();
        for entry in SNAPSHOT_PROJECTIONS {
            assert!(
                seen.insert(entry.json_key),
                "duplicate json_key {:?} in SNAPSHOT_PROJECTIONS",
                entry.json_key
            );
        }
    }

    /// Every dotted JSON key in the conformance test
    /// (`SnapshotProjectionsConformanceTests.swift`) must be present in
    /// this registry — and vice versa for the six dotted-key entries the
    /// conformance test names. If a new dotted key is added to the
    /// conformance test, this registry must grow too (and the renderer
    /// will produce a matching `CodingKeys` case). If a dotted key is
    /// removed from the registry, the conformance test must drop the
    /// matching `XCTAssertNotNil`.
    #[test]
    fn all_dotted_keys_are_present() {
        let dotted: Vec<&str> = SNAPSHOT_PROJECTIONS
            .iter()
            .map(|e| e.json_key)
            .filter(|k| k.contains('.'))
            .collect();
        // The conformance test names eight dotted keys (six prior + 2 Marmot
        // push projections added in V-107). Hard-code them here so a drift on
        // either side fails this test loudly.
        let expected = [
            "nmp.nip29.group_chat",
            "nmp.nip29.discovered_groups",
            "nmp.nip17.dm_inbox",
            "nmp.follow_list",
            "nmp.nip57.zaps",
            "nmp.nip17.dm_relay_list",
            "nmp.marmot.snapshot",
            "nmp.marmot.messages",
        ];
        for key in expected {
            assert!(
                dotted.contains(&key),
                "dotted projection key {key:?} is in the conformance test \
                 but missing from SNAPSHOT_PROJECTIONS"
            );
        }
    }
}
