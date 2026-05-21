import Foundation
import SwiftUI
import os.log

private let kmLog = Logger(subsystem: "io.f7z.chirp", category: "KernelModel")

/// PR-L (no_print_in_bridge SwiftLint rule): structured replacement for the
/// prior `print("NMP_DIAG …")` / `print("NMP_PERF …")` stdout lines. The
/// dedicated `org.nmp.chirp.diag` subsystem keeps the perf trace filterable
/// without polluting the primary `io.f7z.chirp` stream.
private let diagLog = Logger(subsystem: "org.nmp.chirp.diag", category: "KernelModel")

/// `ObservableObject` mirror of the kernel snapshot. The Rust actor pushes
/// JSON updates via the callback; this class decodes them and republishes
/// for SwiftUI consumption.
///
/// PR-L (KernelModel collapse): every kernel-driven projection lives behind
/// the single `@Published var snapshot: KernelUpdate?` slot; the computed
/// accessors below restore the per-field view-facing API (`model.profile`,
/// `model.items`, …) verbatim. The four genuinely-local mutable slots —
/// `lastErrorToast` (clearable by the toast tap), `appMetrics` (timing
/// accumulator), `pendingActions` / `lastDispatchError` (PR-A) — stay
/// individual `@Published` properties.
@MainActor
final class KernelModel: ObservableObject {

    // ── Snapshot slot — single source of truth for kernel-driven state ──

    /// Latest decoded snapshot. `nil` before the first tick lands.
    @Published private(set) var snapshot: KernelUpdate?

    // ── Local mutable state ──────────────────────────────────────────────

    /// Modular timeline blocks — sourced from `kernel.chirpSnapshot()` rather
    /// than the snapshot JSON, so this stays a standalone `@Published` slot.
    @Published private(set) var modularTimeline: ChirpTimelineSnapshot = .empty
    @Published private(set) var snapshotCount: UInt64 = 0
    @Published private(set) var lastSnapshotAt: Date?
    @Published private(set) var appMetrics = AppRuntimeMetrics()
    /// Snapshot-derived AND user-clearable, so we cannot fold this into the
    /// `snapshot` accessor — the clear gesture has nowhere else to land.
    @Published private(set) var lastErrorToast: String?
    /// PR-A: correlation ids of dispatched actions whose terminal verdict
    /// has not yet arrived in `projections["action_results"]`. Add on accept,
    /// remove on terminal tick.
    @Published private(set) var pendingActions: Set<String> = []
    /// PR-A / PR-L: synchronous dispatch-error toast slot, distinct from the
    /// snapshot-driven `lastErrorToast`.
    @Published private(set) var lastDispatchError: String?
    @Published var visibleLimit: UInt32 = 80
    @Published var emitHz: UInt32 = 4

    // ── Computed projections — read through `snapshot` ────────────────────

    var isRunning: Bool { snapshot?.running ?? false }
    var rev: UInt64 { snapshot?.rev ?? 0 }
    var profile: ProfileCard? { snapshot?.profile }
    var authorView: AuthorProfileSnapshot? { snapshot?.authorView }
    var items: [TimelineItem] { snapshot?.items ?? [] }
    var metrics: KernelMetrics? { snapshot?.metrics }
    var relayStatuses: [RelayStatus] { snapshot?.relayStatuses ?? [] }
    var accounts: [AccountSummary] { snapshot?.accounts ?? [] }
    var activeAccount: String? { snapshot?.activeAccount }
    var publishQueue: [PublishQueueEntry] { snapshot?.publishQueue ?? [] }
    var publishOutbox: [PublishOutboxItem] { snapshot?.publishOutbox ?? [] }
    var outboxSummary: OutboxSummary { snapshot?.outboxSummary ?? .empty }
    var relayEditRows: [RelayEditRow] { snapshot?.relayEditRows ?? [] }
    var settingsHub: SettingsHubSummary { snapshot?.settingsHub ?? .empty }
    var threadView: ThreadView? { snapshot?.threadView }
    var walletStatus: WalletStatusData? { snapshot?.walletStatus }
    var logicalInterests: [LogicalInterestStatus] { snapshot?.logicalInterests ?? [] }
    var wireSubscriptions: [WireSubscriptionStatus] { snapshot?.wireSubscriptions ?? [] }
    var relayDiagnostics: RelayDiagnosticsSnapshot { snapshot?.relayDiagnostics ?? .empty }
    var logs: [String] { snapshot?.logs ?? [] }
    var bunkerHandshake: BunkerHandshake? { snapshot?.bunkerHandshake }
    var nip46Onboarding: Nip46Onboarding? { snapshot?.nip46Onboarding }

