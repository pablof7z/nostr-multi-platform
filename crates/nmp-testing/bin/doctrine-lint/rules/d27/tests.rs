use super::*;

#[test]
fn flags_short_npub_call() {
    let hits = check("    let s = short_npub(&pk);", false, false);
    assert_eq!(hits.len(), 1);
    assert!(
        hits[0].1.contains("short_npub"),
        "message must name the token"
    );
    assert!(hits[0].1.contains("ADR-0072"), "must ref ADR-0072");
}

/// #3113 / ADR-0077 — the canonical example: `to_npub` is a deterministic,
/// lossless, context-free hex<->bech32 codec, not display formatting.
/// Banning it would force every shell (native, wasm, TS) to reimplement the
/// same codec — the exact SSOT violation D27 exists to prevent. It must NOT
/// be flagged.
#[test]
fn does_not_flag_to_npub_codec_call() {
    let hits = check("        npub: to_npub(&self.pubkey),", false, false);
    assert!(
        hits.is_empty(),
        "to_npub is a canonical bech32 codec, not display formatting — must \
         not be flagged (#3113, ADR-0077); got: {:?}",
        hits
    );
}

/// Truncation (`short_npub`) is the presentation counterpart to the
/// `to_npub` codec above and must stay flagged — the boundary is drawn by
/// helper name (lossy truncation vs. lossless codec), not by "anything
/// npub-shaped".
#[test]
fn still_flags_short_npub_truncation_call() {
    let hits = check("        npub_short: short_npub(&self.pubkey),", false, false);
    assert_eq!(hits.len(), 1, "short_npub truncation must still be flagged");
    assert!(hits[0].1.contains("short_npub"));
}

#[test]
fn flags_avatar_color_hex_call() {
    let hits = check("    color: avatar_color_hex(&pk),", false, false);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("avatar_color_hex"));
}

#[test]
fn flags_format_ago_secs_call() {
    let hits = check("    ago: format_ago_secs(now, ts),", false, false);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("format_ago_secs"));
}

#[test]
fn ignores_call_in_comment() {
    let hits = check("// short_npub is banned here", true, false);
    assert!(hits.is_empty(), "comment lines must not be flagged");
}

#[test]
fn ignores_call_in_test_cfg() {
    let hits = check("    let s = short_npub(&pk);", false, true);
    assert!(hits.is_empty(), "#[cfg(test)] bodies must not be flagged");
}

#[test]
fn col_is_1_indexed_for_call() {
    let line = "let x = short_npub(&pk);";
    let hits = check(line, false, false);
    assert_eq!(hits.len(), 1);
    // "short_npub(" starts at byte offset 8.
    assert_eq!(hits[0].0, 9, "column must be 1-indexed");
}

#[test]
fn flags_kinds_label_field() {
    let hits = check("    pub kinds_label: String,", false, false);
    assert_eq!(hits.len(), 1, "kinds_label: String must be flagged");
    assert!(hits[0].1.contains("kinds_label"));
    assert!(hits[0].1.contains("ADR-0072"));
}

#[test]
fn flags_signer_label_field() {
    let hits = check("    pub signer_label: String,", false, false);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("signer_label"));
}

#[test]
fn flags_display_label_field() {
    let hits = check("    pub display_label: String,", false, false);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("display_label"));
}

#[test]
fn flags_invites_chip_label_option_field() {
    let hits = check("    pub invites_chip_label: Option<String>,", false, false);
    assert_eq!(hits.len(), 1);
    assert!(hits[0].1.contains("invites_chip_label"));
}

#[test]
fn does_not_flag_let_binding() {
    // A local `let` binding is NOT a struct field.
    let hits = check("    let kinds_label: String = compute();", false, false);
    assert!(hits.is_empty(), "let bindings must not be flagged");
}

#[test]
fn does_not_flag_struct_construction_value() {
    // Struct construction: `kinds_label: r.kinds_label().to_string()` —
    // after `:` the value starts with `r.`, not a String type prefix.
    let hits = check(
        "        kinds_label: r.kinds_label().unwrap_or_default().to_string(),",
        false,
        false,
    );
    assert!(
        hits.is_empty(),
        "struct construction with a method-call value must not be flagged"
    );
}

#[test]
fn does_not_flag_raw_display_name_field() {
    // raw_display_name ends in `_name`, not `_label` or `_display`.
    let hits = check("    pub raw_display_name: Option<String>,", false, false);
    assert!(
        hits.is_empty(),
        "raw_display_name must not be flagged (ends in _name, not _label/_display)"
    );
}

#[test]
fn does_not_flag_field_in_test_cfg() {
    let hits = check("    pub status_label: String,", false, true);
    assert!(hits.is_empty(), "#[cfg(test)] must not be flagged");
}

