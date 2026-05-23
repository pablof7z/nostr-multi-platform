//! Regression tests for the V6 Stage 1 Swift emitter.
//!
//! ## Why this file exists
//!
//! PR #358 shipped V6 Stage 1 (the `nmp-codegen` Swift `Decodable` emitter
//! pilot). For every Stage 1 type the emitter wrote an explicit `CodingKeys`
//! enum with snake_case raw values:
//!
//! ```swift
//! enum CodingKeys: String, CodingKey {
//!     case actorQueueDepth = "actor_queue_depth"
//!     case relayUrl        = "relay_url"
//!     // ...
//! }
//! ```
//!
//! The iOS shell's `KernelBridge.decode()` sets
//! `JSONDecoder.keyDecodingStrategy = .convertFromSnakeCase`, which
//! transforms incoming JSON keys to camelCase BEFORE `Codable` matches them
//! against the `CodingKeys` raw values. The decoder's flow on
//! `"actor_queue_depth"`:
//!
//! 1. Transform `"actor_queue_depth"` → `"actorQueueDepth"`.
//! 2. Look for a `CodingKey` whose `stringValue` is `"actorQueueDepth"`.
//! 3. Find `case actorQueueDepth = "actor_queue_depth"` whose `stringValue`
//!    is the explicit raw value `"actor_queue_depth"` — NOT `"actorQueueDepth"`.
//! 4. No match → `KEY_NOT_FOUND` on every field of every Stage 1 struct.
//!
//! Result: every iOS build between commits `e5a4a88d` (PR #358) and the
//! fix in PR #364 decoded `nil` for every kernel snapshot tick, leaving the
//! app stuck at the `createAccount` spinner. PR #364 removed the explicit
//! `CodingKeys` from the Stage 1 emitter (synthesised `CodingKeys`, whose
//! implicit raw value equals the Swift identifier `"actorQueueDepth"`,
//! matches the decoder's post-transform key correctly).
//!
//! ## What slipped through CI
//!
//! The `codegen-drift` workflow only checks that the committed Swift file
//! matches a fresh codegen run (text equality). It NEVER asks "is the
//! generated output decodable under the decoder configuration the iOS
//! shell actually uses?" The first three assertions below would have
//! turned this regression into a red CI gate at PR #358 time.
//!
//! ## What these tests check
//!
//! 1. **No explicit `CodingKeys` in Stage 1.** The decoder strategy is
//!    `.convertFromSnakeCase`; explicit raw values double-transform.
//! 2. **No `= "snake_case"` raw values anywhere in Stage 1.** A regex-free
//!    line walk: any `= "..."` literal in the Stage 1 slice must contain
//!    no `_`. Covers `actor_queue_depth`, `relay_url`, `first_event_ms`,
//!    and any future snake_case field a contributor accidentally promotes
//!    into a raw value.
//! 3. **Field name matches `convertFromSnakeCase` semantics.** For a JSON
//!    key `actor_queue_depth` the Swift field MUST be `actorQueueDepth`
//!    (no underscore in the identifier, no explicit raw value).
//! 4. **Stage 1 / Stage 2 boundary is detectable.** The renderer always
//!    emits a `// MARK: - SnapshotProjections` header; without it the
//!    "Stage 1 has no CodingKeys" check cannot scope itself and either
//!    fires false positives on the Stage 2 section or misses Stage 1
//!    regressions entirely.
//!
//! ## What these tests do NOT check
//!
//! - Actual Swift decoding. The Rust test binary cannot invoke `JSONDecoder`.
//!   The PROXY is: enforce the textual invariants Apple's
//!   `.convertFromSnakeCase` algorithm requires for a `CodingKey`-free
//!   struct. Equivalent strength: if any assertion would let a regression
//!   through, the iOS decode would fail. If every assertion passes, the
//!   iOS decode succeeds (modulo Stage 2, which has its own targeted
//!   tests in `swift.rs`).
//! - Stage 2 `SnapshotProjections` rules. Those are covered by
//!   `render_snapshot_projections_*` tests in `swift.rs` and are
//!   structurally different (they DO emit explicit raw values for dotted
//!   host-registered keys whose post-transform value differs from the
//!   Swift field name).

