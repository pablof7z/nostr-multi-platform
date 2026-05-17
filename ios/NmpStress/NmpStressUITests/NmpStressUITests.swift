import XCTest

@MainActor
final class NmpStressUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testPrimalTimelineAndDiagnosticsRender() throws {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_VISIBLE_LIMIT"] = "80"
        app.launchEnvironment["NMP_EMIT_HZ"] = "4"
        app.launch()

        let relayState = app.staticTexts["relay-state-value"]
        let events = app.staticTexts["metric-events-value"]
        let visible = app.staticTexts["metric-visible-value"]
        let rx = app.staticTexts["metric-rx-value"]
        let firstMs = app.staticTexts["metric-first-ms-value"]
        let applyUs = app.staticTexts["metric-apply-us-value"]

        XCTAssertTrue(relayState.waitForExistence(timeout: 8))
        XCTAssertTrue(waitForLabel(relayState, equals: "CONNECTED", timeout: 20), relayState.label)
        XCTAssertTrue(waitForNumericValue(events, greaterThan: 20, timeout: 20), events.label)
        XCTAssertTrue(waitForNumericValue(visible, greaterThan: 5, timeout: 10), visible.label)
        XCTAssertNotEqual(rx.label, "0 B")
        XCTAssertNotEqual(firstMs.label, "-")
        XCTAssertLessThan(applyUs.label.numericValue, 50_000)
        let relayStateLabel = relayState.label
        let eventsLabel = events.label
        let visibleLabel = visible.label
        let rxLabel = rx.label
        let firstMsLabel = firstMs.label
        let applyUsLabel = applyUs.label

        app.buttons["Diagnostics"].tap()
        XCTAssertTrue(app.collectionViews["diagnostics-list"].waitForExistence(timeout: 3))
        XCTAssertTrue(app.staticTexts["Relays"].exists)
        app.swipeUp(velocity: .fast)

        app.buttons["Timeline"].tap()
        XCTAssertTrue(app.collectionViews["timeline-list"].waitForExistence(timeout: 3))
        app.swipeUp(velocity: .fast)

        let profileLink = app.buttons.matching(identifier: "timeline-author-link").firstMatch
        XCTAssertTrue(profileLink.waitForExistence(timeout: 8))
        profileLink.tap()
        XCTAssertTrue(app.collectionViews["profile-detail-list"].waitForExistence(timeout: 5))
        let profileNotes = app.staticTexts["profile-notes-count-value"]
        XCTAssertTrue(waitForNumericValue(profileNotes, greaterThan: 0, timeout: 15), profileNotes.label)
        let profileNotesLabel = profileNotes.label

        let profileThreadLink = app.descendants(matching: .any).matching(identifier: "profile-thread-link").firstMatch
        XCTAssertTrue(profileThreadLink.waitForExistence(timeout: 8))
        profileThreadLink.tap()
        XCTAssertTrue(app.collectionViews["thread-detail-list"].waitForExistence(timeout: 5))
        let focusedNote = app.descendants(matching: .any)["thread-focused-note"]
        XCTAssertTrue(focusedNote.waitForExistence(timeout: 15))

        print(
            "NMP_REAL_RELAY_METRICS relay=\(relayStateLabel) events=\(eventsLabel) visible=\(visibleLabel) rx=\(rxLabel) first_ms=\(firstMsLabel) apply_us=\(applyUsLabel) profile_notes=\(profileNotesLabel)"
        )
    }

    private func waitForLabel(
        _ element: XCUIElement,
        equals expected: String,
        timeout: TimeInterval
    ) -> Bool {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if element.label == expected {
                return true
            }
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
            if element.label.numericValue > threshold {
                return true
            }
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
