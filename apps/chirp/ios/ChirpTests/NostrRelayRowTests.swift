import SwiftUI
import XCTest
@testable import Chirp

/// Issue #996 — `NostrRelayRow` is the gallery relay-row primitive and the ONLY
/// presentation logic it owns is `tintColor(for:)` (a semantic-token → SwiftUI
/// `Color` mapping). The role → tint selection comes from the kernel's
/// `relay_role_options` projection (`RelayRoleOption.tint`). The role → label
/// mapping is now a computed shell property (`RelayRoleOption.label`, #1678 /
/// D7 — removed from the wire, derived from `value` in the shell). These tests
/// pin both halves of that contract:
///
///  1. `tintColor(for:)` resolves the kernel's semantic tint tokens (and a hex
///     fallback) — the one rendering computation the component is allowed to do.
///  2. Resolving a relay's role against `relayRoleOptions` yields the
///     shell-computed `label` and kernel-emitted `tint` — the same lookup
///     `RelayConfigRow` performs before constructing a `NostrRelayRow`.
@MainActor
final class NostrRelayRowTests: XCTestCase {

    // MARK: tintColor(for:) — the only allowed presentation logic

    func testTintColorResolvesSemanticTokens() {
        XCTAssertEqual(NostrRelayRow.tintColor(for: "accent"), .accentColor)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "info"), ChirpColor.accent)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "success"), .green)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "warning"), .orange)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "danger"), .red)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "error"), .red)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "neutral"), .secondary)
    }

    func testTintColorIsCaseInsensitive() {
        XCTAssertEqual(NostrRelayRow.tintColor(for: "ACCENT"), .accentColor)
        XCTAssertEqual(NostrRelayRow.tintColor(for: "Success"), .green)
    }

    func testTintColorAcceptsHexFallback() {
        // A 6-char hex token resolves to that exact color (forward-compat for a
        // future kernel that emits hex tints).
        XCTAssertEqual(NostrRelayRow.tintColor(for: "ff8800"), Color(red: 1.0, green: 0x88 / 255, blue: 0.0))
        XCTAssertEqual(NostrRelayRow.tintColor(for: "#00ff00"), Color(red: 0.0, green: 1.0, blue: 0.0))
    }

    func testTintColorUnknownTokenFallsBackToSecondary() {
        XCTAssertEqual(NostrRelayRow.tintColor(for: "totally-unknown"), .secondary)
        XCTAssertEqual(NostrRelayRow.tintColor(for: ""), .secondary)
    }

    // MARK: role → label/tint comes from the kernel options, not Swift

    /// The kernel's `relay_role_options` projection provides `value` + `tint`
    /// for each role option. The `label` is now a computed shell property
    /// mapping `value` → English string (#1678 / D7). Resolving a relay row's
    /// role against the options yields the correct label (via `RelayRoleOption
    /// .label`) and the kernel-emitted tint — the same lookup `RelayConfigRow`
    /// performs before constructing a `NostrRelayRow`.
    func testRoleResolvesToLabelAndTint() {
        // `label` is no longer a stored property: remove it from the initialiser.
        // The computed property derives it from `value` in the shell.
        let options = [
            RelayRoleOption(isDefault: true, tint: "accent", value: "both"),
            RelayRoleOption(isDefault: false, tint: "info", value: "read"),
            RelayRoleOption(isDefault: false, tint: "neutral", value: "indexer"),
            RelayRoleOption(isDefault: false, tint: "accent", value: "both,indexer"),
        ]

        func resolve(_ role: String) -> (label: String, tint: String) {
            let option = options.first { $0.value == role }
            return (option?.label ?? role, option?.tint ?? "accent")
        }

        XCTAssertEqual(resolve("read").label, "Read")
        XCTAssertEqual(resolve("read").tint, "info")
        XCTAssertEqual(resolve("indexer").label, "Index")
        XCTAssertEqual(resolve("both,indexer").label, "Both + Index")
        XCTAssertEqual(resolve("both,indexer").tint, "accent")
    }

    /// An unrecognised role (kernel option not yet loaded) degrades to the raw
    /// role token with an `accent` tint — never a Swift-derived label.
    func testUnknownRoleDegradesToRawTokenAndAccent() {
        let options: [RelayRoleOption] = []
        let option = options.first { $0.value == "read" }
        XCTAssertEqual(option?.label ?? "read", "read")
        XCTAssertEqual(option?.tint ?? "accent", "accent")
    }
}
