use super::*;

/// Minimal one-type document — covers the "no nested objects, mixed
/// optional/required, snake↔camel transform" case.
fn one_type_document() -> &'static str {
    r#"{
      "version": 1,
      "types": [
        {
          "rust_path": "nmp_core::demo::Sample",
          "swift_name": "Sample",
          "id_field": "id",
          "conformances": ["Decodable", "Equatable"],
          "schema": {
            "type": "object",
            "title": "Sample",
            "properties": {
              "id": { "type": "string" },
              "open_views": { "type": "integer", "format": "uint32", "minimum": 0 },
              "first_event_ms": { "type": ["integer", "null"], "format": "uint128" },
              "relay_urls": { "type": "array", "items": { "type": "string" } },
              "denied": { "type": "boolean" }
            },
            "required": ["id", "open_views", "denied", "relay_urls"]
          }
        }
      ]
    }"#
}

#[test]
fn renders_one_type_with_required_and_optional_fields() {
    let out = render_swift(one_type_document()).expect("renders");
    // Per-field expectations — assert the exact lines rather than
    // matching against a golden file, so test failures point at the
    // emitter rule that broke.
    assert!(out.contains("public struct Sample: Decodable, Equatable, Identifiable {"));
    // `id` field with literal name — synthesised Identifiable picks
    // it up; no extra `var id: String { id }` should appear.
    assert!(out.contains("    public let id: String\n"));
    assert!(
        !out.contains("public var id: String { id }"),
        "literal `id` field should NOT get a computed accessor"
    );
    assert!(out.contains("    public let openViews: UInt32\n"));
    // Optional field — `first_event_ms` is NOT in required, so `?`.
    assert!(out.contains("    public let firstEventMs: UInt64?\n"));
    // Array of strings.
    assert!(out.contains("    public let relayUrls: [String]\n"));
    assert!(out.contains("    public let denied: Bool\n"));
    // PR #358 regression guard — see `tests/swift_codegen_regression.rs`
    // for the exhaustive set. Stage 1 must not emit `CodingKeys` (the
    // decoder uses `.convertFromSnakeCase`; explicit raw values would
    // double-transform to KEY_NOT_FOUND). Stage 2 SnapshotProjections
    // DOES legitimately emit `CodingKeys`, so we scope to everything
    // before that section's marker.
    let stage1 = out.split("// MARK: - SnapshotProjections").next().unwrap_or(&out);
    assert!(!stage1.contains("CodingKeys"), "Stage 1 must not emit CodingKeys");
    assert!(!stage1.contains("= \"open_views\""), "no snake_case rawValues in Stage 1");
}

#[test]
fn identifiable_with_non_id_field_emits_computed_accessor() {
    let doc = r#"{
      "version": 1,
      "types": [
        {
          "rust_path": "demo::Row",
          "swift_name": "Row",
          "id_field": "key",
          "conformances": ["Decodable", "Equatable"],
          "schema": {
            "type": "object",
            "properties": { "key": { "type": "string" } },
            "required": ["key"]
          }
        }
      ]
    }"#;
    let out = render_swift(doc).expect("renders");
    assert!(out.contains("public struct Row: Decodable, Equatable, Identifiable {"));
    assert!(out.contains("public var id: String { key }"));
}

#[test]
fn rejects_unknown_document_version() {
    let doc = r#"{ "version": 999, "types": [] }"#;
    let err = render_swift(doc).expect_err("must reject unknown version");
    assert!(matches!(
        err,
        SwiftEmitError::UnsupportedDocumentVersion { found: 999, expected: 1 }
    ));
}

