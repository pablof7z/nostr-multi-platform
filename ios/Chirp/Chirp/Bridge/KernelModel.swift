import Foundation
import SwiftUI
import os.log

private let kmLog = Logger(subsystem: "com.example.Chirp", category: "KernelModel")

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
    @Published private(set) var metrics: KernelMetrics?
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
    // NIP-47 wallet state
    @Published private(set) var walletStatus: WalletStatusData?
    // Perf diagnostics (ported from NmpStress goals).
    @Published private(set) var logicalInterests: [LogicalInterestStatus] = []
    @Published private(set) var wireSubscriptions: [WireSubscriptionStatus] = []
    @Published private(set) var logs: [String] = []
    @Published private(set) var appMetrics = AppRuntimeMetrics()
    @Published var visibleLimit: UInt32 = 80
    @Published var emitHz: UInt32 = 4
    // NIP-46 bunker handshake progress (Stage 3 backend emits this).
    // Live data once Stage 3 lands; see snapshot field `bunker_handshake`.
    @Published private(set) var bunkerHandshake: BunkerHandshake?

    var hasActiveAccount: Bool { activeAccount != nil }

    private let kernel = KernelHandle()
    private var lastLogicalInterestSummary = ""

    /// Platform capability implementations injected for the kernel to use.
    let capabilities = NmpPulseCapabilities()

    init() {
        if let v = ProcessInfo.processInfo.environment["NMP_VISIBLE_LIMIT"].flatMap(UInt32.init) {
            visibleLimit = v
        }
        if let v = ProcessInfo.processInfo.environment["NMP_EMIT_HZ"].flatMap(UInt32.init) {
            emitHz = v
        }
        kernel.listen { [weak self] result in
            Task { @MainActor [weak self] in
                self?.apply(result: result)
            }
        }
    }

    func start() {
        guard !isRunning else { return }
        capabilities.start()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        isRunning = true
        // UITest affordance: NMP_TEST_NSEC auto-signs-in without driving onboarding.
        if let nsec = ProcessInfo.processInfo.environment["NMP_TEST_NSEC"] {
            kernel.signInNsec(nsec)
        }
    }

    func stop() {
        kernel.stop()
        capabilities.stop()
        isRunning = false
    }

    func resetAndRestart() {
        kernel.reset()
        items = []
        threadView = nil
        metrics = nil
        rev = 0
        relayStatuses = []
        logicalInterests = []
        wireSubscriptions = []
        logs = []
        appMetrics = AppRuntimeMetrics()
        lastLogicalInterestSummary = ""
        capabilities.start()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        isRunning = true
    }

    func applyConfiguration() {
        kernel.configure(visibleLimit: visibleLimit, emitHz: emitHz)
    }

    func openAuthor(pubkey: String) {
        kernel.openAuthor(pubkey: pubkey)
    }

    func closeAuthor(pubkey: String) {
        kernel.closeAuthor(pubkey: pubkey)
    }

    func openThread(eventID: String) {
        kernel.openThread(eventID: eventID)
    }

    func closeThread(eventID: String) {
        kernel.closeThread(eventID: eventID)
    }

    func claimProfile(pubkey: String, consumerID: String) {
        kernel.claimProfile(pubkey: pubkey, consumerID: consumerID)
    }

    func releaseProfile(pubkey: String, consumerID: String) {
        kernel.releaseProfile(pubkey: pubkey, consumerID: consumerID)
    }

    func openFirehose(tag: String) {
        kernel.openFirehose(tag: tag)
    }

    // ── T66a command surface ──────────────────────────────────────────────
    // Every method is a pass-through to a real kernel dispatch. No Swift-side
    // business logic, no cached state (D5/D8) — the @Published properties
    // above are a pure mirror of the kernel snapshot.

    func signInNsec(_ secret: String) {
        kmLog.info("signInNsec dispatched (len=\(secret.count))")
        kernel.signInNsec(secret)
    }

    func signInBunker(_ uri: String) { kernel.signInBunker(uri) }
    /// Cancel an in-flight NIP-46 handshake. Stage 4 (the broker) backs this
    /// with `nmp_app_cancel_bunker_handshake`, which flips the handshake
    /// thread's cancel flag and tears down its relay client. We also clear
    /// the local mirror so the sheet resets immediately; the next snapshot
    /// will reconcile through the broker's `idle` progress event.
    func cancelBunkerHandshake() {
        kernel.cancelBunkerHandshake()
        bunkerHandshake = nil
    }
    func createAccount() {
        kmLog.info("createAccount dispatched")
        kernel.createAccount()
    }
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

    // ── NIP-47 wallet commands ────────────────────────────────────────────────
    func walletConnect(uri: String) { kernel.walletConnect(uri: uri) }
    func walletDisconnect() { kernel.walletDisconnect() }
    func walletPayInvoice(bolt11: String, amountMsats: UInt64? = nil) {
        kernel.walletPayInvoice(bolt11: bolt11, amountMsats: amountMsats)
    }

    // ── T118 / G3 — scenePhase pass-through ───────────────────────────────
    //
    // `ChirpApp` observes `@Environment(\.scenePhase)` and routes the OS
    // event here. The kernel decides what each phase MEANS (D7); the model
    // is a pure pass-through — no state, no policy.

    /// iOS `.active` — app became visible / interactive. On a meaningful
    /// `Background→Foreground` transition the kernel fans
    /// `TriggerEvent::Foreground` through its registered observer so the
    /// NIP-77 reconciler resumes from the persisted watermark.
    func lifecycleForeground() { kernel.lifecycleForeground() }

    /// iOS `.background` — app is no longer visible. Symmetric counterpart;
    /// today no in-kernel consumer reacts (NIP-77 has no Background trigger
    /// variant), but the hook is in place for future close-idle-sockets
    /// policy.
    func lifecycleBackground() { kernel.lifecycleBackground() }

    private func apply(result: KernelUpdateResult) {
        let update = result.update
        guard update.rev > rev else { return }

        let applyStart = ContinuousClock.now
        let callbackToApplyMicros = result.callbackReceivedAt.duration(to: applyStart).microseconds

        if update.activeAccount != activeAccount {
            kmLog.info("apply: activeAccount changing \(self.activeAccount ?? "nil") → \(update.activeAccount ?? "nil")")
        }
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
        walletStatus = update.walletStatus
        bunkerHandshake = update.bunkerHandshake
        // Perf diagnostics.
        if let li = update.logicalInterests { logicalInterests = li }
        if let ws = update.wireSubscriptions { wireSubscriptions = ws }
        if let lg = update.logs { logs = lg }

        let logicalInterestSummary = logicalInterests
            .map { "\($0.key)=\($0.state)[\($0.cacheCoverage)]" }
            .joined(separator: " | ")
        if !logicalInterestSummary.isEmpty, logicalInterestSummary != lastLogicalInterestSummary {
            lastLogicalInterestSummary = logicalInterestSummary
            print("NMP_DIAG logical_interests rev=\(update.rev) \(logicalInterestSummary)")
        }

        let applyMicros = applyStart.duration(to: .now).microseconds
        let callbackToAppliedMicros = result.callbackReceivedAt.duration(to: .now).microseconds
        appMetrics.record(
            decodeMicros: result.decodeMicros,
            callbackToApplyMicros: callbackToApplyMicros,
            applyMicros: applyMicros,
            callbackToAppliedMicros: callbackToAppliedMicros,
            payloadBytes: result.payloadBytes
        )
        let insertedCount = update.inserted?.count ?? 0
        let updatedCount = update.updated?.count ?? 0
        let removedCount = update.removed?.count ?? 0
        print(
            "NMP_PERF swift_apply rev=\(update.rev) total_events=\(update.metrics.eventsRx) batch_events=\(update.metrics.eventsSinceLastUpdate) inserted=\(insertedCount) updated=\(updatedCount) removed=\(removedCount) visible=\(update.metrics.visibleItems) payload_bytes=\(result.payloadBytes) rust_event_to_emit_ms=\(update.metrics.lastEventToEmitMs.map(String.init) ?? "none") decode_us=\(result.decodeMicros) callback_to_apply_us=\(callbackToApplyMicros) apply_us=\(applyMicros) callback_to_applied_us=\(callbackToAppliedMicros)"
        )

        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }
}

