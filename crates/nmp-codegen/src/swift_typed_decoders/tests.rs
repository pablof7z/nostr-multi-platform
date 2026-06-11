use super::*;
use crate::swift_projections_registry::TypedSidecar;

/// A two-entry registry: one emitted (has a Swift reader binding), one skipped
/// (sidecar present but no reader binding yet). Mirrors the real-world split.
fn mixed_registry() -> Vec<SnapshotProjectionEntry> {
    vec![
        SnapshotProjectionEntry {
            json_key: "active_account",
            swift_field: "activeAccount",
            swift_type: "String",
            typed_sidecar: Some(TypedSidecar {
                key: "active_account",
                schema_id: "active_account",
                file_identifier: "KACT",
                swift_reader_type: Some("nmp_kernel_ActiveAccountSnapshot"),
            }),
        },
        // Sidecar present, but reader binding not generated yet → skipped.
        SnapshotProjectionEntry {
            json_key: "settings_hub",
            swift_field: "settingsHub",
            swift_type: "[String: Int]",
            typed_sidecar: Some(TypedSidecar {
                key: "settings_hub",
                schema_id: "settings_hub",
                file_identifier: "KSHB",
                swift_reader_type: None,
            }),
        },
        // No sidecar at all → skipped.
        SnapshotProjectionEntry {
            json_key: "last_action_result",
            swift_field: "lastActionResult",
            swift_type: "LastActionResult",
            typed_sidecar: None,
        },
    ]
}

#[test]
fn emits_decoder_only_for_keys_with_a_reader_binding() {
    let out = render_typed_decoders(&mixed_registry());
    // The one emitted key.
    assert!(
        out.contains("enum TypedActiveAccountDecoder {"),
        "active_account (has reader) must be emitted; got:\n{out}"
    );
    // Skipped keys must NOT appear — referencing an absent reader type would
    // not compile in the Chirp target.
    assert!(
        !out.contains("TypedSettingsHubDecoder"),
        "settings_hub (no reader binding) must be skipped"
    );
    assert!(
        !out.contains("TypedLastActionResultDecoder"),
        "last_action_result (no sidecar) must be skipped"
    );
}

#[test]
fn emitted_decoder_carries_the_sidecar_identity_constants() {
    let out = render_typed_decoders(&mixed_registry());
    assert!(out.contains("static let key = \"active_account\"\n"));
    assert!(out.contains("static let schemaId = \"active_account\"\n"));
    assert!(out.contains("static let fileIdentifier = \"KACT\"\n"));
}

#[test]
fn emitted_decoder_matches_envelope_by_key_and_schema_id() {
    let out = render_typed_decoders(&mixed_registry());
    // The lookup predicate is the load-bearing contract (mirrors
    // TypedHomeFeedDecoder). Both clauses must be present.
    assert!(out.contains("$0.key == key && $0.schemaId == schemaId"));
}

#[test]
fn emitted_decoder_decodes_into_the_flatc_reader_type() {
    let out = render_typed_decoders(&mixed_registry());
    assert!(
        out.contains("guard let reader: nmp_kernel_ActiveAccountSnapshot = try? getCheckedRoot("),
        "must getCheckedRoot into the named flatc reader struct; got:\n{out}"
    );
    assert!(out.contains("fileId: fileIdentifier"));
}

#[test]
fn emitted_decoder_returns_domain_type_and_delegates_to_glue() {
    let out = render_typed_decoders(&mixed_registry());
    // Domain return type from swift_type.
    assert!(out.contains("static func decode(from projections: [TypedProjectionEnvelope]) -> String? {"));
    assert!(out.contains("static func decode(bytes: Data) -> String? {"));
    // Hand-written glue seam — NOT generated, called by name.
    assert!(
        out.contains("return TypedProjectionGlue.activeAccount(reader)"),
        "must delegate the reader→domain mapping to the hand-written glue; got:\n{out}"
    );
}

#[test]
fn output_is_deterministic() {
    let a = render_typed_decoders(&mixed_registry());
    let b = render_typed_decoders(&mixed_registry());
    assert_eq!(a, b, "renderer must be byte-deterministic for the --check gate");
}

#[test]
fn output_ends_with_single_newline() {
    let out = render_typed_decoders(&mixed_registry());
    assert!(out.ends_with('\n'), "file must end with a newline");
    assert!(!out.ends_with("\n\n"), "file must not end with a blank line");
}

#[test]
fn empty_when_no_reader_bindings() {
    let entries = vec![SnapshotProjectionEntry {
        json_key: "settings_hub",
        swift_field: "settingsHub",
        swift_type: "[String: Int]",
        typed_sidecar: Some(TypedSidecar {
            key: "settings_hub",
            schema_id: "settings_hub",
            file_identifier: "KSHB",
            swift_reader_type: None,
        }),
    }];
    let out = render_typed_decoders(&entries);
    assert!(out.contains("No projection key has a checked-in"));
    assert!(!out.contains("enum Typed"));
}

#[test]
fn decoder_enum_name_capitalizes_first_letter() {
    assert_eq!(decoder_enum_name("accounts"), "TypedAccountsDecoder");
    assert_eq!(decoder_enum_name("activeAccount"), "TypedActiveAccountDecoder");
    assert_eq!(decoder_enum_name("wallet"), "TypedWalletDecoder");
}

