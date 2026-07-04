// Tail half of the `SNAPSHOT_PROJECTIONS` array — split out of
// `swift_projections_registry_entries.rs` purely as a size-management seam
// (AGENTS.md 500-LOC ceiling): the array literal grew past the cap when
// `nmp.nip29.joined_groups` was added. `include!`d as a top-level item into
// the parent module, which stitches `HEAD` (there) and `TAIL` (here) back
// into one contiguous `SNAPSHOT_PROJECTIONS` slice via `concat` — order is
// load-bearing (see the parent file's doc comment) and preserved: `HEAD`
// entries always precede `TAIL` entries.
const TAIL: [SnapshotProjectionEntry; TAIL_LEN] = [
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
    // (issue #1283 / ADR-0072 §embed-sidecar) — keyed by `primary_id`, one
    // `EmbeddedEventEnvelope` (the kind-dispatched `EmbedKindProjection`) per
    // currently resolved event ref. Produced by `crates/nmp-native-runtime/src/embed_sidecar.rs`,
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
    // V-107 / ADR-0070 / ADR-0072-amended: Marmot (MLS-over-Nostr) push
    // projections. Both are registered by `nmp_marmot::install` during
    // explicit Rust composition. The runtime emits empty objects when no
    // local-key Marmot projection is active.
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
            // Host-registered typed producer in `crates/nmp-marmot/src/runtime.rs`
            // (`nmp_marmot::install` -> `crate::wire::snapshot_fb::typed_projection`).
            // Nested-vector copy of
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
            // producer in `crates/nmp-marmot/src/runtime.rs`
            // (`nmp_marmot::install` -> `crate::wire::messages_fb::typed_projection`).
            // FlatBuffers has no map
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