use nmp_codegen::swift::render_swift;

/// Schema with one type whose every property is snake_case. This is the
/// exact shape PR #358 broke on — multiple snake_case fields that the
/// emitter would have wrapped in a `CodingKeys` enum.
///
/// Field count is deliberately above 5 to satisfy the task's
/// "5+ snake_case fields" requirement and to give the line-walk assertion
/// in `stage1_emits_no_snake_case_raw_values` more material to scan.
fn snake_case_heavy_document() -> &'static str {
    r#"{
      "version": 1,
      "types": [
        {
          "rust_path": "nmp_core::demo::SnapshotMetrics",
          "swift_name": "SnapshotMetrics",
          "id_field": null,
          "conformances": ["Decodable", "Equatable"],
          "schema": {
            "type": "object",
            "title": "SnapshotMetrics",
            "properties": {
              "actor_queue_depth": { "type": "integer", "format": "uint64" },
              "relay_url":         { "type": "string" },
              "first_event_ms":    { "type": ["integer", "null"], "format": "uint128" },
              "last_seen_at":      { "type": "integer", "format": "uint64" },
              "pending_messages":  { "type": "integer", "format": "uint32" },
              "is_connected":      { "type": "boolean" },
              "subscription_id":   { "type": "string" }
            },
            "required": [
              "actor_queue_depth",
              "relay_url",
              "last_seen_at",
              "pending_messages",
              "is_connected",
              "subscription_id"
            ]
          }
        }
      ]
    }"#
}

/// The Stage 1 / Stage 2 boundary header. The renderer always appends the
/// `SnapshotProjections` section after Stage 1; assertions against
/// Stage-1-only invariants must scope to everything BEFORE this marker.
const SNAPSHOT_SECTION_MARKER: &str = "// MARK: - SnapshotProjections";

/// Return the rendered output's Stage 1 portion — everything before the
/// `SnapshotProjections` section header. If the marker is missing the whole
/// output is Stage 1 (defensive; the renderer always emits the marker).
fn stage1_slice(out: &str) -> &str {
    match out.find(SNAPSHOT_SECTION_MARKER) {
        Some(idx) => &out[..idx],
        None => out,
    }
}

