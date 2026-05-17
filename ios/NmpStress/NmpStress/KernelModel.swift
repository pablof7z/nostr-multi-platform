import Foundation
import SwiftUI

@MainActor
final class KernelModel: ObservableObject {
    @Published private(set) var isRunning = false
    @Published private(set) var rev: UInt64 = 0
    @Published private(set) var relayUrl = "wss://relay.primal.net"
    @Published private(set) var testNpub = ""
    @Published private(set) var profile: ProfileCard?
    @Published private(set) var items: [TimelineItem] = []
    @Published private(set) var authorView: AuthorViewPayload?
    @Published private(set) var threadView: ThreadViewPayload?
    @Published private(set) var metrics: KernelMetrics?
    @Published private(set) var relayStatus: RelayStatus?
    @Published private(set) var relayStatuses: [RelayStatus] = []
    @Published private(set) var logicalInterests: [LogicalInterestStatus] = []
    @Published private(set) var wireSubscriptions: [WireSubscriptionStatus] = []
    @Published private(set) var logs: [String] = []
    @Published private(set) var appMetrics = AppRuntimeMetrics()
    @Published var visibleLimit: UInt32 = 80
    @Published var emitHz: UInt32 = 4
    @Published private(set) var diagnosticFirehoseTag: String?

    private let kernel = KernelHandle()
    private var authorViewCache: [String: ProjectionCacheEntry<AuthorViewPayload>] = [:]
    private var threadViewCache: [String: ProjectionCacheEntry<ThreadViewPayload>] = [:]
    private let projectionCacheTTL: TimeInterval = 60
    private var lastLogicalInterestSummary = ""

    init() {
        let environment = ProcessInfo.processInfo.environment
        if let value = environment["NMP_VISIBLE_LIMIT"].flatMap(UInt32.init) {
            visibleLimit = value
        }
        if let value = environment["NMP_EMIT_HZ"].flatMap(UInt32.init) {
            emitHz = value
        }
        if let value = Self.launchArgument(after: "--nmp-visible-limit").flatMap(UInt32.init) {
            visibleLimit = value
        }
        if let value = Self.launchArgument(after: "--nmp-emit-hz").flatMap(UInt32.init) {
            emitHz = value
        }
        if let tag = Self.launchArgument(after: "--nmp-diagnostic-firehose") {
            diagnosticFirehoseTag = tag
        }
        kernel.listen { [weak self] result in
            Task { @MainActor [weak self] in
                self?.apply(result: result)
            }
        }
    }

    private static func launchArgument(after flag: String) -> String? {
        if let index = CommandLine.arguments.firstIndex(of: flag),
           CommandLine.arguments.indices.contains(index + 1) {
            return CommandLine.arguments[index + 1]
        }
        return nil
    }

    func start() {
        guard !isRunning else {
            return
        }
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        isRunning = true
        if let diagnosticFirehoseTag {
            kernel.openFirehose(tag: diagnosticFirehoseTag)
        }
    }

    func stop() {
        kernel.stop()
        isRunning = false
    }

    func resetAndRestart() {
        kernel.reset()
        items = []
        authorView = nil
        threadView = nil
        metrics = nil
        rev = 0
        relayStatus = nil
        relayStatuses = []
        logicalInterests = []
        wireSubscriptions = []
        logs = []
        authorViewCache = [:]
        threadViewCache = [:]
        appMetrics = AppRuntimeMetrics()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        isRunning = true
        if let diagnosticFirehoseTag {
            kernel.openFirehose(tag: diagnosticFirehoseTag)
        }
    }

    func applyConfiguration() {
        kernel.configure(visibleLimit: visibleLimit, emitHz: emitHz)
    }

    func openAuthor(pubkey: String) {
        kernel.openAuthor(pubkey: pubkey)
    }

    func openThread(eventID: String) {
        kernel.openThread(eventID: eventID)
    }

    func closeAuthor(pubkey: String) {
        kernel.closeAuthor(pubkey: pubkey)
    }

    func closeThread(eventID: String) {
        kernel.closeThread(eventID: eventID)
    }

    func cachedAuthorView(pubkey: String) -> AuthorViewPayload? {
        cachedValue(for: pubkey, in: authorViewCache)
    }

    func cachedThreadView(eventID: String) -> ThreadViewPayload? {
        cachedValue(for: eventID, in: threadViewCache)
    }

    private func apply(result: KernelUpdateResult) {
        guard result.update.rev > rev else {
            return
        }

        let applyStart = ContinuousClock.now
        let update = result.update
        let callbackToApplyMicros = result.callbackReceivedAt.duration(to: applyStart).microseconds
        rev = update.rev
        isRunning = update.running
        relayUrl = update.relayUrl
        testNpub = update.testNpub
        profile = update.profile
        items = update.items
        authorView = update.authorView
        threadView = update.threadView
        metrics = update.metrics
        relayStatus = update.relayStatus
        relayStatuses = update.relayStatuses
        logicalInterests = update.logicalInterests
        wireSubscriptions = update.wireSubscriptions
        logs = update.logs
        if let authorView = update.authorView {
            authorViewCache[authorView.pubkey] = ProjectionCacheEntry(value: authorView, storedAt: Date())
        }
        if let threadView = update.threadView {
            threadViewCache[threadView.focusedEventId] = ProjectionCacheEntry(value: threadView, storedAt: Date())
        }
        purgeProjectionCaches()
        let logicalInterestSummary = update.logicalInterests
            .map { "\($0.key)=\($0.state)[\($0.cacheCoverage)]" }
            .joined(separator: " | ")
        if logicalInterestSummary != lastLogicalInterestSummary {
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
        print(
            "NMP_PERF swift_apply rev=\(update.rev) total_events=\(update.metrics.eventsRx) batch_events=\(update.metrics.eventsSinceLastUpdate) inserted=\(update.inserted.count) updated=\(update.updated.count) removed=\(update.removed.count) visible=\(update.metrics.visibleItems) payload_bytes=\(result.payloadBytes) rust_event_to_emit_ms=\(update.metrics.lastEventToEmitMs.map(String.init) ?? "none") decode_us=\(result.decodeMicros) callback_to_apply_us=\(callbackToApplyMicros) apply_us=\(applyMicros) callback_to_applied_us=\(callbackToAppliedMicros)"
        )
    }

    private func cachedValue<Value>(for key: String, in cache: [String: ProjectionCacheEntry<Value>]) -> Value? {
        guard let entry = cache[key], Date().timeIntervalSince(entry.storedAt) <= projectionCacheTTL else {
            return nil
        }
        return entry.value
    }

    private func purgeProjectionCaches() {
        let now = Date()
        authorViewCache = authorViewCache.filter { now.timeIntervalSince($0.value.storedAt) <= projectionCacheTTL }
        threadViewCache = threadViewCache.filter { now.timeIntervalSince($0.value.storedAt) <= projectionCacheTTL }
    }
}

private struct ProjectionCacheEntry<Value> {
    let value: Value
    let storedAt: Date
}

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
