import XCTest

/// UI test suite ported from `ios/NmpStress/NmpStressUITests/NmpStressUITests.swift`
/// (now deleted). Runs against Chirp on the simulator, driving the real kernel
/// FFI through the actual SwiftUI navigation stack rather than an isolated
/// diagnostic harness.
///
/// Navigation path differs from the original NmpStress test:
///   Home feed → Settings → Diagnostics  (for relay/metrics assertions)
///   Settings → Home → tap author → Profile  (for profile assertions)
///   Profile → tap note row → Thread  (for thread assertions)
///
/// `NMP_TEST_NSEC` causes KernelModel to auto-call `signInNsec` on start,
/// bypassing the Onboarding wall so the test can reach the main tabs without
/// driving the UI sign-in flow.
@MainActor
final class ChirpUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testTimelineDiagnosticsAndNavigation() throws {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_VISIBLE_LIMIT"] = "80"
        app.launchEnvironment["NMP_EMIT_HZ"] = "4"
        // Pre-baked fixture key — same as SmokeScenariosTests.nsecA.
        app.launchEnvironment["NMP_TEST_NSEC"] =
            "nsec12c7ujxnnut2dnahjjsecq79507fg2p2h7ul4a3rqepg5vyk8c9lqyc30gw"
        app.launch()

        // ── Wait for the Home feed to appear (sign-in succeeded) ────────────
        let timeline = app.collectionViews["timeline-list"]
        XCTAssertTrue(timeline.waitForExistence(timeout: 15), "timeline-list never appeared — auto sign-in may have failed")

        // ── Navigate to Settings → Diagnostics ──────────────────────────────
        app.tabBars.buttons["Settings"].tap()
        app.collectionViews.buttons["Diagnostics"].tap()

        // ── Diagnostics: relay state and perf metrics ────────────────────────
        let diagList = app.scrollViews["diagnostics-list"]
        XCTAssertTrue(diagList.waitForExistence(timeout: 5))

        let relayState = app.staticTexts["relay-state-value"]
        XCTAssertTrue(relayState.waitForExistence(timeout: 8))
        XCTAssertTrue(waitForLabel(relayState, equals: "CONNECTED", timeout: 30), relayState.label)

        let events = app.staticTexts["metric-events-value"]
        let visible = app.staticTexts["metric-visible-value"]
        let rx = app.staticTexts["metric-rx-value"]
        let firstMs = app.staticTexts["metric-first-ms-value"]
        let applyUs = app.staticTexts["metric-apply-us-value"]

        XCTAssertTrue(waitForNumericValue(events, greaterThan: 20, timeout: 25), events.label)
        XCTAssertTrue(waitForNumericValue(visible, greaterThan: 5, timeout: 15), visible.label)
        XCTAssertNotEqual(rx.label, "0 bytes")
        XCTAssertNotEqual(firstMs.label, "-")
        XCTAssertLessThan(applyUs.label.numericValue, 50_000)

        let relayStateLabel = relayState.label
        let eventsLabel = events.label
        let visibleLabel = visible.label
        let rxLabel = rx.label
        let firstMsLabel = firstMs.label
        let applyUsLabel = applyUs.label

        // ── Navigate to Home feed ────────────────────────────────────────────
        app.tabBars.buttons["Home"].tap()
        XCTAssertTrue(app.collectionViews["timeline-list"].waitForExistence(timeout: 5))
        app.swipeUp(velocity: .fast)

        // ── Tap an author avatar to open a Profile ────────────────────────────
        let profileLink = app.buttons.matching(identifier: "timeline-author-link").firstMatch
        XCTAssertTrue(profileLink.waitForExistence(timeout: 10))
        profileLink.tap()

        XCTAssertTrue(app.scrollViews["profile-detail-list"].waitForExistence(timeout: 8))
        let profileNotes = app.staticTexts["profile-notes-count-value"]
        XCTAssertTrue(waitForNumericValue(profileNotes, greaterThan: 0, timeout: 20), profileNotes.label)
        let profileNotesLabel = profileNotes.label

        // ── Tap a note row to open a Thread ──────────────────────────────────
        let profileThreadLink = app.descendants(matching: .any).matching(identifier: "profile-thread-link").firstMatch
        XCTAssertTrue(profileThreadLink.waitForExistence(timeout: 10))
        profileThreadLink.tap()

        XCTAssertTrue(app.scrollViews["thread-detail-list"].waitForExistence(timeout: 8))
        let focusedNote = app.descendants(matching: .any)["thread-focused-note"]
        XCTAssertTrue(focusedNote.waitForExistence(timeout: 20))

        print(
            "NMP_REAL_RELAY_METRICS relay=\(relayStateLabel) events=\(eventsLabel) visible=\(visibleLabel) rx=\(rxLabel) first_ms=\(firstMsLabel) apply_us=\(applyUsLabel) profile_notes=\(profileNotesLabel)"
        )
    }

    // ── Polling helpers ───────────────────────────────────────────────────────

    private func waitForLabel(
        _ element: XCUIElement,
        equals expected: String,
        timeout: TimeInterval
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if element.label == expected { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return false
    }

    private func waitForNumericValue(
        _ element: XCUIElement,
        greaterThan threshold: Int,
        timeout: TimeInterval
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if element.label.numericValue > threshold { return true }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }
        return false
    }
}

private extension String {
    var numericValue: Int {
        let digits = filter(\.isNumber)
        return Int(digits) ?? 0
    }
}
