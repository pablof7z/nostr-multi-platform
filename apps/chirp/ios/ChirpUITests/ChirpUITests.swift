import Foundation
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
/// `NMP_TEST_NSEC` causes KernelModel to restore a Chirp identity on start,
/// bypassing the Onboarding wall so the test can reach the main tabs without
/// driving the UI sign-in flow.
@MainActor
final class ChirpUITests: XCTestCase {
    override func setUpWithError() throws {
        continueAfterFailure = false
    }

    func testCurrentUserAvatarOpensProfile() throws {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_TEST_NSEC"] =
            "nsec12c7ujxnnut2dnahjjsecq79507fg2p2h7ul4a3rqepg5vyk8c9lqyc30gw"
        app.launch()

        let profileButton = app.buttons["Open your profile"]
        XCTAssertTrue(profileButton.waitForExistence(timeout: 10))
        profileButton.tap()

        XCTAssertTrue(app.scrollViews["profile-detail-list"].waitForExistence(timeout: 8))
    }

    func testCreatedAccountRestoresAfterRelaunch() throws {
        let service = isolatedKeychainService("created")
        let app = launchApp(keychainService: service)

        app.buttons["Create account"].tap()
        let name = app.textFields["Satoshi"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.tap()
        name.typeText("Xcode Created")
        app.buttons["Create account"].tap()
        assertSignedIn(app, timeout: 12)

        app.terminate()
        app.launchEnvironment["NMP_TEST_KEYCHAIN_SERVICE"] = service
        app.launch()
        XCTAssertEqual(app.wait(for: .runningForeground, timeout: 10), true)
        assertSignedIn(app, timeout: 30)
    }

    func testNsecSignInRestoresAfterRelaunch() throws {
        let service = isolatedKeychainService("nsec")
        let app = launchApp(keychainService: service)

        app.buttons["I have an account"].tap()
        let field = app.secureTextFields["nsec1…"]
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText("nsec12c7ujxnnut2dnahjjsecq79507fg2p2h7ul4a3rqepg5vyk8c9lqyc30gw")
        app.buttons["Sign in"].tap()
        assertSignedIn(app, timeout: 12)

        app.terminate()
        app.launchEnvironment["NMP_TEST_KEYCHAIN_SERVICE"] = service
        app.launch()
        XCTAssertEqual(app.wait(for: .runningForeground, timeout: 10), true)
        assertSignedIn(app, timeout: 30)
    }

    func testNip46SignInRestoresAfterRelaunch() throws {
        let env = ProcessInfo.processInfo.environment
        let bunkerURI = env["NMP_TEST_BUNKER_URI"] ?? env["TEST_RUNNER_NMP_TEST_BUNKER_URI"]
        guard let bunkerURI, !bunkerURI.isEmpty else {
            throw XCTSkip("Set NMP_TEST_BUNKER_URI to a live bunker:// URI")
        }
        let service = isolatedKeychainService("nip46")
        let app = launchApp(keychainService: service)

        app.buttons["I have an account"].tap()
        let field = app.textFields["bunker://…"]
        XCTAssertTrue(field.waitForExistence(timeout: 5))
        field.tap()
        field.typeText(bunkerURI)
        app.buttons["Connect"].tap()
        assertSignedIn(app, timeout: 45)

        app.terminate()
        app.launchEnvironment["NMP_TEST_KEYCHAIN_SERVICE"] = service
        app.launch()
        XCTAssertEqual(app.wait(for: .runningForeground, timeout: 10), true)
        assertSignedIn(app, timeout: 45)
    }

    func testFreshAccountLoadsSeedFollowTimelineOnDevice() throws {
        let service = isolatedKeychainService("seedfeed")
        let app = launchApp(keychainService: service)

        app.buttons["Create account"].tap()
        let name = app.textFields["Satoshi"]
        XCTAssertTrue(name.waitForExistence(timeout: 5))
        name.tap()
        name.typeText("Seed Feed")
        app.buttons["Create account"].tap()
        XCTAssertTrue(app.tabBars.buttons["Settings"].waitForExistence(timeout: 20))

        app.tabBars.buttons["Settings"].tap()
        let diagnostics = app.buttons["Diagnostics"]
        for _ in 0..<3 where !diagnostics.exists {
            app.swipeUp()
        }
        diagnostics.tap()
        XCTAssertTrue(app.descendants(matching: .any)["diagnostics-list"].waitForExistence(timeout: 5))

        let events = app.staticTexts["metric-events-value"]
        let visible = app.staticTexts["metric-visible-value"]
        let rx = app.staticTexts["metric-rx-value"]
        let firstMs = app.staticTexts["metric-first-ms-value"]
        XCTAssertTrue(events.waitForExistence(timeout: 5))
        XCTAssertTrue(visible.waitForExistence(timeout: 5))
        XCTAssertTrue(rx.waitForExistence(timeout: 5))
        XCTAssertTrue(firstMs.waitForExistence(timeout: 5))
        XCTAssertTrue(waitForNumericValue(events, greaterThan: 0, timeout: 30), events.label)

        let eventsLabel = events.label
        let visibleLabel = visible.label
        let rxLabel = rx.label
        let firstMsLabel = firstMs.label
        print("NMP_FRESH_SEED_DIAG metrics events=\(eventsLabel) visible=\(visibleLabel) rx=\(rxLabel) first_ms=\(firstMsLabel)")

        let primalRelay = app.staticTexts["wss://relay.primal.net"]
        for _ in 0..<5 where !primalRelay.exists {
            app.swipeUp()
        }
        XCTAssertTrue(primalRelay.waitForExistence(timeout: 5))
        let connected = app.staticTexts["Connected"].exists
        print(
            "NMP_FRESH_SEED_DIAG events=\(eventsLabel) visible=\(visibleLabel) rx=\(rxLabel) first_ms=\(firstMsLabel) connected=\(connected)"
        )

        app.tabBars.buttons["Home"].tap()
        let labels = app.staticTexts.allElementsBoundByIndex
            .prefix(40)
            .map(\.label)
            .joined(separator: " | ")
        print("NMP_HOME_LABELS \(labels)")
        XCTAssertTrue(waitForHomeTimeline(app, timeout: 10))
        XCTAssertTrue(
            app.buttons.matching(identifier: "timeline-author-link").firstMatch.waitForExistence(timeout: 10),
            labels
        )
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
        XCTAssertTrue(waitForHomeTimeline(app, timeout: 15), "timeline-list never appeared — auto sign-in may have failed")

        // ── Navigate to Settings → Diagnostics ──────────────────────────────
        app.tabBars.buttons["Settings"].tap()
        app.collectionViews.buttons["Diagnostics"].tap()

        // ── Diagnostics: relay state and perf metrics ────────────────────────
        let diagList = app.descendants(matching: .any)["diagnostics-list"]
        XCTAssertTrue(diagList.waitForExistence(timeout: 5))

        let events = app.staticTexts["metric-events-value"]
        let visible = app.staticTexts["metric-visible-value"]
        let rx = app.staticTexts["metric-rx-value"]
        let firstMs = app.staticTexts["metric-first-ms-value"]
        let applyUs = app.staticTexts["metric-apply-us-value"]

        XCTAssertTrue(waitForNumericValue(events, greaterThan: 0, timeout: 25), events.label)
        XCTAssertNotEqual(rx.label, "0 bytes")
        XCTAssertNotEqual(firstMs.label, "-")
        XCTAssertLessThan(applyUs.label.numericValue, 50_000)

        let eventsLabel = events.label
        let visibleLabel = visible.label
        let rxLabel = rx.label
        let firstMsLabel = firstMs.label
        let applyUsLabel = applyUs.label

        // ── Navigate to Home feed ────────────────────────────────────────────
        app.tabBars.buttons["Home"].tap()
        XCTAssertTrue(waitForHomeTimeline(app, timeout: 5))
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
            "NMP_REAL_RELAY_METRICS events=\(eventsLabel) visible=\(visibleLabel) rx=\(rxLabel) first_ms=\(firstMsLabel) apply_us=\(applyUsLabel) profile_notes=\(profileNotesLabel)"
        )
    }

