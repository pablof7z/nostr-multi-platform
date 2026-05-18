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
        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }
}