/// Extract every `= "<literal>"` raw value present on a single line. Returns
/// the unquoted contents (the literal without the surrounding `"`s).
///
/// Pure string ops — `nmp-codegen` deliberately has no `regex` dep and we
/// don't want one for a CI test. The grammar we need is fixed: Swift
/// `CodingKeys` raw values are always `= "<ascii-identifier-or-key>"` with
/// no interpolation, no escapes, no multi-line strings. A linear scan
/// catches every shape the emitter could produce.
fn raw_values_on_line(line: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let bytes = line.as_bytes();
    let mut i = 0;
    // The bound `i + 3 <= bytes.len()` (equivalently `i + 2 < bytes.len()`)
    // ensures we can safely read 3 bytes starting at `i`. A line ending in
    // exactly `= "` with no closing `"` is correctly skipped — Swift
    // `CodingKeys` raw values always have a closing quote, so a truncated
    // form would be a generator bug separate from the snake_case regression.
    while i + 3 <= bytes.len() {
        // Look for the literal `= "` (with the leading space — that's how
        // the emitter writes it; no other context produces this exact
        // 3-byte sequence in generated Swift).
        if &bytes[i..i + 3] == b"= \"" {
            let start = i + 3;
            if let Some(end_off) = bytes[start..].iter().position(|&b| b == b'"') {
                let end = start + end_off;
                // Safe: we're slicing on byte positions of ASCII delimiters
                // inside what's already valid UTF-8 (`line: &str`).
                out.push(&line[start..end]);
                i = end + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

// ── 1. The exact PR #358 regression: no explicit CodingKeys in Stage 1 ─────

#[test]
fn stage1_emits_no_explicit_coding_keys_enum() {
    let out = render_swift(snake_case_heavy_document()).expect("renders");
    let stage1 = stage1_slice(&out);

    assert!(
        !stage1.contains("enum CodingKeys"),
        "PR #358 regression check failed: the Stage-1 emitter must not write \
         an explicit `CodingKeys` enum. Apple's `JSONDecoder.\
         keyDecodingStrategy = .convertFromSnakeCase` (set in `KernelBridge.\
         decode()`) transforms incoming JSON keys BEFORE matching them \
         against `CodingKey.stringValue`, so any explicit raw value would \
         have to be the post-transform key. The simpler fix is to omit the \
         enum and let Swift synthesise `CodingKeys` whose implicit raw value \
         equals the Swift identifier — which is exactly what the decoder \
         produces post-transform. See the file-level doc-comment for the \
         full incident write-up.\n\nStage 1 slice was:\n{stage1}"
    );

    // Belt and braces: even if a future emitter change added a CodingKeys
    // enum that happens to use the literal `CodingKeys` token in a comment
    // rather than a declaration, the `case` keyword inside an enum body is
    // the actual smoking gun.
    assert!(
        !stage1.contains("\n        case "),
        "PR #358 regression check failed: the Stage-1 slice contains a \
         `case <field>` declaration, which is only emitted inside a \
         `CodingKeys: String, CodingKey` enum body. Stage 1 must rely on \
         synthesised `CodingKeys`. Stage 1 slice was:\n{stage1}"
    );
}

// ── 2. No `= "snake_case"` raw values anywhere in Stage 1 ──────────────────

#[test]
fn stage1_emits_no_snake_case_raw_values() {
    let out = render_swift(snake_case_heavy_document()).expect("renders");
    let stage1 = stage1_slice(&out);

    // Walk every line in the Stage 1 slice. Any line carrying a `= "..."`
    // raw value literal must contain no `_` inside that literal. The
    // emitter has no legitimate reason to write a snake_case raw value
    // anywhere in the Stage 1 section (Stage 2 is excluded by `stage1_slice`).
    //
    // We collect failures and assert at the end so the error message names
    // every offending line — much faster to debug than the first-line
    // failure mode.
    let mut offenders: Vec<(usize, String, String)> = Vec::new();
    for (lineno, line) in stage1.lines().enumerate() {
        for literal in raw_values_on_line(line) {
            if literal.contains('_') {
                offenders.push((lineno + 1, literal.to_string(), line.to_string()));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "PR #358 regression check failed: the Stage-1 slice contains \
         snake_case `CodingKey` raw value(s). With \
         `keyDecodingStrategy = .convertFromSnakeCase` the decoder \
         double-transforms (JSON `actor_queue_depth` → expected key \
         `actorQueueDepth`, raw value `actor_queue_depth` → no match, \
         KEY_NOT_FOUND on every field). Offending lines:\n{}",
        offenders
            .iter()
            .map(|(lineno, literal, line)| format!(
                "  line {lineno}: raw value `\"{literal}\"` in `{line}`"
            ))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

// ── 3. Field identifiers match convertFromSnakeCase semantics ──────────────

#[test]
fn stage1_field_names_match_convert_from_snake_case_output() {
    let out = render_swift(snake_case_heavy_document()).expect("renders");
    let stage1 = stage1_slice(&out);

    // Apple's `.convertFromSnakeCase` produces these camelCase identifiers
    // for the snake_case JSON keys in `snake_case_heavy_document`. The
    // emitter must produce a `public let <camelCase>:` declaration for
    // each — that is the field name Codable will look up post-transform.
    //
    // Pairs are: (JSON key in schema, expected Swift identifier).
    let cases = [
        ("actor_queue_depth", "actorQueueDepth"),
        ("relay_url", "relayUrl"),
        ("first_event_ms", "firstEventMs"),
        ("last_seen_at", "lastSeenAt"),
        ("pending_messages", "pendingMessages"),
        ("is_connected", "isConnected"),
        ("subscription_id", "subscriptionId"),
    ];

    for (json_key, swift_field) in cases {
        let decl_needle = format!("public let {swift_field}:");
        assert!(
            stage1.contains(&decl_needle),
            "PR #358 regression check failed: JSON key `{json_key}` should \
             produce Swift field `{swift_field}` (per \
             `.convertFromSnakeCase` semantics) but no `{decl_needle}` was \
             found in Stage 1 slice:\n{stage1}"
        );
        // The snake_case form must NOT appear as a Swift identifier. If
        // the emitter regressed to writing `public let actor_queue_depth:`
        // the decoder's post-transform key `actorQueueDepth` would never
        // match.
        let snake_decl = format!("public let {json_key}:");
        assert!(
            !stage1.contains(&snake_decl),
            "PR #358 regression check failed: the Stage-1 slice declares \
             a snake_case field `{snake_decl}`. Swift field identifiers \
             must be camelCase to match what the \
             `.convertFromSnakeCase` decoder produces post-transform."
        );
    }
}

// ── 4. Stage 1 / Stage 2 boundary header is present and detectable ─────────

#[test]
fn renderer_emits_snapshot_projections_section_marker() {
    // The `// MARK: - SnapshotProjections` header is what every "Stage 1
    // invariant" assertion uses to scope itself. If the renderer ever stops
    // emitting it (refactor, formatting change, accidental deletion), the
    // Stage 1 assertions above silently start scanning the Stage 2 section
    // too — which DOES emit explicit raw values and `CodingKeys` legitimately
    // — and either fire false positives or, worse, miss real Stage 1
    // regressions because the broader output also contains legitimate
    // matches.
    //
    // This test pins the marker so any future renderer change that drops
    // or renames it fails loudly before it breaks the regression suite.
    let out = render_swift(snake_case_heavy_document()).expect("renders");
    assert!(
        out.contains(SNAPSHOT_SECTION_MARKER),
        "renderer must emit `{SNAPSHOT_SECTION_MARKER}` as the Stage 1 / \
         Stage 2 boundary header; the regression tests in this file scope \
         their assertions to the Stage 1 slice using this exact marker. \
         Render was:\n{out}"
    );

    // The marker must appear AFTER at least one Stage 1 `public struct`,
    // otherwise the slice is empty and the regression assertions are
    // vacuous (they pass trivially on an empty string).
    let marker_pos = out.find(SNAPSHOT_SECTION_MARKER).expect("marker present");
    let first_struct_pos = out
        .find("public struct ")
        .expect("at least one Stage 1 struct must be present");
    assert!(
        first_struct_pos < marker_pos,
        "Stage 1 `public struct` declaration must precede the \
         `SnapshotProjections` marker, otherwise the Stage 1 slice is \
         empty and regression assertions become vacuous. first_struct_pos \
         = {first_struct_pos}, marker_pos = {marker_pos}"
    );
}

// ── 5. Sanity: the raw-value scanner itself works ──────────────────────────

#[test]
fn raw_value_scanner_extracts_quoted_literals() {
    // Guard the scanner the regression assertions depend on. If
    // `raw_values_on_line` regresses (skipped literals, misparsed escapes,
    // or matches outside the `= "..."` form), the snake_case raw-value
    // check above silently passes when it should fail. Pin the scanner's
    // behaviour explicitly here.
    assert_eq!(
        raw_values_on_line("        case groupChat = \"nmp.nip29.groupChat\""),
        vec!["nmp.nip29.groupChat"],
    );
    assert_eq!(
        raw_values_on_line("        case foo = \"a\", bar = \"b\""),
        vec!["a", "b"],
    );
    // No `= "..."` → no matches. Lines like `public let foo: String` must
    // not be misread.
    assert!(raw_values_on_line("    public let foo: String").is_empty());
    assert!(raw_values_on_line("        case wallet").is_empty());
    // The exact regression we're guarding against MUST surface as a match.
    assert_eq!(
        raw_values_on_line("        case actorQueueDepth = \"actor_queue_depth\""),
        vec!["actor_queue_depth"],
    );
}