#[test]
fn rejects_non_object_root() {
    // Stage 1 must NOT silently render a tagged enum (its root schema
    // is `oneOf`, no `"type": "object"`). The error must name the
    // type so a future Stage 2/3 author knows what to migrate.
    let doc = r#"{
      "version": 1,
      "types": [
        {
          "rust_path": "demo::Tag",
          "swift_name": "Tag",
          "id_field": null,
          "conformances": ["Decodable", "Equatable"],
          "schema": { "oneOf": [{ "type": "object" }] }
        }
      ]
    }"#;
    let err = render_swift(doc).expect_err("rejects non-object root");
    match err {
        SwiftEmitError::Unsupported { swift_name, .. } => {
            assert_eq!(swift_name, "Tag");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn snake_to_camel_handles_common_shapes() {
    assert_eq!(snake_to_camel("relay_url"), "relayUrl");
    assert_eq!(snake_to_camel("first_event_ms"), "firstEventMs");
    assert_eq!(snake_to_camel("id"), "id");
    assert_eq!(snake_to_camel("a_b_c"), "aBC");
    // Already camelCase passes through unchanged.
    assert_eq!(snake_to_camel("alreadyCamel"), "alreadyCamel");
}

#[test]
fn integer_format_mapping_matches_chirp_convention() {
    assert_eq!(map_integer_format(Some("uint32")), "UInt32");
    assert_eq!(map_integer_format(Some("uint64")), "UInt64");
    // `usize` (schemars `uint`) is the Swift-side `Int` for counts.
    assert_eq!(map_integer_format(Some("uint")), "Int");
    // `u128` collapses to `UInt64` — see `map_integer_format` doc.
    assert_eq!(map_integer_format(Some("uint128")), "UInt64");
    assert_eq!(map_integer_format(Some("int64")), "Int");
    // Unknown format → Int (safe default for any integer schemars emits).
    assert_eq!(map_integer_format(None), "Int");
}

#[test]
fn check_swift_returns_up_to_date_on_match() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.swift");
    generate_swift(one_type_document(), &out).expect("write");
    let result = check_swift(one_type_document(), &out).expect("check");
    assert!(result.up_to_date);
    assert_eq!(result.first_diff_line, None);
}

#[test]
fn check_swift_flags_stale_file() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("out.swift");
    std::fs::write(&out, "// stale\n").expect("write");
    let result = check_swift(one_type_document(), &out).expect("check");
    assert!(!result.up_to_date);
    assert_eq!(result.first_diff_line, Some(1));
}

#[test]
fn check_swift_treats_missing_file_as_stale() {
    let dir = tempfile::tempdir().expect("tempdir");
    let out = dir.path().join("never_written.swift");
    let result = check_swift(one_type_document(), &out).expect("check");
    assert!(!result.up_to_date);
    assert_eq!(result.first_diff_line, None);
}

// ── V6 Stage 2 ──────────────────────────────────────────────────────────
//
// Tests for the `SnapshotProjections` registry render. These cover the
// three load-bearing pieces independently — the
// `.convertFromSnakeCase` algorithm, single-entry rendering, and the
// full-registry render that the CI gate diffs against the committed
// file.

#[test]
fn post_convert_handles_single_word_pass_through() {
    // No `_`, no `.` → strategy is a no-op. Covers `wallet`, `profile`,
    // `timeline`, `accounts`, etc.
    assert_eq!(post_convert_from_snake_case("wallet"), "wallet");
    assert_eq!(post_convert_from_snake_case("profile"), "profile");
    assert_eq!(post_convert_from_snake_case("zaps"), "zaps");
}

#[test]
fn post_convert_camelises_snake_case() {
    // Standard snake_case → camelCase. Covers the bulk of the
    // built-in projection keys.
    assert_eq!(post_convert_from_snake_case("bunker_handshake"), "bunkerHandshake");
    assert_eq!(post_convert_from_snake_case("publish_queue"), "publishQueue");
    assert_eq!(post_convert_from_snake_case("active_account"), "activeAccount");
    assert_eq!(post_convert_from_snake_case("relay_diagnostics"), "relayDiagnostics");
}

#[test]
fn post_convert_leaves_dots_opaque() {
    // `.` is NOT a separator for `.convertFromSnakeCase`; only `_` is.
    // The dotted host-registered keys camelise per segment.
    assert_eq!(
        post_convert_from_snake_case("nmp.nip29.group_chat"),
        "nmp.nip29.groupChat"
    );
    assert_eq!(
        post_convert_from_snake_case("nmp.nip17.dm_inbox"),
        "nmp.nip17.dmInbox"
    );
    assert_eq!(
        post_convert_from_snake_case("nmp.nip29.discovered_groups"),
        "nmp.nip29.discoveredGroups"
    );
    assert_eq!(
        post_convert_from_snake_case("nmp.nip17.dm_relay_list"),
        "nmp.nip17.dmRelayList"
    );
    // `nmp.follow_list` — only the tail `follow_list` carries an `_`.
    assert_eq!(
        post_convert_from_snake_case("nmp.follow_list"),
        "nmp.followList"
    );
    // `nmp.nip57.zaps` — no `_` anywhere. Strategy returns it
    // unchanged. The renderer must STILL emit an explicit raw value
    // because declaring `CodingKeys` overrides synthesis — the
    // synthesised default for the Swift property `zaps` would be the
    // bare string `"zaps"`, which doesn't match the dotted kernel key.
    assert_eq!(
        post_convert_from_snake_case("nmp.nip57.zaps"),
        "nmp.nip57.zaps"
    );
}

