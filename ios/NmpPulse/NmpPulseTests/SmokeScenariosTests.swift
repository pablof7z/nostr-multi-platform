import XCTest
@testable import NmpPulse

/// T66c — e2e smoke scenarios that exercise the *hard parts* of the kernel
/// through the real FFI surface, the same code path the SwiftUI screens drive.
///
/// These are unit-test-bundle tests (the only test target `project.yml`
/// defines is `NmpPulseCapabilityTests`, a `bundle.unit-test` — there is no
/// UI-testing target and `project.yml` is out of scope for this task). So the
/// scenarios drive `KernelModel` directly rather than tapping SwiftUI views.
/// The companion `docs/perf/pulse/smoke/README.md` documents the matching
/// xcode-MCP simulator walkthrough + screenshots.
///
/// They hit real relays (`relay.primal.net` + `purplepag.es` — the kernel's
/// fixed `RelayRole::all()` pair) so, like the Rust `#[ignore]` smoke suite,
/// they are gated behind `NMP_SMOKE=1` and skipped otherwise. A skipped
/// scenario is honest non-execution, never a fake pass.
///
/// Two design choices forced by empirically-observed crashes (both are
/// REPORT-finding gaps documented in the smoke README; `crates/**` is
/// off-limits this task so neither is fixed in Rust here):
///
///  1. **Process-shared kernel.** `nmp_app_new()` → `nmp_app_free()` →
///     `nmp_app_new()` in one process SEGVs when relay sockets were live
///     during the free (`crates/nmp-core/src/ffi/mod.rs:62-104`,
///     `crates/nmp-core/src/relay_worker.rs`). The suite uses ONE shared
///     `KernelModel`; scenarios run additively in XCTest's default
///     alphabetical method order (2 < 3 < 4 < 6).
///  2. **Polling, never Combine sinks.** Attaching `model.$x.sink`
///     subscribers to the long-lived shared kernel and calling
///     `XCTestExpectation.fulfill()` from them crashed in
///     `swift_task_localValuePopImpl()` (`KERN_INVALID_ADDRESS`): the shared
///     kernel keeps emitting after the async test's task-local context is
///     torn down, so the sink fires into a freed XCTest error-observation
///     scope. All convergence uses the synchronous `waitUntil` poller, which
///     reads `@MainActor` snapshot state on the test's own task and registers
///     no escaping callback into the kernel's publisher chain.
///
/// Honest assertion-scope gaps:
///  - **NIP-77 diagnostic is dead.** `crates/nmp-core/src/kernel/status.rs:22`
///    hardcodes `nip77_negentropy: "unknown"`. Scenario 3 asserts the
///    *reachable subset*: events arrive + a relay connects (REQ-fallback path
///    proven over a real socket).
///  - **`nmp_app_add_relay` does not open a wire socket.**
///    `crates/nmp-core/src/actor/commands/relays.rs::add_relay` only mutates
///    the `relay_edit_rows` projection; the wire layer iterates the fixed
///    `RelayRole::all()` pair, neither AUTH-required. Scenario 4 asserts the
///    reachable subset (AUTH state machine wired, default relays report
///    `not_required`, no false challenge). Full NIP-42 transition coverage
///    lives in the Rust `crates/nmp-core/src/kernel/auth_tests.rs` suite.
@MainActor
final class SmokeScenariosTests: XCTestCase {
    /// Pre-baked fixture key (matches `crates/nmp-testing/fixtures/test_nsec.txt`).
    private let nsecA = "nsec12c7ujxnnut2dnahjjsecq79507fg2p2h7ul4a3rqepg5vyk8c9lqyc30gw"
    /// Second independent key for the multi-session scenario.
    private let nsecB = "nsec1uc3t3cp0nn976n4n5dk4zr2vpqrgttldj3e5v3al8p0sarazra3qhv7mxm"
    /// jb55 — a prolific pubkey, used as a follow target in scenario 2.
    private let followTarget =
        "32e1827635450ebb3c5a7d12c1f8e7b2b514439ac10a67eef3d9fd9c5c68e245"

    @MainActor private static var sharedModel: KernelModel?

    @MainActor private static func model() -> KernelModel {
        if let m = sharedModel { return m }
        let m = KernelModel()
        m.start()
        sharedModel = m
        return m
    }

    private var model: KernelModel { Self.sharedModel! }