    // ── Profile-name regression + perf gates ───────────────────────────────────
    //
    // Defect under guard: navigating away from the feed and back caused author
    // display names to flash back to raw `shortHex` (e.g. "fce27ca1…0e145e514")
    // for 1–2 snapshot ticks (≤500ms) before re-resolving. The user also reported
    // general sluggishness. These tests pin the regression and add performance
    // baselines for scroll + nav transitions.
    //
    // shortHex shape (Chirp/Extensions/PubkeyFormatting.swift): exactly
    // `<8 hex>…<8 hex>` joined by a *horizontal ellipsis* (U+2026, one char) —
    // NOT three ASCII dots. The detector below matches the real shape and also
    // tolerates an ASCII `...`/`..` fallback for defensiveness.

    /// NSPredicate format string used to build the inverted "never regresses"
    /// expectation against an element's `label`.
    private static let shortHexPredicateFormat = "label MATCHES %@"
    /// Regex matching the Chirp `shortHex` abbreviation: 8 hex chars, an ellipsis
    /// (`…` or `...`/`..`), 8 hex chars — anchored so a fully-resolved display
    /// name ("Pablo", "Alice") never matches.
    private static let shortHexRegex =
        "^[0-9a-f]{8}(\u{2026}|\\.{2,3})[0-9a-f]{8}$"