/// The real registry must emit decoders for EXACTLY the keys whose `flatc
/// --swift` binding is checked into the Chirp target today: the two proof keys
/// (`accounts`, `active_account`, PR #1039), the Wave B batch #2 thin-glue
/// keys (`configured_relays`, `relay_role_options`, `outbox_summary`,
/// `publish_outbox`, `publish_queue`), the Wave B batch #3 diagnostics +
/// action-lifecycle keys (`relay_diagnostics`, `action_lifecycle`), plus the
/// Wave B Tier-1 #4 app-projection keys (`nmp.follow_list`, `nmp.nip57.zaps`,
/// `nmp.nip29.group_chat`, `nmp.nip29.discovered_groups`). If a future PR adds a
/// reader binding to another entry, this test fails loudly — a reminder to
/// regenerate the Swift and update this expectation.
#[test]
fn real_registry_emits_exactly_the_proof_keys() {
    let out = render_typed_decoders(SNAPSHOT_PROJECTIONS);
    // PR #1039 proof keys.
    assert!(out.contains("enum TypedAccountsDecoder {"));
    assert!(out.contains("enum TypedActiveAccountDecoder {"));
    // Wave B batch #2 thin-glue keys.
    assert!(out.contains("enum TypedConfiguredRelaysDecoder {"));
    assert!(out.contains("enum TypedRelayRoleOptionsDecoder {"));
    assert!(out.contains("enum TypedOutboxSummaryDecoder {"));
    assert!(out.contains("enum TypedPublishOutboxDecoder {"));
    assert!(out.contains("enum TypedPublishQueueDecoder {"));
    // Wave B batch #3 diagnostics + action-lifecycle keys.
    assert!(out.contains("enum TypedRelayDiagnosticsDecoder {"));
    assert!(out.contains("enum TypedActionLifecycleDecoder {"));
    // Wave B Tier-1 #4 app-projection keys (dotted producer keys; the enum name
    // derives from `swift_field`, so `followList` → `TypedFollowListDecoder`).
    assert!(out.contains("enum TypedFollowListDecoder {"));
    assert!(out.contains("enum TypedZapsDecoder {"));
    assert!(out.contains("enum TypedGroupChatDecoder {"));
    assert!(out.contains("enum TypedDiscoveredGroupsDecoder {"));
    // Profile-cluster keys — all three share the `nmp_kernel_ProfileCard` reader
    // (defined once via the shared `profile_card.fbs` include).
    assert!(out.contains("enum TypedProfileDecoder {"));
    assert!(out.contains("enum TypedClaimedProfilesDecoder {"));
    assert!(out.contains("enum TypedResolvedProfilesDecoder {"));
    // NIP-17 DM cluster + claimed-event map. The enum name derives from
    // `swift_field`, so the dotted producer keys map to camelCase decoders.
    assert!(out.contains("enum TypedDmInboxDecoder {"));
    assert!(out.contains("enum TypedDmRelayListDecoder {"));
    assert!(out.contains("enum TypedClaimedEventsDecoder {"));
    // NIP-46 per-key sidecar flips. The enum name derives from `swift_field`.
    assert!(out.contains("enum TypedBunkerHandshakeDecoder {"));
    assert!(out.contains("enum TypedNip46OnboardingDecoder {"));
    // Marmot push-projection cluster. The enum name derives from `swift_field`.
    assert!(out.contains("enum TypedMarmotSnapshotDecoder {"));
    assert!(out.contains("enum TypedMarmotMessagesDecoder {"));
    // Wallet (producer field-add) + settings-hub (kernel built-in) flips. The
    // enum name derives from `swift_field` (`wallet` / `settingsHub`).
    assert!(out.contains("enum TypedWalletDecoder {"));
    assert!(out.contains("enum TypedSettingsHubDecoder {"));
    // Wave C: action_results, action_stages, author_view, thread_view.
    // Enum names derive from `swift_field`.
    assert!(out.contains("enum TypedActionResultsDecoder {"));
    assert!(out.contains("enum TypedActionStagesDecoder {"));
    assert!(out.contains("enum TypedAuthorViewDecoder {"));
    assert!(out.contains("enum TypedThreadViewDecoder {"));
    let emitted = SNAPSHOT_PROJECTIONS
        .iter()
        .filter(|e| {
            e.typed_sidecar
                .as_ref()
                .and_then(|s| s.swift_reader_type)
                .is_some()
        })
        .count();
    assert_eq!(
        emitted, 29,
        "exactly twenty-nine keys have a checked-in flatc --swift reader binding \
         today (accounts + active_account from PR #1039; the Wave B batch #2: \
         configured_relays, relay_role_options, outbox_summary, \
         publish_outbox, publish_queue; the Wave B batch #3: \
         relay_diagnostics, action_lifecycle; the Wave B Tier-1 #4: \
         nmp.follow_list, nmp.nip57.zaps, nmp.nip29.group_chat, \
         nmp.nip29.discovered_groups; the profile cluster: profile, \
         claimed_profiles, resolved_profiles; the NIP-17 DM cluster: \
         nmp.nip17.dm_inbox, nmp.nip17.dm_relay_list, claimed_events; the \
         NIP-46 cluster: bunker_handshake, nip46_onboarding; the Marmot \
         push-projection cluster: nmp.marmot.snapshot, nmp.marmot.messages; \
         plus the wallet (producer field-add) + settings_hub (kernel built-in) \
         flips; Wave C: action_results, action_stages, author_view, \
         thread_view); if this changed, regenerate \
         TypedProjectionDecoders.generated.swift and update this test"
    );
}
