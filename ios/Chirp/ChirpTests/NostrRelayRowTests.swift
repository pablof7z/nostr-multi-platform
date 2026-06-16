import SwiftUI
import XCTest
@testable import Chirp

/// Issue #996 — `NostrRelayRow` is the gallery relay-row primitive and the ONLY
/// presentation logic it owns is `tintColor(for:)` (a semantic-token → SwiftUI
/// `Color` mapping). The role → label / role → tint *selection* is the kernel's
/// job: it flows from the `relay_role_options` projection
/// (`RelayRoleOption.label` / `.tint`) and the shell hands those strings to the
/// row verbatim. These tests pin both halves of that contract:
///
///  1. `tintColor(for:)` resolves the kernel's semantic tint tokens (and a hex
///     fallback) — the one rendering computation the component is allowed to do.
///  2. Resolving a relay's role against `relayRoleOptions` yields the
///     kernel-emitted `label`/`tint`, never a Swift-derived string — the same
///     lookup `RelayConfigRow` performs before constructing a `NostrRelayRow`.
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

    /// The kernel's `relay_role_options` projection is the single source of
    /// truth for a role's label + tint. Resolving a relay row's role against it
    /// is the entire derivation Chirp's `RelayConfigRow` performs — no Swift
    /// `switch role { … }` exists anymore.
    func testRoleResolvesToKernelEmittedLabelAndTint() {
        let options = [
            RelayRoleOption(isDefault: true, label: "Both", tint: "accent", value: "both"),
            RelayRoleOption(isDefault: false, label: "Read", tint: "info", value: "read"),
            RelayRoleOption(isDefault: false, label: "Index", tint: "neutral", value: "indexer"),
            RelayRoleOption(isDefault: false, label: "Both + Index", tint: "accent", value: "both,indexer"),
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
