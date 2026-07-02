//! Owns the V6 Stage 2 `SnapshotProjections` registry-wiring emitter —
//! rendering the `SnapshotProjections` struct + `CodingKeys` enum from the
//! static [`SnapshotProjectionEntry`] slice in
//! [`crate::swift_projections_registry`], plus Apple's
//! `.convertFromSnakeCase` key-transform algorithm the `CodingKeys` raw
//! values depend on.
//!
//! Split out of `swift.rs` so the top-level orchestration file stays under
//! the file-size ceiling; the sibling [`crate::swift::flat_record_emit`]
//! module owns the unrelated Stage 1 type-emission half of the pilot.

use crate::swift_projections_registry::SnapshotProjectionEntry;

/// Render the V6 Stage 2 `SnapshotProjections` struct and its `CodingKeys`
/// enum to `out`, driven by the [`SnapshotProjectionEntry`] registry.
///
/// Output shape, drop-in for the hand-written declaration in
/// `apps/chirp/ios/Chirp/Bridge/KernelBridge.swift`:
///
/// ```swift
/// internal struct SnapshotProjections: Decodable, Equatable {
///     let wallet: WalletStatusData?
///     // ... one line per entry ...
///
///     enum CodingKeys: String, CodingKey {
///         case wallet
///         case bunkerHandshake
///         case groupChat = "nmp.nip29.groupChat"
///         // ... case per entry, raw value only when post-transform key
///         //     differs from the Swift field name ...
///     }
/// }
/// ```
///
/// Visibility is `internal` (no modifier) to match the hand-written
/// declaration's visibility verbatim — the conformance test in
/// `SnapshotProjectionsConformanceTests.swift` accesses the type via
/// `@testable import Chirp`, which exposes `internal` symbols. Bumping to
/// `public` would change the symbol-table surface area unnecessarily.
pub(crate) fn render_snapshot_projections(entries: &[SnapshotProjectionEntry], out: &mut String) {
    out.push_str("// MARK: - SnapshotProjections\n");
    out.push_str(
        "// Source: crates/nmp-codegen/src/swift_projections_registry.rs (Stage 2 registry)\n",
    );
    out.push_str("//\n");
    out.push_str("// The kernel's host-extensible `projections` map. Each entry mirrors one\n");
    out.push_str("// registered snapshot-projection key. Every member is optional so a stale\n");
    out.push_str("// kernel build that predates a projection still decodes (D1 forward-compat).\n");
    out.push_str("//\n");
    out.push_str("// The `CodingKeys` enum below uses post-`.convertFromSnakeCase` raw values\n");
    out.push_str("// (the iOS shell's `KernelHandle.decode` sets that strategy). Cases whose\n");
    out.push_str("// raw value matches the Swift property name carry no explicit literal.\n");
    out.push_str("struct SnapshotProjections: Decodable, Equatable {\n");

    for entry in entries {
        out.push_str(&format!(
            "    let {}: {}?\n",
            entry.swift_field, entry.swift_type
        ));
    }

    out.push('\n');
    out.push_str("    enum CodingKeys: String, CodingKey {\n");
    for entry in entries {
        let post_transform = post_convert_from_snake_case(entry.key);
        if post_transform == entry.swift_field {
            out.push_str(&format!("        case {}\n", entry.swift_field));
        } else {
            out.push_str(&format!(
                "        case {} = \"{}\"\n",
                entry.swift_field, post_transform
            ));
        }
    }
    out.push_str("    }\n");
    out.push_str("}\n");
}

/// Apple's `JSONDecoder.KeyDecodingStrategy.convertFromSnakeCase` algorithm.
///
/// The strategy transforms an incoming JSON key BEFORE matching it against
/// any `CodingKey.stringValue`. The transform per Apple's docs:
///
/// 1. Capture all leading underscores (preserved verbatim on the output).
/// 2. Capture all trailing underscores (preserved verbatim on the output).
/// 3. Split the middle on each `_`, lowercase the first run, uppercase the
///    first letter of every subsequent run.
///
/// What the docs leave implicit and what bit the iOS shell historically:
///
/// - **`.` is opaque.** `.convertFromSnakeCase` does NOT split on `.`; it
///   only touches `_`. So `"nmp.nip29.group_events"` becomes
///   `"nmp.nip29.groupChat"`, NOT `"nmp.Nip29.GroupChat"`. The dot-separated
///   prefix passes through unchanged, and only the `group_events` tail
///   camelises.
/// - **Single-word inputs are returned unchanged.** `"wallet"` → `"wallet"`,
///   `"profile"` → `"profile"`. Apple's algorithm has nothing to do, so the
///   strategy is a no-op for any key without `_`.
///
/// This implementation handles both observed shapes (`snake_case` and
/// `nmp.<nip>.snake_case`) plus the pure-camel pass-through case. It is
/// NOT a complete reimplementation of Apple's full edge-case set (leading
/// and trailing underscores in particular) — none of the registry keys
/// carry those, and the docstring on
/// [`crate::swift_projections_registry::SnapshotProjectionEntry`] tells the
/// next contributor to validate any new key shape here before adding it.
pub(crate) fn post_convert_from_snake_case(key: &str) -> String {
    // Single-word fast path: no `_`, the strategy returns the input
    // unchanged. Covers `wallet`, `profile`, `timeline`, etc.
    if !key.contains('_') {
        return key.to_string();
    }
    // The `.` is opaque to `.convertFromSnakeCase`. Split on `.` first,
    // transform each segment independently, rejoin. A bare snake_case
    // key (no `.`) hits the same path with a single segment.
    let segments: Vec<String> = key.split('.').map(camelize_snake_segment).collect();
    segments.join(".")
}

/// Transform one `.`-delimited segment of a key (or the whole key when it
/// has no `.`s). Splits on `_`, lowercases the first run, uppercases the
/// first letter of every subsequent run. The implementation matches
/// Apple's reference for the inner `_`-handling step of
/// `.convertFromSnakeCase`.
fn camelize_snake_segment(segment: &str) -> String {
    let mut parts = segment.split('_');
    let mut out = parts.next().unwrap_or("").to_string();
    for part in parts {
        if part.is_empty() {
            // Consecutive `__` is preserved as nothing — Apple's algorithm
            // collapses empty runs. None of the registry keys hit this.
            continue;
        }
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(chars.as_str());
        }
    }
    out
}
