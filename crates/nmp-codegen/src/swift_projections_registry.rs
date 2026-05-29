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
}

/// The Stage 2 registry — every entry on the hand-written
/// `SnapshotProjections` struct in `KernelBridge.swift`, in declaration
/// order. Order is load-bearing (the generated file is byte-diffed against
/// the committed copy by the `codegen-drift` CI gate).
///
/// The hand-written declaration carries 32 fields; this slice has 32
/// entries. Adding or removing a member here changes the generated Swift —
/// the CI gate will refuse stale output until the regenerated file is
/// committed.
pub const SNAPSHOT_PROJECTIONS: &[SnapshotProjectionEntry] = &[
    // Built-in NWC wallet projection. `projections["wallet"]`.
    SnapshotProjectionEntry {
        json_key: "wallet",
        swift_field: "wallet",
        swift_type: "WalletStatusData",
    },
    // NIP-46 bunker handshake projection. `projections["bunker_handshake"]`.
    SnapshotProjectionEntry {
        json_key: "bunker_handshake",
        swift_field: "bunkerHandshake",
        swift_type: "BunkerHandshake",
    },
    // NIP-46 typed onboarding read model. Always populated by the kernel;
    // optional only so an older kernel build that predates the projection
    // still decodes (D1).
    SnapshotProjectionEntry {
        json_key: "nip46_onboarding",
        swift_field: "nip46Onboarding",
        swift_type: "Nip46Onboarding",
    },
    // Publish-cluster outbox feeds — kernel-owned `publish_queue` and
    // `publish_outbox` arrays driven by the actor publish path.
    SnapshotProjectionEntry {
        json_key: "publish_queue",
        swift_field: "publishQueue",
        swift_type: "[PublishQueueEntry]",
    },
    SnapshotProjectionEntry {
        json_key: "publish_outbox",
        swift_field: "publishOutbox",
        swift_type: "[PublishOutboxItem]",
    },
    // §6/AP1 pre-formatted outbox header — kernel-owned strings the shell
    // renders verbatim.
    SnapshotProjectionEntry {
        json_key: "outbox_summary",
        swift_field: "outboxSummary",
        swift_type: "OutboxSummary",
    },
    // Relay-edit settings cluster — pre-rolled rows + role pick options.
    SnapshotProjectionEntry {
        json_key: "relay_edit_rows",
        swift_field: "relayEditRows",
        swift_type: "[RelayEditRow]",
    },
    SnapshotProjectionEntry {
        json_key: "relay_role_options",
        swift_field: "relayRoleOptions",
        swift_type: "[RelayRoleOption]",
    },
    // D0 identity output. `accounts` enriches AccountSummary rows with
    // kind:0 metadata; `active_account` is the active pubkey scalar.
    SnapshotProjectionEntry {
        json_key: "accounts",
        swift_field: "accounts",
        swift_type: "[AccountSummary]",
    },
    SnapshotProjectionEntry {
        json_key: "active_account",
        swift_field: "activeAccount",
        swift_type: "String",
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
    },
    SnapshotProjectionEntry {
        json_key: "last_action_result",
        swift_field: "lastActionResult",
        swift_type: "LastActionResult",
    },
    SnapshotProjectionEntry {
        json_key: "action_stages",
        swift_field: "actionStages",
        swift_type: "[String: [ActionStageEntry]]",
    },
    SnapshotProjectionEntry {
        json_key: "action_lifecycle",
        swift_field: "actionLifecycle",
        swift_type: "ActionLifecycleSnapshot",
    },
    // D0 views cluster — `profile`, `timeline`, `author_view`,
    // `thread_view`, plus the per-tick `inserted` / `updated` / `removed`
    // timeline deltas.
    SnapshotProjectionEntry {
        json_key: "profile",
        swift_field: "profile",
        swift_type: "ProfileCard",
    },
    SnapshotProjectionEntry {
        json_key: "timeline",
        swift_field: "timeline",
        swift_type: "[TimelineItem]",
    },
    SnapshotProjectionEntry {
        json_key: "nmp.feed.home",
        swift_field: "homeFeed",
        swift_type: "ChirpTimelineSnapshot",
    },
    SnapshotProjectionEntry {
        json_key: "author_view",
        swift_field: "authorView",
        swift_type: "AuthorProfileSnapshot",
    },
    SnapshotProjectionEntry {
        json_key: "thread_view",
        swift_field: "threadView",
        swift_type: "ThreadView",
    },
    SnapshotProjectionEntry {
        json_key: "inserted",
        swift_field: "inserted",
        swift_type: "[TimelineItem]",
    },
    SnapshotProjectionEntry {
        json_key: "updated",
        swift_field: "updated",
        swift_type: "[TimelineItem]",
    },
    SnapshotProjectionEntry {
        json_key: "removed",
        swift_field: "removed",
        swift_type: "[String]",
    },
    // Host-registered dotted-key projections. The `.` in the JSON key is
    // opaque to `.convertFromSnakeCase` (it splits on `_` only), so the
    // post-transform key keeps the `nmp.<nip>.<verb>` shape but with the
    // tail camelised.
    SnapshotProjectionEntry {
        json_key: "nmp.nip29.group_chat",
        swift_field: "groupChat",
        swift_type: "GroupChatSnapshot",
    },
    SnapshotProjectionEntry {
        json_key: "nmp.nip17.dm_inbox",
        swift_field: "dmInbox",
        swift_type: "DmInboxSnapshot",
    },
    SnapshotProjectionEntry {
        json_key: "nmp.follow_list",
        swift_field: "followList",
        swift_type: "FollowListSnapshot",
    },
    SnapshotProjectionEntry {
        json_key: "nmp.nip29.discovered_groups",
        swift_field: "discoveredGroups",
        swift_type: "DiscoveredGroupsSnapshot",
    },
    // `nmp.nip57.zaps` has no `_`, so the post-transform key is identical
    // — but declaring the `CodingKeys` enum overrides synthesised raw
    // values, so the case still needs the explicit literal.
    SnapshotProjectionEntry {
        json_key: "nmp.nip57.zaps",
        swift_field: "zaps",
        swift_type: "ZapsAggregateSnapshot",
    },
    SnapshotProjectionEntry {
        json_key: "nmp.nip17.dm_relay_list",
        swift_field: "dmRelayList",
        swift_type: "DmRelayListSnapshot",
    },
    // Diagnostics roll-up + pre-merged resolved-profile map + settings hub
    // view. All single-component snake_case keys that pass through
    // `.convertFromSnakeCase` cleanly.
    SnapshotProjectionEntry {
        json_key: "relay_diagnostics",
        swift_field: "relayDiagnostics",
        swift_type: "RelayDiagnosticsSnapshot",
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
    },
    SnapshotProjectionEntry {
        json_key: "settings_hub",
        swift_field: "settingsHub",
        swift_type: "SettingsHubSummary",
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
        // 33 entries: the original 32 plus the `claimed_events` projection
        // (ADR-0034 / F-CR-06 NMP embed system). Bump this (and add a new
        // SnapshotProjectionEntry above) when a new projection is wired.
        assert_eq!(
            SNAPSHOT_PROJECTIONS.len(),
            33,
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
        // The conformance test names six dotted keys. Hard-code them here
        // so a drift on either side fails this test loudly.
        let expected = [
            "nmp.nip29.group_chat",
            "nmp.nip29.discovered_groups",
            "nmp.nip17.dm_inbox",
            "nmp.follow_list",
            "nmp.nip57.zaps",
            "nmp.nip17.dm_relay_list",
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