#[test]
fn stale_allow_points_at_the_marker() {
    // A raw field carrying a leftover D27 allow fires NO real finding...
    let line = "    pub pubkey: String, // doctrine-allow: D27 — leftover";
    assert!(
        check(line, false, false).is_empty(),
        "a raw `String` field must not fire a real D27 finding"
    );
    // ...so the driver asks for the stale-allow finding.
    let (col, msg, _suggested) = stale_allow(line).expect("marker present");
    // Column is 1-indexed and points at the `//` of the marker.
    assert_eq!(&line[col - 1..col + 1], "//");
    assert!(
        msg.contains("stale"),
        "message must call out the stale marker"
    );
    assert!(msg.contains("D27"), "message must name the rule");
}

#[test]
fn stale_allow_is_none_without_a_marker() {
    assert!(stale_allow("    pub pubkey: String,").is_none());
}

#[test]
fn legit_allow_still_fires_a_real_finding() {
    // A line with a genuine banned call still produces a real D27 finding;
    // the driver suppresses it via the allow and never reaches `stale_allow`.
    // Uses `short_npub` (presentation truncation, still banned) rather than
    // `to_npub` (canonical codec, exempt per #3113/ADR-0077) so this fixture
    // keeps testing legit-suppression, not a no-op allow.
    let line = "        npub_short: short_npub(&pk), // doctrine-allow: D27 — fixture";
    assert_eq!(
        check(line, false, false).len(),
        1,
        "the banned call must still fire so the allow has something to silence"
    );
}

#[test]
fn nip47_runtime_is_in_scope() {
    assert!(file_in_scope(Path::new("crates/nmp-nip47/src/runtime.rs")));
}

#[test]
fn nmp_marmot_projection_is_in_scope() {
    assert!(file_in_scope(Path::new(
        "crates/nmp-marmot/src/projection/payload.rs"
    )));
}

#[test]
fn marmot_projection_display_is_exempt() {
    // ADR-0072 explicitly permits these free-form name fallbacks.
    assert!(!file_in_scope(Path::new(
        "crates/nmp-marmot/src/projection/display.rs"
    )));
}

#[test]
fn nmp_core_typed_projections_in_scope() {
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/typed_projections/accounts_fb.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/actor/typed_projections/nip46_onboarding_fb.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/update/projections.rs"
    )));
}

#[test]
fn nmp_core_logging_paths_out_of_scope() {
    // kernel/status.rs uses short_hex for logging — NOT a projection file.
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/status.rs"
    )));
    // kernel/ingest/ is not a projection builder.
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/ingest/contacts.rs"
    )));
}

#[test]
fn presentation_shells_out_of_scope() {
    assert!(!file_in_scope(Path::new("crates/nmp-cli/src/main.rs")));
}

#[test]
fn generated_files_out_of_scope() {
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/typed_projections/generated/foo.rs"
    )));
}

#[test]
fn doctrine_lint_source_out_of_scope() {
    assert!(!file_in_scope(Path::new(
        "crates/nmp-testing/bin/doctrine-lint/rules/d27.rs"
    )));
}

/// #3098 — the gallery app crate's UniFFI snapshot adapter must be in
/// scope. `snapshot_json.rs::profile_card_json` baked `npub_short` straight
/// into the UniFFI wire via `short_npub(` and went uncaught because this
/// allowlist excluded `apps/*` entirely (the #3095 scanner fix only widened
/// doctrine-lint's file *walk*, not this rule's own scope allowlist).
#[test]
fn gallery_app_crate_is_in_scope() {
    assert!(file_in_scope(Path::new(
        "apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
    )));
    assert!(file_in_scope(Path::new(
        "/abs/path/apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
    )));
}

/// The widened scope, combined with the existing `check()` token scan, would
/// have caught the exact pre-fix `short_npub(`/`npub_short` bake (no
/// `doctrine-allow` marker) that shipped in #3098's violation.
///
/// #3113 / ADR-0077 correction: the sibling `npub` field's `to_npub(` call is
/// NOT part of this regression — it is the canonical bech32 codec, legitimate
/// on the wire. Only `npub_short`'s truncation was the real leak; this test
/// pins that corrected boundary (see #3110, which removed the transient
/// `doctrine-allow: D27` marker #3112 had placed on the now-exempt `to_npub`
/// call once this rule was fixed).
#[test]
fn widened_scope_catches_the_old_3098_leak() {
    assert!(file_in_scope(Path::new(
        "apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
    )));
    let old_short_npub_line = "        \"npub_short\": short_npub(pubkey),";
    let hits = check(old_short_npub_line, false, false);
    assert_eq!(
        hits.len(),
        1,
        "old short_npub( bake must fire once scope is widened"
    );
    assert!(hits[0].1.contains("short_npub"));
    let to_npub_line = "    let npub = to_npub(pubkey);";
    let hits = check(to_npub_line, false, false);
    assert!(
        hits.is_empty(),
        "to_npub is the canonical codec, not part of the #3098 leak — must \
         not fire even in the widened gallery scope (#3113, ADR-0077); got: {:?}",
        hits
    );
}
