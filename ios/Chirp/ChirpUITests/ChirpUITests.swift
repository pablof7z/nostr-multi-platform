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

    func testCreateAccountAndCompleteReplMlsConversation() throws {
        let app = XCUIApplication()
        app.launchEnvironment["NMP_VISIBLE_LIMIT"] = "80"
        app.launchEnvironment["NMP_EMIT_HZ"] = "4"
        app.launch()

        let nonce = String(UUID().uuidString.replacingOccurrences(of: "-", with: "").prefix(8))
        let deviceNpub = try createFreshAccount(app: app, nonce: nonce)

        let groupName = "DeviceMLS\(nonce)"
        let hostMessage = "repl hello from host \(nonce)"
        let phoneMessage = "phone hello from device \(nonce)"

        let createOutput = try runRepl([
            "set-app-relays wss://relay.primal.net",
            "create-account Repl\(nonce)",
            "mls-init",
            "mls-create \(groupName)",
            "quit",
        ])
        let hostNsec = try capture(pattern: #"nsec:\s+(nsec1[0-9a-z]+)"#, in: createOutput)
        let groupID = try capture(pattern: #"group_id:\s+([0-9a-f]+)"#, in: createOutput)

        let inviteOutput = try runRepl([
            "set-app-relays wss://relay.primal.net",
            "load-key \(hostNsec)",
            "mls-init",
            "mls-invite \(groupID) \(deviceNpub)",
            "mls-send \(groupID) \(hostMessage)",
            "quit",
        ], timeout: 120)
        XCTAssertTrue(inviteOutput.contains("invited \(deviceNpub)"))
        XCTAssertTrue(inviteOutput.contains("sent message"))

        app.tabBars.buttons["Groups"].tap()
        let accept = app.buttons.matching(
            NSPredicate(format: "identifier BEGINSWITH %@", "marmot-accept-invite-")
        ).firstMatch
        XCTAssertTrue(accept.waitForExistence(timeout: 60), "MLS invite did not arrive")
        accept.tap()

        let group = app.buttons["marmot-group-row-\(groupID)"]
        XCTAssertTrue(group.waitForExistence(timeout: 30), "accepted MLS group did not appear")
        group.tap()

        XCTAssertTrue(app.staticTexts[hostMessage].waitForExistence(timeout: 60))
        let editor = app.textViews["marmot-message-editor"]
        XCTAssertTrue(editor.waitForExistence(timeout: 10))
        editor.tap()
        editor.typeText(phoneMessage)
        app.buttons["marmot-send-button"].tap()
        XCTAssertTrue(app.staticTexts[phoneMessage].waitForExistence(timeout: 30))

        print("NMP_DEVICE_MLS_CHAT npub=\(deviceNpub) group=\(groupID) received='\(hostMessage)' sent='\(phoneMessage)'")
    }

    // ── Wait helpers ──────────────────────────────────────────────────────────

    private func createFreshAccount(app: XCUIApplication, nonce: String) throws -> String {
        let onboardingField = firstExisting([
            app.textFields["onboarding-display-name"],
            app.textFields["onboarding-display-name-field"],
        ], timeout: 5)
        if let field = onboardingField {
            field.tap()
            field.typeText("Device MLS \(nonce)")
            let createButton = firstExisting([
                app.buttons["onboarding-create-account"],
                app.buttons["onboarding-submit-create-account-button"],
            ], timeout: 5)
            XCTAssertNotNil(createButton, "onboarding create account button missing")
            createButton?.tap()
        } else {
            XCTAssertTrue(app.tabBars.buttons["Settings"].waitForExistence(timeout: 20))
            app.tabBars.buttons["Settings"].tap()
            app.buttons["Accounts"].tap()
            app.buttons["Add account"].tap()
            app.buttons["New identity"].tap()
            app.buttons["create-new-identity-button"].tap()
        }

        app.tabBars.buttons["Settings"].tap()
        if !app.navigationBars["Accounts"].exists {
            app.buttons["Accounts"].tap()
        }
        let row = app.buttons["account-row-active"]
        XCTAssertTrue(row.waitForExistence(timeout: 20))
        guard let npub = row.value as? String, npub.hasPrefix("npub1") else {
            throw XCTSkip("active account row did not expose full npub")
        }
        print("NMP_DEVICE_LIVE_ACCOUNT npub=\(npub)")
        return npub
    }

    private func firstExisting(_ elements: [XCUIElement], timeout: TimeInterval) -> XCUIElement? {
        for element in elements {
            if element.waitForExistence(timeout: timeout) {
                return element
            }
        }
        return nil
    }

    private func runRepl(_ commands: [String], timeout: TimeInterval = 90) throws -> String {
        let repo = ProcessInfo.processInfo.environment["NMP_REPO_ROOT"]
            ?? "/Users/pablofernandez/Work/nostr-multi-platform"
        let repl = URL(fileURLWithPath: repo).appendingPathComponent("target/debug/nmp-repl")
        let process = Process()
        process.executableURL = repl
        process.currentDirectoryURL = URL(fileURLWithPath: repo)

        let input = Pipe()
        let output = Pipe()
        process.standardInput = input
        process.standardOutput = output
        process.standardError = output

        let done = DispatchSemaphore(value: 0)
        process.terminationHandler = { _ in done.signal() }

        try process.run()
        input.fileHandleForWriting.write((commands.joined(separator: "\n") + "\n").data(using: .utf8)!)
        input.fileHandleForWriting.closeFile()

        if done.wait(timeout: .now() + timeout) == .timedOut {
            process.terminate()
            _ = done.wait(timeout: .now() + 5)
            XCTFail("nmp-repl timed out")
        }
        let data = output.fileHandleForReading.readDataToEndOfFile()
        let text = String(data: data, encoding: .utf8) ?? ""
        XCTAssertEqual(process.terminationStatus, 0, text)
        return text
    }

    private func capture(pattern: String, in text: String) throws -> String {
        let regex = try NSRegularExpression(pattern: pattern)
        let range = NSRange(text.startIndex..<text.endIndex, in: text)
        guard let match = regex.firstMatch(in: text, range: range),
              let captureRange = Range(match.range(at: 1), in: text) else {
            XCTFail("missing pattern \(pattern) in output:\n\(text)")
            return ""
        }
        return String(text[captureRange])
    }

    private func waitForLabel(
        _ element: XCUIElement,
        equals expected: String,
        timeout: TimeInterval
    ) -> Bool {
        let predicate = NSPredicate { _, _ in element.label == expected }
        let expectation = XCTNSPredicateExpectation(predicate: predicate, object: nil)
        return XCTWaiter.wait(for: [expectation], timeout: timeout) == .completed
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
        "com.example.Chirp.uitests.\(suffix).\(UUID().uuidString)"
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