    /// True when `label` is a raw shortHex pubkey abbreviation rather than a
    /// resolved display name.
    private func isShortHex(_ label: String) -> Bool {
        label.range(
            of: ChirpUITests.shortHexRegex,
            options: .regularExpression
        ) != nil
    }

    /// Launches Chirp with the pre-baked fixture key + 4Hz kernel and waits for
    /// the timeline to appear. Mirrors `testTimelineDiagnosticsAndNavigation`.
    private func launchFeedApp() -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_VISIBLE_LIMIT"] = "80"
        app.launchEnvironment["NMP_EMIT_HZ"] = "4"
        app.launchEnvironment["NMP_TEST_NSEC"] =
            "nsec12c7ujxnnut2dnahjjsecq79507fg2p2h7ul4a3rqepg5vyk8c9lqyc30gw"
        app.launch()
        XCTAssertTrue(waitForHomeTimeline(app, timeout: 15), "timeline-list never appeared — auto sign-in may have failed")
        return app
    }

    /// UI tests can relaunch into an iOS-restored navigation path (for example
    /// a Thread pushed from the Home tab). Force the Home tab back to its root
    /// before asserting feed state.
    private func waitForHomeTimeline(
        _ app: XCUIApplication,
        timeout: TimeInterval
    ) -> Bool {
        let timeline = timelineList(app)
        let authorName = app.staticTexts.matching(identifier: "timeline-author-name").firstMatch
        let authorLink = app.buttons.matching(identifier: "timeline-author-link").firstMatch
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if timeline.exists || authorName.exists || authorLink.exists {
                return true
            }
            let home = app.tabBars.buttons["Home"]
            if home.exists, home.isHittable {
                home.tap()
            }
            if timeline.waitForExistence(timeout: 0.2)
                || authorName.waitForExistence(timeout: 0.2)
                || authorLink.waitForExistence(timeout: 0.2)
            {
                return true
            }
            let backCandidates = [
                app.navigationBars.buttons["Chirp"].firstMatch,
                app.buttons["Chirp"].firstMatch,
            ]
            for candidate in backCandidates where candidate.exists && candidate.isHittable {
                candidate.tap()
                if timeline.waitForExistence(timeout: 0.4)
                    || authorName.waitForExistence(timeout: 0.4)
                    || authorLink.waitForExistence(timeout: 0.4)
                {
                    return true
                }
                break
            }
            _ = authorName.waitForExistence(timeout: 0.25)
        }
        return timeline.exists || authorName.exists || authorLink.exists
    }

    private func timelineList(_ app: XCUIApplication) -> XCUIElement {
        app.descendants(matching: .any)["timeline-list"]
    }

    /// Polls the timeline author-name labels and returns the first one that has
    /// resolved to a real display name (not shortHex, not empty). Returns `nil`
    /// on timeout. Requires the `timeline-author-name` accessibility identifier
    /// added to `NoteRowView`'s author `Text`.
    private func waitForResolvedAuthorName(
        _ app: XCUIApplication,
        timeout: TimeInterval
    ) -> XCUIElement? {
        let names = app.staticTexts.matching(identifier: "timeline-author-name")
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            for element in names.allElementsBoundByIndex.prefix(20) {
                let label = element.label
                if !label.isEmpty, !isShortHex(label) {
                    return element
                }
            }
            _ = names.firstMatch.waitForExistence(timeout: 0.25)
        }
        return nil
    }

    /// Tier 2 — flagship regression. Author display names must never regress to
    /// shortHex after they have once resolved, including across a feed→Settings→
    /// feed round-trip.
    func testProfileName_persistsThroughNavRoundtrip() throws {
        let app = launchFeedApp()

        // 1. Wait for an author label that has fully resolved (NOT shortHex).
        guard let resolved = waitForResolvedAuthorName(app, timeout: 25) else {
            throw XCTSkip("No author name resolved to a display name within 25s — cannot assert regression")
        }
        let resolvedName = resolved.label
        XCTAssertFalse(
            isShortHex(resolvedName),
            "captured name is still shortHex (\(resolvedName)) — assertion would be vacuous"
        )

        // 2. Round-trip: Settings → Home.
        app.tabBars.buttons["Settings"].tap()
        XCTAssertTrue(app.tabBars.buttons["Home"].waitForExistence(timeout: 5))
        app.tabBars.buttons["Home"].tap()
        XCTAssertTrue(waitForHomeTimeline(app, timeout: 5))

        // 3. For 2s (~8 ticks at 4Hz) the SAME label that resolved in step 1 must
        //    NEVER show as shortHex. We assert against the captured `resolved`
        //    element (bound by index, re-queried on each access) — NOT
        //    `firstMatch`, because row 0's author may be a permanently-unresolved
        //    pubkey (no kind:0 metadata) and would false-positive the regression.
        //    The inverted predicate auto-polls and fails if the label ever matches.
        XCTAssertTrue(resolved.waitForExistence(timeout: 5), "captured author label vanished after round-trip")
        let regressed = NSPredicate(
            format: ChirpUITests.shortHexPredicateFormat,
            ChirpUITests.shortHexRegex
        )
        let noRegression = XCTNSPredicateExpectation(predicate: regressed, object: resolved)
        noRegression.isInverted = true
        let result = XCTWaiter.wait(for: [noRegression], timeout: 2.0)
        XCTAssertEqual(
            result, .completed,
            "author name regressed to shortHex (\(resolved.label)) during nav round-trip — FLICKER DEFECT"
        )

        // 4. The same label must remain at the resolved display name.
        XCTAssertEqual(
            resolved.label,
            resolvedName,
            "author name did not remain '\(resolvedName)' after round-trip — was '\(resolved.label)'"
        )
    }

    /// Tier 2 — feed content must never blank out during a Settings→Home
    /// round-trip.
    func testFeed_doesNotBlankDuringNav() throws {
        let app = launchFeedApp()

        // 1. Assert ≥1 feed row with real content (an author label exists).
        let authorNames = app.staticTexts.matching(identifier: "timeline-author-name")
        XCTAssertTrue(authorNames.firstMatch.waitForExistence(timeout: 15), "no feed rows rendered")
        XCTAssertGreaterThan(authorNames.count, 0, "feed had no author labels before nav")

        // 2. Round-trip: Settings → Home, back-to-back with no settle delay.
        app.tabBars.buttons["Settings"].tap()
        XCTAssertTrue(app.tabBars.buttons["Home"].waitForExistence(timeout: 5))
        app.tabBars.buttons["Home"].tap()

        // 3. Feed rows must return without manual refresh after the tab round-trip.
        XCTAssertTrue(waitForHomeTimeline(app, timeout: 5), "feed did not recover after Settings→Home round-trip")
    }

    /// Tier 3 — scroll performance gate.
    /// Gate: ≥58fps / hitch <5ms/s — set baseline on first run via the Xcode
    /// performance-result baseline UI (Edit Test → set average). Until a baseline
    /// is recorded this only collects the metric and never fails.
    func testScrollPerformance() throws {
        let app = launchFeedApp()
        // Ensure rows are present so the swipe actually decelerates over content.
        XCTAssertTrue(
            app.staticTexts.matching(identifier: "timeline-author-name").firstMatch
                .waitForExistence(timeout: 15)
        )

        measure(metrics: [XCTOSSignpostMetric.scrollDecelerationMetric]) {
            app.swipeUp(velocity: .fast)
        }
    }

    /// Tier 3 — nav-transition performance gate (Settings↔Home).
    /// Gate: transition animation should stay hitch-free — set baseline on first
    /// run. Collects `navigationTransitionMetric`; no failure until a baseline is
    /// recorded.
    func testNavTransitionPerformance() throws {
        let app = launchFeedApp()
        XCTAssertTrue(
            app.staticTexts.matching(identifier: "timeline-author-name").firstMatch
                .waitForExistence(timeout: 15)
        )

        let settingsTab = app.tabBars.buttons["Settings"]
        let homeTab = app.tabBars.buttons["Home"]
        XCTAssertTrue(settingsTab.waitForExistence(timeout: 5))

        measure(metrics: [XCTOSSignpostMetric.navigationTransitionMetric]) {
            settingsTab.tap()
            homeTab.tap()
        }
    }

    private func waitForNumericValue(
        _ element: XCUIElement,
        greaterThan threshold: Int,
        timeout: TimeInterval
    ) -> Bool {
        let predicate = NSPredicate { _, _ in element.label.numericValue > threshold }
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: nil)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
    }

    private func isolatedKeychainService(_ suffix: String) -> String {
        "io.f7z.chirp.uitests.\(suffix).\(UUID().uuidString)"
    }

    private func launchApp(keychainService: String) -> XCUIApplication {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_TEST_KEYCHAIN_SERVICE"] = keychainService
        app.launch()
        XCTAssertEqual(app.wait(for: .runningForeground, timeout: 10), true)
        return app
    }

    private func assertSignedIn(_ app: XCUIApplication, timeout: TimeInterval) {
        if app.staticTexts["Your timeline"].waitForExistence(timeout: timeout) {
            return
        }
        if app.tabBars.buttons["Settings"].waitForExistence(timeout: 2) {
            return
        }
        XCTFail("App did not reach the signed-in shell")
    }
}

private extension String {
    var numericValue: Int {
        let digits = filter(\.isNumber)
        return Int(digits) ?? 0
    }
}