#[test]
fn render_snapshot_projections_emits_one_field_and_one_case_per_entry() {
    // Three-entry hand-rolled registry covers the three case shapes:
    // single-word (`wallet`), snake_case → camelCase (`bunker_handshake`),
    // and dotted (`nmp.nip29.group_chat`).
    let entries = vec![
        SnapshotProjectionEntry {
            json_key: "wallet",
            swift_field: "wallet",
            swift_type: "WalletStatusData",
        },
        SnapshotProjectionEntry {
            json_key: "bunker_handshake",
            swift_field: "bunkerHandshake",
            swift_type: "BunkerHandshake",
        },
        SnapshotProjectionEntry {
            json_key: "nmp.nip29.group_chat",
            swift_field: "groupChat",
            swift_type: "GroupChatSnapshot",
        },
    ];
    let mut out = String::new();
    render_snapshot_projections(&entries, &mut out);

    // Struct header + per-field optional declaration.
    assert!(out.contains("struct SnapshotProjections: Decodable, Equatable {"));
    assert!(out.contains("    let wallet: WalletStatusData?\n"));
    assert!(out.contains("    let bunkerHandshake: BunkerHandshake?\n"));
    assert!(out.contains("    let groupChat: GroupChatSnapshot?\n"));

    // CodingKeys enum.
    assert!(out.contains("    enum CodingKeys: String, CodingKey {\n"));
    // `wallet`: post-transform equals the Swift field → no raw value.
    assert!(out.contains("        case wallet\n"));
    // `bunker_handshake`: post-transform `bunkerHandshake` matches the
    // Swift field → no raw value.
    assert!(out.contains("        case bunkerHandshake\n"));
    assert!(
        !out.contains("case bunkerHandshake = \"bunker_handshake\""),
        "snake_case keys whose camelCase post-transform matches the Swift field MUST not carry an explicit raw value"
    );
    // `nmp.nip29.group_chat`: post-transform `nmp.nip29.groupChat`
    // differs from the Swift field `groupChat` → explicit raw value.
    assert!(out.contains("        case groupChat = \"nmp.nip29.groupChat\"\n"));
}

#[test]
fn render_snapshot_projections_emits_explicit_raw_for_dotted_no_underscore_key() {
    // The `zaps` trap: `nmp.nip57.zaps` has no `_`, so the strategy
    // returns it unchanged. The synthesised default for property
    // `zaps` would be `"zaps"`, which doesn't match the dotted key.
    // The renderer MUST emit an explicit `= "nmp.nip57.zaps"` raw
    // value because post-transform `"nmp.nip57.zaps"` != swift field
    // `"zaps"`.
    let entries = vec![SnapshotProjectionEntry {
        json_key: "nmp.nip57.zaps",
        swift_field: "zaps",
        swift_type: "ZapsAggregateSnapshot",
    }];
    let mut out = String::new();
    render_snapshot_projections(&entries, &mut out);
    assert!(
        out.contains("        case zaps = \"nmp.nip57.zaps\"\n"),
        "dotted no-underscore key MUST emit explicit raw value; got:\n{out}"
    );
}

#[test]
fn render_swift_appends_snapshot_projections_section_after_pilot_types() {
    // The full pipeline: a Stage 1 document renders the seven pilot
    // types AND the Stage 2 SnapshotProjections at the bottom. The
    // CI gate diffs the whole file, so the section order is
    // load-bearing.
    let out = render_swift(one_type_document()).expect("renders");
    // Stage 1 output is still there.
    assert!(out.contains("public struct Sample: Decodable, Equatable, Identifiable {"));
    // Stage 2 SnapshotProjections is appended after.
    assert!(out.contains("struct SnapshotProjections: Decodable, Equatable {"));
    let sample_pos = out
        .find("public struct Sample:")
        .expect("Stage 1 Sample present");
    let snap_pos = out
        .find("struct SnapshotProjections:")
        .expect("Stage 2 SnapshotProjections present");
    assert!(
        snap_pos > sample_pos,
        "Stage 2 SnapshotProjections must follow Stage 1 types"
    );
}
