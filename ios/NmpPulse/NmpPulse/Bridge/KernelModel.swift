import Foundation
import SwiftUI

/// `@Observable` mirror of the kernel snapshot. The Rust actor pushes JSON
/// updates via the callback; this class decodes them and republishes for
/// SwiftUI consumption.
@MainActor
final class KernelModel: ObservableObject {
    @Published private(set) var isRunning = false
    @Published private(set) var rev: UInt64 = 0
    @Published private(set) var relayUrl: String = ""
    @Published private(set) var testNpub: String = ""
    @Published private(set) var profile: ProfileCard?
    @Published private(set) var items: [TimelineItem] = []
    @Published private(set) var metrics: KernelMetricsLite?
    @Published private(set) var relayStatuses: [RelayStatus] = []
    @Published private(set) var snapshotCount: UInt64 = 0
    @Published private(set) var lastSnapshotAt: Date?
    // T66a projections.
    @Published private(set) var accounts: [AccountSummary] = []
    @Published private(set) var activeAccount: String?
    @Published private(set) var publishQueue: [PublishQueueEntry] = []
    @Published private(set) var lastErrorToast: String?
    @Published private(set) var relayEditRows: [RelayEditRow] = []
    @Published private(set) var threadView: ThreadView?

    var hasActiveAccount: Bool { activeAccount != nil }

    private let kernel = KernelHandle()

    /// Platform capability implementations injected for the kernel to use.
    /// Owns the Keychain-backed keyring; the Onboarding flow persists an
    /// imported key via `capabilities.persistImportedSecret(...)`.
    let capabilities = NmpPulseCapabilities()

    init() {
        kernel.listen { [weak self] update in
            Task { @MainActor [weak self] in
                self?.apply(update: update)
            }
        }
    }

    func start() {
        guard !isRunning else { return }
        capabilities.start()
        kernel.start(visibleLimit: 80, emitHz: 4)
        isRunning = true
    }

    func stop() {
        kernel.stop()
        capabilities.stop()
        isRunning = false
    }

    func openAuthor(pubkey: String) {
        kernel.openAuthor(pubkey: pubkey)
    }

    func openThread(eventID: String) {
        kernel.openThread(eventID: eventID)
    }

    // ── T66a command surface ──────────────────────────────────────────────
    // Every method is a pass-through to a real kernel dispatch. No Swift-side
    // business logic, no cached state (D5/D8) — the @Published properties
    // above are a pure mirror of the kernel snapshot.

    func signInNsec(_ secret: String) {
        // Pure pass-through. At-rest Keychain persistence is T63a's
        // responsibility and must be keyed by the real identity_id the
        // kernel assigns — doing it here with a placeholder id would be
        // Swift-side identity business logic (forbidden) and would clobber
        // one slot per sign-in. The kernel-side capability socket that
        // routes persistence through `capabilities.keyring` is unbuilt
        // (PD-019); persistence lands when that socket graduates.
        kernel.signInNsec(secret)
    }

    func signInBunker(_ uri: String) { kernel.signInBunker(uri) }
    func createAccount() { kernel.createAccount() }
    func switchActive(_ identityID: String) { kernel.switchActive(identityID: identityID) }
    func removeAccount(_ identityID: String) { kernel.removeAccount(identityID: identityID) }
    func publishNote(_ content: String, replyToID: String? = nil) {
        kernel.publishNote(content: content, replyToID: replyToID)
    }
    func react(targetEventID: String, reaction: String = "❤") {
        kernel.react(targetEventID: targetEventID, reaction: reaction)
    }
    func follow(_ pubkey: String) { kernel.follow(pubkey: pubkey) }
    func unfollow(_ pubkey: String) { kernel.unfollow(pubkey: pubkey) }
    func addRelay(url: String, role: String) { kernel.addRelay(url: url, role: role) }
    func removeRelay(url: String) { kernel.removeRelay(url: url) }
    func openTimeline() { kernel.openTimeline() }
    func clearErrorToast() { lastErrorToast = nil }

    private func apply(update: KernelUpdate) {
        guard update.rev > rev else { return }
        rev = update.rev
        isRunning = update.running
        relayUrl = update.relayUrl
        testNpub = update.testNpub
        profile = update.profile
        items = update.items
        metrics = update.metrics
        relayStatuses = update.relayStatuses
        // T66a projections — mirror only; never derive (D8).
        if let a = update.accounts { accounts = a }
        activeAccount = update.activeAccount
        if let q = update.publishQueue { publishQueue = q }
        lastErrorToast = update.lastErrorToast
        if let r = update.relayEditRows { relayEditRows = r }
        threadView = update.threadView
        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }
}