    /// Per-author mention payloads — adapted from the wire DTO at read time.
    /// Falls back to `[:]` when an older kernel elides the projection.
    var mentionProfiles: [String: MentionProfile] {
        guard let wire = snapshot?.mentionProfiles else { return [:] }
        return wire.mapValues(MentionProfile.init(wire:))
    }

    var hasActiveAccount: Bool { activeAccount != nil }

    /// O(N) lookup of the active `AccountSummary` (kept on the model so
    /// views never write `.first(where:)` — aim.md §4.5).
    var activeAccountSummary: AccountSummary? {
        guard let id = activeAccount else { return nil }
        for account in accounts where account.id == id { return account }
        return nil
    }

    // ── Stores & capabilities (non-published) ────────────────────────────

    private let kernel = KernelHandle()
    /// Re-entrance guard for `start()`. The snapshot-driven `isRunning`
    /// accessor only flips after the first tick lands, so a re-entrant
    /// `start()` before then would dispatch the FFI twice.
    private var startedKernel = false
    private var lastLogicalInterestSummary = ""
    private var marmotRegistrationRequested = false
    /// Last `(activeAccount, read-eligible-relay-set)` pair published as a
    /// kind:10050 NIP-17 DM-relay list. Prevents re-publish every tick.
    private var lastPublishedDmRelaySet: (account: String, urls: Set<String>)?

    private(set) lazy var marmot = MarmotStore(kernel: kernel)
    private(set) lazy var groupChat = GroupChatStore(groupId: Self.demoGroupId, kernel: kernel)
    private(set) lazy var dmInbox = DmInboxStore(kernel: kernel)

    /// NIP-02 follow list mirror — the active account's kind:3 contact list.
    /// The store registers its read projection (`nmp_app_chirp_register_follow_list`)
    /// in its initializer; that initializer runs on the first snapshot tick
    /// because `apply` below touches `followList` every tick. The store
    /// re-invokes the FFI once the active account is known so the projection's
    /// active-pubkey slot is updated.
    private(set) lazy var followList = FollowListStore(kernel: kernel)

    /// NIP-29 group-discovery + join mirror — the read side of
    /// `JoinGroupView`. Unlike `groupChat` / `dmInbox` this is lazy AND
    /// relay-keyed: registration deferred until the user enters a relay
    /// URL and taps "Search" (the store's `searchGroups` is the trigger).
    /// Until then the snapshot key is unwired and the store stays empty.
    /// Touching it every tick keeps `apply` symmetric with the other
    /// projection mirrors.
    private(set) lazy var discoveredGroups = DiscoveredGroupsStore(kernel: kernel)

    /// The NIP-29 group the group-chat screen reads and posts to. A single
    /// fixed room for the first-consumer proof; a real multi-group app
    /// would thread a chosen `GroupId` through navigation.
    static let demoGroupId = GroupId(
        hostRelayUrl: "wss://relay.groups.nip29.com",
        localId: "chirp-demo")

    let capabilities: ChirpCapabilities