    /// `async throws` overrides inherit the class's `@MainActor` isolation in
    /// Swift 6 (the synchronous `setUpWithError` did not — mutating
    /// `@MainActor` state from a nonisolated context raced the FFI callback
    /// onto the wrong thread).
    override func setUp() async throws {
        try await super.setUp()
        try XCTSkipUnless(
            ProcessInfo.processInfo.environment["NMP_SMOKE"] == "1",
            "Set NMP_SMOKE=1 to run the real-relay smoke scenarios "
                + "(network-bound + slow, like the Rust #[ignore] suite).")
        _ = Self.model()  // lazily create the shared kernel on first scenario.
    }

    // The shared model is NEVER stopped or nil'd — process exit cleans it up.
    // Stopping it would re-trigger the free-then-recreate SEGV.

    // MARK: - Scenario 2 — kind:3 follow-list change → timeline re-targets

    /// Sign in, publish a kind:3 follow-list mutation via `nmp_app_follow`,
    /// and assert the kernel (a) accepts the kind:3 into the publish queue and
    /// (b) re-plans the timeline (`rev` advances past the follow). Per
    /// `ios/NmpPulse/README.md` the timeline retargets via
    /// `open_author(active_pubkey)`; the per-followed-author fan-out is a
    /// documented kernel follow-up, not a Swift gap.
    func testScenario2_FollowListChangeRetargetsTimeline() async throws {
        try await signIn(nsecA, label: "scenario-2")

        let revBeforeFollow = model.rev
        model.follow(followTarget)

        try await waitUntil(timeout: 25, "kind:3 enters publish queue") { [self] in
            model.publishQueue.contains { $0.kind == 3 }
        }
        XCTAssertTrue(
            model.publishQueue.contains { $0.kind == 3 },
            "kind:3 follow mutation never reached the publish queue")

        try await waitUntil(timeout: 20, "rev advances after follow") { [self] in
            model.rev > revBeforeFollow
        }
        XCTAssertGreaterThan(
            model.rev, revBeforeFollow,
            "timeline did not re-plan after the kind:3 follow mutation")
    }

    // MARK: - Scenario 3 — NIP-77 path on connect (REQ-fallback asserted)

    /// The kernel connects and backfills. The NIP-77 negentropy diagnostic is
    /// hardcoded `"unknown"` (REPORT gap), so we assert the *reachable
    /// subset*: a populated timeline + a live relay connection — the
    /// REQ-fallback path proven end-to-end over a real socket.
    ///
    /// SHARED-STATE NOTE: because the suite uses one process-shared kernel,
    /// this no longer proves "on the *first* connect" — by the time it runs
    /// the kernel may already be connected from scenario 2. It still proves
    /// the REQ-fallback path carries events over a real socket, the
    /// load-bearing claim. Strict "cold-open negentropy-first" is unreachable
    /// from the app surface until the diagnostic is un-hardcoded (REPORT gap).
    func testScenario3_ConnectBackfillReqFallback() async throws {
        try await signIn(nsecA, label: "scenario-3")

        try await waitUntil(timeout: 45, "timeline backfills") { [self] in
            !model.items.isEmpty
        }
        XCTAssertFalse(
            model.items.isEmpty,
            "no events arrived — REQ-fallback backfill path failed")

        try await waitUntil(timeout: 20, "a relay reaches `connected`") { [self] in
            model.relayStatuses.contains { $0.connection == "connected" }
        }
        XCTAssertTrue(
            model.relayStatuses.contains { $0.connection == "connected" },
            "no relay reached `connected` — connect path not exercised")

        // Every relay must expose a non-empty connection state (no silent
        // black-hole). This also pins the dead NIP-77 diagnostic: if a future
        // build starts surfacing a real negentropy verdict the constant-string
        // assumption in the README becomes stale and should be revisited.
        for relay in model.relayStatuses {
            XCTAssertFalse(
                relay.connection.isEmpty,
                "relay \(relay.relayUrl) has no connection state")
        }
    }

    // MARK: - Scenario 4 — NIP-42 AUTH (reachable subset asserted)

