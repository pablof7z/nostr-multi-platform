use super::*;
use crate::swift_projections_registry::TypedSidecar;

/// A two-entry registry: one emitted (has a Swift reader binding), one skipped
/// (sidecar present but no reader binding yet). Mirrors the real-world split.
///
/// Note: entries with `typed_sidecar: None` are banned by the coverage gate
/// (`typed_sidecar_coverage_gate` test) so this test fixture no longer
/// includes one. The renderer still handles `None` defensively, but the
/// registry enforces that no live entry may have that state.
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
    // Buffers are trusted (in-process FFI); use unchecked getRoot, not
    // getCheckedRoot, to skip the O(N) Verifier walk on the 4 Hz hot path.
    assert!(
        out.contains("let reader: nmp_kernel_ActiveAccountSnapshot = getRoot(byteBuffer: &buffer)"),
        "must use unchecked getRoot into the named flatc reader struct; got:\n{out}"
    );
    assert!(
        !out.contains("getCheckedRoot"),
        "getCheckedRoot must not appear in generated output (use getRoot instead); got:\n{out}"
    );
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
    // #626: crate-owned NIP-29 group-create defaults (`groupDefaults` →
    // `TypedGroupDefaultsDecoder`).
    assert!(out.contains("enum TypedGroupDefaultsDecoder {"));
    // Profile key — serves via the refs.profile KPRF NRRD row-delta sidecar
    // (ADR-0063). The old claimed_profiles (KCPR) and resolved_profiles (KRPR)
    // JSON-snapshot decoders are deleted in Lane H.
    assert!(out.contains("enum TypedProfileDecoder {"));
    assert!(!out.contains("enum TypedClaimedProfilesDecoder {"), "claimed_profiles deleted — ADR-0063 Lane H");
    assert!(!out.contains("enum TypedResolvedProfilesDecoder {"), "resolved_profiles deleted — ADR-0063 Lane H");
    // NIP-17 DM cluster + claimed-event map. The enum name derives from
    // `swift_field`, so the dotted producer keys map to camelCase decoders.
    assert!(out.contains("enum TypedDmInboxDecoder {"));
    assert!(out.contains("enum TypedDmRelayListDecoder {"));
    assert!(out.contains("enum TypedClaimedEventsDecoder {"));
    // Issue #1283 Phase 1: the typed embed sidecar (`claimedEventEmbeds` →
    // `TypedClaimedEventEmbedsDecoder`, `NEMB` / `nmp_embed_ClaimedEventEmbeds`).
    assert!(out.contains("enum TypedClaimedEventEmbedsDecoder {"));
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
    // Wave C: action_results, action_stages.
    // V-112 (ADR-0042): author_view / thread_view deleted from registry.
    assert!(out.contains("enum TypedActionResultsDecoder {"));
    assert!(out.contains("enum TypedActionStagesDecoder {"));
    assert!(!out.contains("enum TypedAuthorViewDecoder {"), "author_view deleted — V-112");
    assert!(!out.contains("enum TypedThreadViewDecoder {"), "thread_view deleted — V-112");
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
        emitted, 28,
        "exactly 28 keys have a checked-in flatc --swift reader binding \
         today (accounts + active_account from PR #1039; the Wave B batch #2: \
         configured_relays, relay_role_options, outbox_summary, \
         publish_outbox, publish_queue; the Wave B batch #3: \
         relay_diagnostics, action_lifecycle; the Wave B Tier-1 #4: \
         nmp.follow_list, nmp.nip57.zaps, nmp.nip29.group_chat, \
         nmp.nip29.discovered_groups; the profile cluster: profile; \
         ADR-0063 Lane H: claimed_profiles + resolved_profiles DELETED; \
         the NIP-17 DM cluster: \
         nmp.nip17.dm_inbox, nmp.nip17.dm_relay_list, claimed_events; the \
         NIP-46 cluster: bunker_handshake, nip46_onboarding; the Marmot \
         push-projection cluster: nmp.marmot.snapshot, nmp.marmot.messages; \
         plus the wallet (producer field-add) + settings_hub (kernel built-in) \
         flips; Wave C: action_results, action_stages; signer_state \
         (ADR-0048 D6, generalised from V-14 bunker_connection_state); V-112 \
         author_view + thread_view deleted = 30 - 2 = 28; #626: \
         nmp.nip29.group_defaults = 28 + 1 = 29; #1283 Phase 1: \
         claimed_event_embeds (NEMB) = 29 + 1 = 30; ADR-0063 Lane H: \
         - 2 = 28); if this changed, \
         regenerate TypedProjectionDecoders.generated.swift and update this test"
    );
}

#[test]
fn check_typed_decoders_length_diff_reports_first_diff_line_not_none() {
    // A file that matches the rendered output on every common line but is
    // shorter (last line missing) must report a diff line, not `None` — `None`
    // is the "file missing" signal the CI reporting code keys off.
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("TypedProjectionDecoders.generated.swift");
    generate_typed_decoders(&out).expect("write");
    let full = std::fs::read_to_string(&out).expect("read");
    let line_count = full.lines().count();
    let truncated: String =
        full.lines().take(line_count - 1).collect::<Vec<_>>().join("\n") + "\n";
    std::fs::write(&out, &truncated).expect("truncate");
    let result = check_typed_decoders(&out).expect("check");
    assert!(!result.up_to_date, "truncated file must be stale");
    assert!(
        result.first_diff_line.is_some(),
        "truncated file should report a diff line, not None (which implies missing)"
    );
}