    init() {
        if let service = ProcessInfo.processInfo.environment["NMP_TEST_KEYCHAIN_SERVICE"] {
            capabilities = ChirpCapabilities(keyring: KeychainCapability(service: service))
        } else {
            capabilities = ChirpCapabilities()
        }
        if let v = ProcessInfo.processInfo.environment["NMP_VISIBLE_LIMIT"].flatMap(UInt32.init) {
            visibleLimit = v
        }
        if let v = ProcessInfo.processInfo.environment["NMP_EMIT_HZ"].flatMap(UInt32.init) {
            emitHz = v
        }
        kernel.listen { [weak self] result in
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                MainActor.assumeIsolated { self.apply(result: result) }
            }
        }
        // Register the keychain capability handler before start() so the
        // kernel can route capability requests from the first tick.
        kernel.registerCapabilityHandler(capabilities)
    }

    var onboardingRelayOverride: String? {
        if let relay = Self.launchArgument("CHIRP_MAESTRO_RELAY_URL"), !relay.isEmpty {
            return relay
        }
        return nil
    }

    private static func launchArgument(_ key: String) -> String? {
        let args = ProcessInfo.processInfo.arguments
        for index in args.indices {
            let arg = args[index]
            if arg == key || arg == "-\(key)" {
                let next = args.index(after: index)
                return next < args.endIndex ? args[next] : nil
            }
            let prefixes = ["\(key)=", "-\(key)="]
            if let prefix = prefixes.first(where: { arg.hasPrefix($0) }) {
                return String(arg.dropFirst(prefix.count))
            }
        }
        return UserDefaults.standard.string(forKey: key)
    }

    // ── Lifecycle ────────────────────────────────────────────────────────

    func start() {
        guard !startedKernel else { return }
        startedKernel = true
        capabilities.start()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        kernel.restoreChirpIdentity(testNsec: ProcessInfo.processInfo.environment["NMP_TEST_NSEC"])
    }

    func stop() {
        kernel.stop()
        capabilities.stop()
        startedKernel = false
    }

    func resetAndRestart() {
        kernel.reset()
        // Dropping `snapshot` clears every kernel-driven projection in one
        // move via the computed accessors. Local-only slots clear explicitly.
        snapshot = nil
        // T146 — Reset preserves the observer slot but the grouper retains
        // the prior session's blocks; re-register so it starts empty.
        kernel.reregisterChirpProjection()
        modularTimeline = .empty
        appMetrics = AppRuntimeMetrics()
        lastLogicalInterestSummary = ""
        pendingActions = []
        lastDispatchError = nil
        lastErrorToast = nil
        capabilities.start()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        startedKernel = true
    }

    func applyConfiguration() {
        kernel.configure(visibleLimit: visibleLimit, emitHz: emitHz)
    }

    // ── View/Author/Thread open + close ──────────────────────────────────

    func openAuthor(pubkey: String) { kernel.openAuthor(pubkey: pubkey) }
    func closeAuthor(pubkey: String) { kernel.closeAuthor(pubkey: pubkey) }
    func openThread(eventID: String) { kernel.openThread(eventID: eventID) }
    func closeThread(eventID: String) { kernel.closeThread(eventID: eventID) }
    func claimProfile(pubkey: String, consumerID: String) {
        kernel.claimProfile(pubkey: pubkey, consumerID: consumerID)
    }
    func releaseProfile(pubkey: String, consumerID: String) {
        kernel.releaseProfile(pubkey: pubkey, consumerID: consumerID)
    }
    func openFirehose(tag: String) { kernel.openFirehose(tag: tag) }

    // ── T66a command surface (identity / publish / multi-account) ────────
    // Every method is a pass-through to a real kernel dispatch. No Swift-side
    // business logic, no cached state (D5/D8) — every accessor above is a
    // pure read of the kernel snapshot.

    func signInNsec(_ secret: String) {
        kmLog.info("signInNsec dispatched (len=\(secret.count))")
        kernel.signInNsecAndRegisterMarmot(secret)
    }

    func signInBunker(_ uri: String) { kernel.signInBunker(uri) }

    /// Cancel an in-flight NIP-46 handshake. The handshake projection is part
    /// of `snapshot`, so reading `bunkerHandshake` reconciles automatically
    /// when the broker emits `idle` on the next tick.
    func cancelBunkerHandshake() { kernel.cancelBunkerHandshake() }

    func nostrConnectURI() -> String? {
        let relay = relayEditRows.first { row in
            let roles = row.role
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
            return roles.contains("both") || roles.contains("write")
        }?.url ?? "wss://r.f7z.io"
        // Rust composes the `&callback=` suffix from the scheme; Swift never
        // builds the URL itself.
        return kernel.nostrConnectURI(relay: relay, callbackScheme: "chirp://nip46")
    }

    func createAccount(
        profile: [String: String] = ["name": "New User"],
        relays: [(String, String)]? = nil,
        mls: Bool = true
    ) {
        kmLog.info("createAccount dispatched")
        let relayFacts = relays ?? onboardingRelayOverride.map { [($0, "")] } ?? []
        marmotRegistrationRequested = mls
        // PR-L: the bridge defends the JSON encode path instead of trapping
        // with `try!`. A typed-but-impossible encode failure surfaces as a
        // toast and the dispatch is aborted — never a crash.
        if let encodeError = kernel.createAccount(profile: profile, relays: relayFacts, mls: mls) {
            kmLog.error("createAccount encode failed: \(encodeError, privacy: .public)")
            lastDispatchError = encodeError
            lastErrorToast = encodeError
            marmotRegistrationRequested = false
        }
    }

    @discardableResult
    func publishProfile(name: String, about: String, picture: String) -> DispatchResult {
        var profile: [String: String] = ["name": name]
        if !about.isEmpty { profile["about"] = about }
        if !picture.isEmpty { profile["picture"] = picture }
        return track(kernel.publishProfile(profile: profile))
    }

    func switchActive(_ identityID: String) {
        marmotRegistrationRequested = true
        kernel.switchActive(identityID: identityID)
    }

    func removeAccount(_ identityID: String) {
        kernel.removeAccountAndForgetSecret(identityID: identityID)
    }

    @discardableResult
    func publishNote(_ content: String, replyToID: String? = nil) -> DispatchResult {
        track(kernel.publishNote(content: content, replyToID: replyToID))
    }

    func retryPublish(handle: String) { kernel.retryPublish(handle: handle) }
    func cancelPublish(handle: String) { kernel.cancelPublish(handle: handle) }

    @discardableResult
    func react(targetEventID: String, reaction: String = "❤") -> DispatchResult {
        track(kernel.react(targetEventID: targetEventID, reaction: reaction))
    }

    @discardableResult
    func follow(_ pubkey: String) -> DispatchResult {
        track(kernel.follow(pubkey: pubkey))
    }

    @discardableResult
    func unfollow(_ pubkey: String) -> DispatchResult {
        track(kernel.unfollow(pubkey: pubkey))
    }

    /// Fire a write action authored by Rust through the namespace-keyed
    /// dispatch seam. Rust composes both `namespace` and `bodyJson` (aim.md §4.4).
    @discardableResult
    func dispatchProfileAction(_ spec: ProfileDispatchSpec) -> DispatchResult {
        track(kernel.dispatchRawAction(namespace: spec.namespace, bodyJson: spec.bodyJson))
    }

    func isActionPending(_ correlationId: String) -> Bool {
        pendingActions.contains(correlationId)
    }
    func clearDispatchError() { lastDispatchError = nil }

    /// PR-A: routes a `DispatchResult` through the pending / dispatch-error slots.
    @discardableResult
    private func track(_ result: DispatchResult) -> DispatchResult {
        switch result {
        case let .accepted(id):
            pendingActions.insert(id)
        case let .failure(message):
            kmLog.error("dispatch_action rejected: \(message, privacy: .public)")
            lastDispatchError = message
        }
        return result
    }

    func addRelay(url: String, role: String) { kernel.addRelay(url: url, role: role) }
    func removeRelay(url: String) { kernel.removeRelay(url: url) }
    func openTimeline() { kernel.openTimeline() }
    func clearErrorToast() { lastErrorToast = nil }

    // ── NIP-47 wallet commands ────────────────────────────────────────────

    func walletConnect(uri: String) { kernel.walletConnect(uri: uri) }
    func walletDisconnect() { kernel.walletDisconnect() }
    func walletPayInvoice(bolt11: String, amountMsats: UInt64? = nil) {
        kernel.walletPayInvoice(bolt11: bolt11, amountMsats: amountMsats)
    }

    // ── T118 / G3 — scenePhase pass-through ───────────────────────────────

    func lifecycleForeground() { kernel.lifecycleForeground() }
    func lifecycleBackground() { kernel.lifecycleBackground() }

    // ── Snapshot apply ────────────────────────────────────────────────────

    private func apply(result: KernelUpdateResult) {
        let update = result.update
        guard update.rev > rev else { return }

        let applyStart = ContinuousClock.now
        let callbackToApplyMicros = result.callbackReceivedAt.duration(to: applyStart).microseconds

        // Capture pre-assignment values for delta-driven side-effects below.
        let priorActiveAccount = activeAccount
        let priorItems = items
        if update.activeAccount != priorActiveAccount {
            kmLog.info(
                "apply: activeAccount \(priorActiveAccount ?? "nil") → \(update.activeAccount ?? "nil")")
        }

        // Single source-of-truth assignment — every projection accessor
        // reads through this slot. `lastErrorToast` stays distinct because
        // tap-to-dismiss has nowhere else to land.
        snapshot = update
        lastErrorToast = update.lastErrorToast

        // T146 — refresh modular timeline blocks only when the flat items
        // changed. The grouper has already accepted events by the time the
        // snapshot lands (kernel event observer is synchronous).
        if update.items != priorItems {
            let nextTimeline = kernel.chirpSnapshot()
            if nextTimeline != modularTimeline {
                modularTimeline = nextTimeline
            }
        }

        let activeAccountChanged = update.activeAccount != priorActiveAccount
        if marmotRegistrationRequested, activeAccountChanged {
            _ = kernel.registerActiveMarmotIfAvailable()
            marmotRegistrationRequested = false
        }
        marmot.apply(snapshot: kernel.marmotSnapshot(), isRegistered: kernel.isMarmotRegistered)
        // NIP-29 + NIP-17 stores — pushed every tick so their lazy init fires
        // on the first snapshot (registering the read projections in the
        // process). DM inbox forwards the active pubkey so the kind:1059
        // gift-wrap interest is pushed once a user is signed in.
        groupChat.apply(snapshot: update.groupChat)
        dmInbox.apply(snapshot: update.dmInbox, activePubkey: update.activeAccount)
        // NIP-02 follow list projection mirror. Push every tick so the store
        // tracks `projections["chirp.follow_list"]`. Touching `followList`
        // here forces the lazy `FollowListStore` init on the first snapshot,
        // which registers the read projection (`nmp_app_chirp_register_follow_list`).
        // The active-account pubkey is forwarded so the store can re-invoke
        // the FFI to update the projection's active-pubkey slot after sign-in.
        followList.apply(snapshot: update.followList, activePubkey: update.activeAccount)

        // NIP-29 group-discovery projection mirror. Push every tick so the
        // store tracks `projections["nip29.discovered_groups"]`. The store
        // is unwired until the user enters a relay and taps Search
        // (`searchGroups`); the snapshot key is `nil` until then, and the
        // store ignores stale snapshots from a previously-registered
        // relay during a switch.
        discoveredGroups.apply(snapshot: update.discoveredGroups)

        // NIP-17 § 2 — auto-publish kind:10050 when relay set / account changes.
        if activeAccountChanged { lastPublishedDmRelaySet = nil }
        maybePublishDmRelayList()

        // PR-A: drain `pendingActions` by every terminal verdict on this tick.
        if let results = update.actionResults, !results.isEmpty {
            for terminal in results {
                pendingActions.remove(terminal.correlationId)
            }
        }

        let logicalInterestSummary = logicalInterests
            .map { "\($0.key)=\($0.state)[\($0.cacheCoverage)]" }
            .joined(separator: " | ")
        if !logicalInterestSummary.isEmpty, logicalInterestSummary != lastLogicalInterestSummary {
            lastLogicalInterestSummary = logicalInterestSummary
            diagLog.debug(
                "NMP_DIAG logical_interests rev=\(update.rev, privacy: .public) \(logicalInterestSummary, privacy: .public)")
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
        let lastEventToEmit = update.metrics.lastEventToEmitMs.map(String.init) ?? "none"
        diagLog.debug(
            "NMP_PERF swift_apply rev=\(update.rev, privacy: .public) total_events=\(update.metrics.eventsRx, privacy: .public) batch_events=\(update.metrics.eventsSinceLastUpdate, privacy: .public) inserted=\(insertedCount, privacy: .public) updated=\(updatedCount, privacy: .public) removed=\(removedCount, privacy: .public) visible=\(update.metrics.visibleItems, privacy: .public) payload_bytes=\(result.payloadBytes, privacy: .public) rust_event_to_emit_ms=\(lastEventToEmit, privacy: .public) decode_us=\(result.decodeMicros, privacy: .public) callback_to_apply_us=\(callbackToApplyMicros, privacy: .public) apply_us=\(applyMicros, privacy: .public) callback_to_applied_us=\(callbackToAppliedMicros, privacy: .public)"
        )

        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }

    // ── NIP-17 kind:10050 DM-relay list publish ──────────────────────────

    /// Snapshot-driven kind:10050 publish: fires on relay-set / account
    /// change. No-ops when there's no active account or no read-eligible
    /// relays (the Rust action rejects empty input — kind:10050 with zero
    /// `relay` tags would clear the cache on every peer).
    private func maybePublishDmRelayList() {
        guard let account = activeAccount, !account.isEmpty else { return }
        let readUrls = Self.readEligibleRelayUrls(rows: relayEditRows)
        guard !readUrls.isEmpty else { return }
        let readSet = Set(readUrls)
        if let last = lastPublishedDmRelaySet,
           last.account == account,
           last.urls == readSet
        {
            return
        }
        lastPublishedDmRelaySet = (account, readSet)
        kernel.publishDmRelayList(relays: readUrls)
    }

    /// Return the URLs of relays whose `role` includes `read` or `both`.
    /// Mirrors `nmp_core::actor::relay_roles::has_role` — tokens split on
    /// `,`, `+`, or whitespace, lowercased, and `both` implies both `read`
    /// and `write`.
    nonisolated static func readEligibleRelayUrls(rows: [RelayEditRow]) -> [String] {
        rows.compactMap { row in
            let tokens = row.role
                .lowercased()
                .split(whereSeparator: { $0 == "," || $0 == "+" || $0.isWhitespace })
                .map(String.init)
            return tokens.contains("read") || tokens.contains("both") ? row.url : nil
        }
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
