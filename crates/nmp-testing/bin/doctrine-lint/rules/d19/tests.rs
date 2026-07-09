use super::*;

// -- check() unit tests ---------------------------------------------------

#[test]
fn flags_short_npub_presentation_helper_in_prod() {
    let hits = check("    let s = crate::display::short_npub(pk);", false, false);
    assert_eq!(
        hits.len(),
        1,
        "must flag crate::display::short_npub (presentation truncation) in prod code"
    );
    assert!(
        hits[0].1.contains("ADR-0072"),
        "message must reference ADR-0072; got: {}",
        hits[0].1
    );
}

#[test]
fn flags_other_presentation_helpers_in_prod() {
    for token in [
        "crate::display::short_hex(",
        "crate::display::avatar_initials(",
        "crate::display::display_name_initials(",
        "crate::display::avatar_color_hex(",
        "crate::display::format_ago_secs(",
    ] {
        let line = format!("    let x = {token}pk);");
        let hits = check(&line, false, false);
        assert_eq!(hits.len(), 1, "must flag `{token}` in prod code");
    }
}

/// #3113 / ADR-0077 — the canonical example: `to_npub` is a deterministic,
/// lossless, context-free hex<->bech32 codec, not display formatting.
/// Calling it (even fully qualified as `crate::display::to_npub`) in a
/// kernel projection builder must NOT fire D19.
#[test]
fn does_not_flag_to_npub_codec_call() {
    let hits = check("    let npub = crate::display::to_npub(pk);", false, false);
    assert!(
        hits.is_empty(),
        "to_npub is a canonical bech32 codec, not display formatting — must \
         not be flagged (#3113, ADR-0077); got: {:?}",
        hits
    );
}

/// Every entry of the codec allowlist is exempt — proven here for a second
/// codec (`to_bech32`) so the allowlist, not a single hard-coded name, is
/// what grants the exemption.
#[test]
fn does_not_flag_to_bech32_codec_call() {
    let hits = check("    let s = crate::display::to_bech32(id);", false, false);
    assert!(
        hits.is_empty(),
        "to_bech32 is an allowlisted canonical codec — must not be flagged; \
         got: {:?}",
        hits
    );
}

/// THE DURABILITY POINT (ADR-0077, S12 fragility): a NEW `crate::display::`
/// helper that nobody has added to any banned list — invented here as
/// `avatar_shape` — must be caught BY DEFAULT because D19 allowlists only the
/// codecs and bans the open-ended presentation side. If this ever stops
/// firing, the gate has regressed to enumerate-the-banned and the next
/// presentation helper will leak silently.
#[test]
fn flags_new_display_helper_by_default() {
    let hits = check("    let x = crate::display::avatar_shape(pk);", false, false);
    assert_eq!(
        hits.len(),
        1,
        "a NEW crate::display:: helper not in the codec allowlist must be \
         flagged by default (catch-by-default; #3113, ADR-0077)"
    );
    assert!(
        hits[0].1.contains("avatar_shape"),
        "message must name the offending helper; got: {}",
        hits[0].1
    );
    assert!(
        hits[0].1.contains("ADR-0072"),
        "message must reference ADR-0072; got: {}",
        hits[0].1
    );
}

#[test]
fn flags_format_timestamp_in_prod() {
    let hits = check(
        "    let ts = format_timestamp(row.created_at);",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "must flag format_timestamp in prod code");
    assert!(
        hits[0].1.contains("ADR-0072"),
        "message must reference ADR-0072"
    );
}

#[test]
fn does_not_flag_comment_lines() {
    let hits = check("// crate::display::short_npub is banned here", true, false);
    assert!(hits.is_empty(), "comment lines must not be flagged");
}

#[test]
fn does_not_flag_in_test_cfg() {
    let hits = check("    let s = crate::display::short_npub(pk);", false, true);
    assert!(
        hits.is_empty(),
        "#[cfg(test)] bodies must not be flagged by D19"
    );
}

#[test]
fn col_is_1_indexed() {
    let line = "let x = crate::display::short_npub(pk);";
    let hits = check(line, false, false);
    assert_eq!(hits.len(), 1);
    // "crate::display::short_npub(" starts at byte offset 8 (0-indexed).
    assert_eq!(hits[0].0, 9, "column must be 1-indexed");
}

#[test]
fn flags_two_occurrences_same_line() {
    let hits = check(
        "a(crate::display::short_npub(x), crate::display::short_hex(y))",
        false,
        false,
    );
    assert_eq!(
        hits.len(),
        2,
        "both occurrences on same line must be flagged"
    );
}

#[test]
fn flags_legacy_error_toast_in_prod() {
    let hits = check(
        "    kernel.set_last_error_toast(Some(\"boom\".to_string()));",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "must flag English-only error toasts");
    assert!(
        hits[0].1.contains("UiToken"),
        "message must point to UiToken; got: {}",
        hits[0].1
    );
}

// -- file_in_scope unit tests ---------------------------------------------

#[test]
fn projection_files_are_in_scope() {
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/update/projections.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/update/views.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/types.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/publish_outbox.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/actor/commands/identity/account_ops.rs"
    )));
    assert!(file_in_scope(Path::new(
        "crates/nmp-core/src/actor/dispatch/cmd_publish.rs"
    )));
    // Absolute path variant.
    assert!(file_in_scope(Path::new(
        "/abs/path/crates/nmp-core/src/kernel/update/projections.rs"
    )));
}

#[test]
fn non_projection_files_are_out_of_scope() {
    // nostr.rs (where format_timestamp lives) is NOT a projection builder.
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/nostr.rs"
    )));
    // display module itself is out of scope.
    assert!(!file_in_scope(Path::new("crates/nmp-core/src/display.rs")));
    // Other kernel files.
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/kernel/mod.rs"
    )));
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/actor/dispatch/mod.rs"
    )));
    assert!(!file_in_scope(Path::new(
        "crates/nmp-core/src/substrate/protocol/capabilities.rs"
    )));
    // Protocol crates.
    assert!(!file_in_scope(Path::new("crates/nmp-nip17/src/lib.rs")));
}

#[test]
fn doctrine_lint_source_is_out_of_scope() {
    assert!(!file_in_scope(Path::new(
        "crates/nmp-testing/bin/doctrine-lint/rules/d19.rs"
    )));
}

/// #3098 — the gallery app crate's UniFFI snapshot adapter must be in
/// scope so a re-introduced `crate::display::`/`format_timestamp(` bake
/// into that wire red-fails CI going forward.
#[test]
fn gallery_app_crate_is_in_scope() {
    assert!(file_in_scope(Path::new(
        "apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
    )));
    assert!(file_in_scope(Path::new(
        "/abs/path/apps/nmp-gallery/crates/nmp-app-gallery/src/snapshot_json.rs"
    )));
}