// ─── Swift-side timing accumulator ───────────────────────────────────────

struct AppRuntimeMetrics {
    private(set) var updatesApplied = 0
    private(set) var lastDecodeMicros = 0
    private(set) var lastCallbackToApplyMicros = 0
    private(set) var lastApplyMicros = 0
    private(set) var lastCallbackToAppliedMicros = 0
    private(set) var maxDecodeMicros = 0
    private(set) var maxCallbackToApplyMicros = 0
    private(set) var maxApplyMicros = 0
    private(set) var maxCallbackToAppliedMicros = 0
    private(set) var lastPayloadBytes = 0

    mutating func record(
        decodeMicros: Int,
        callbackToApplyMicros: Int,
        applyMicros: Int,
        callbackToAppliedMicros: Int,
        payloadBytes: Int
    ) {
        updatesApplied += 1
        lastDecodeMicros = decodeMicros
        lastCallbackToApplyMicros = callbackToApplyMicros
        lastApplyMicros = applyMicros
        lastCallbackToAppliedMicros = callbackToAppliedMicros
        maxDecodeMicros = max(maxDecodeMicros, decodeMicros)
        maxCallbackToApplyMicros = max(maxCallbackToApplyMicros, callbackToApplyMicros)
        maxApplyMicros = max(maxApplyMicros, applyMicros)
        maxCallbackToAppliedMicros = max(maxCallbackToAppliedMicros, callbackToAppliedMicros)
        lastPayloadBytes = payloadBytes
    }
}