    /// The default wire relays are not AUTH-required and `nmp_app_add_relay`
    /// does not open a third socket (REPORT gap), so a real NIP-42 handshake
    /// cannot be driven in-sim from the app surface. Assert the reachable
    /// subset: the AUTH state machine is wired and the default relays settle
    /// to `not_required` with no spurious challenge. Full handshake coverage
    /// is the Rust `kernel/auth_tests.rs` suite.
    func testScenario4_Nip42AuthReachableSubset() async throws {
        try await signIn(nsecA, label: "scenario-4")

        try await waitUntil(timeout: 30, "relay statuses populate") { [self] in
            !model.relayStatuses.isEmpty
        }
        XCTAssertFalse(model.relayStatuses.isEmpty, "no relay statuses emitted")

        // Every default relay must expose a valid AUTH-state key. These relays
        // are not auth-required, so `failed` is the only outright bug; the
        // handshake keys are tolerated transiently.
        let validKeys: Set<String> = [
            "not_required", "challenge_received", "authenticating", "authenticated",
        ]
        for relay in model.relayStatuses {
            XCTAssertTrue(
                validKeys.contains(relay.auth),
                "relay \(relay.relayUrl) auth=`\(relay.auth)` — AUTH state "
                    + "machine produced an unexpected/`failed` key against a "
                    + "non-auth relay")
        }
        let allNotRequired = model.relayStatuses.allSatisfy { $0.auth == "not_required" }
        XCTAssertTrue(
            allNotRequired,
            "a default relay issued an AUTH challenge — investigate; states: "
                + model.relayStatuses.map { "\($0.relayUrl)=\($0.auth)" }
                .joined(separator: ", "))
    }

    // MARK: - Scenario 6 — multi-session: add 2nd account + switch active

    /// Ensure account A is active, add account B, switch active to B and back,
    /// asserting the active-account identity flips synchronously each way.
    /// Per D5/D8 there is no Swift-side session state, so a synchronous
    /// `activeAccount` flip (cross-checked against the `accounts` projection's
    /// `isActive`) *is* the feed + compose identity switch — both read the
    /// same kernel snapshot fact.
    func testScenario6_MultiSessionAddAndSwitch() async throws {
        try await signIn(nsecA, label: "scenario-6 / account A")
        let accountAID = try XCTUnwrap(model.activeAccount, "account A has no id")

        // Add account B (same sign-in path the Accounts "+ Add" sheet drives).
        model.signInNsec(nsecB)
        try await waitUntil(timeout: 25, "second account lands") { [self] in
            model.accounts.count >= 2
        }
        let accountBID = try XCTUnwrap(
            model.accounts.first { $0.id != accountAID }?.id,
            "could not resolve account B id")

        // Switch to B — assert a synchronous, observable identity flip.
        try await waitUntil(timeout: 15, "active flips to B") { [self] in
            model.switchActive(accountBID)
            return model.activeAccount == accountBID
        }
        XCTAssertEqual(model.activeAccount, accountBID, "active did not switch to B")
        XCTAssertEqual(
            model.accounts.first { $0.isActive }?.id, accountBID,
            "accounts projection disagrees with activeAccount after switch")

        // Switch back — proves the flip is not one-way and stays consistent.
        try await waitUntil(timeout: 15, "active flips back to A") { [self] in
            model.switchActive(accountAID)
            return model.activeAccount == accountAID
        }
        XCTAssertEqual(model.activeAccount, accountAID, "switch-back to A failed")
        XCTAssertEqual(
            model.accounts.first { $0.isActive }?.id, accountAID,
            "accounts projection disagrees with activeAccount after switch-back")
    }

    // MARK: - Helpers

    /// Drive `nmp_app_signin_nsec` and wait for the kernel to surface an
    /// active account (the exact gate `RootView` uses to leave Onboarding).
    ///
    /// Idempotent for the shared kernel: if an account is already active
    /// (a prior scenario signed in), return without re-dispatching —
    /// re-issuing `signin_nsec(nsecA)` against a kernel that already holds
    /// that identity has undefined dedup/switch semantics from the app side,
    /// so the safe contract is "ensure signed in", not "sign in again".
    private func signIn(_ nsec: String, label: String) async throws {
        if model.hasActiveAccount { return }
        model.signInNsec(nsec)
        try await waitUntil(timeout: 20, "\(label): active account") { [self] in
            model.hasActiveAccount
        }
        XCTAssertTrue(
            model.hasActiveAccount, "\(label): sign-in produced no active account")
    }

    /// Poll a `@MainActor` predicate until true or timeout. The sole
    /// convergence primitive: no Combine subscription is registered into the
    /// shared kernel's publisher chain, so nothing fires after the test's
    /// task-local context is gone (see the class-doc crash note).
    private func waitUntil(
        timeout: TimeInterval,
        _ what: String,
        _ predicate: () -> Bool
    ) async throws {
        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            if predicate() { return }
            try await Task.sleep(nanoseconds: 250_000_000)
        }
        XCTFail("timed out waiting for: \(what)")
    }
}
