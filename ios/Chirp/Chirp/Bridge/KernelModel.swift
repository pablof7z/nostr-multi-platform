import Foundation
import SwiftUI
import os.log

private let kmLog = Logger(subsystem: "io.f7z.chirp", category: "KernelModel")

/// `@Observable` mirror of the kernel snapshot. The Rust actor pushes JSON
/// updates via the callback; this class decodes them and republishes for
/// SwiftUI consumption.
@MainActor
final class KernelModel: ObservableObject {
    @Published private(set) var isRunning = false
    @Published private(set) var rev: UInt64 = 0
    @Published private(set) var profile: ProfileCard?
    @Published private(set) var authorView: AuthorProfileSnapshot?
    @Published private(set) var items: [TimelineItem] = []
    /// T146 — modular timeline blocks produced by `nmp-app-chirp`'s
    /// `Nip10ModularTimelineView` projection. Refreshed on every kernel
    /// snapshot via `kernel.chirpSnapshot()`. Coexists with `items` for the
    /// PR: `HomeFeedView` switches to blocks; `ProfileView` /
    /// `ThreadScreen` still consume the original flat list (M2 follow-up
    /// migrates them).
    @Published private(set) var modularTimeline: ChirpTimelineSnapshot = .empty
    @Published private(set) var metrics: KernelMetrics?
    @Published private(set) var relayStatuses: [RelayStatus] = []
    @Published private(set) var snapshotCount: UInt64 = 0
    @Published private(set) var lastSnapshotAt: Date?
    // T66a projections.
    @Published private(set) var accounts: [AccountSummary] = []
    @Published private(set) var activeAccount: String?
    @Published private(set) var publishQueue: [PublishQueueEntry] = []
    @Published private(set) var publishOutbox: [PublishOutboxItem] = []
    /// Pre-formatted outbox header (title + subtitle + per-status counters)
    /// from `projections["outbox_summary"]`. Doctrine §6 anti-pattern #1 —
    /// the shell binds these strings directly instead of `.filter`-counting
    /// `publishOutbox` to derive them. Empty-state fallback for legacy
    /// kernels (D1).
    @Published private(set) var outboxSummary: OutboxSummary = .empty
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

    /// PR-A: correlation ids of dispatched actions whose terminal verdict has
    /// not yet arrived in `projections["action_results"]`. Add on accept,
    /// remove when the snapshot's `actionResults` tick carries the same id.
    /// Views key per-button spinners on `isActionPending(_:)`. Failed
    /// dispatches (the `{"error":...}` envelope) never enter the set — they
    /// are surfaced through `lastDispatchError` instead.
    @Published private(set) var pendingActions: Set<String> = []

    /// PR-A: most recent synchronous dispatch error (the `{"error":"..."}`
    /// envelope from `nmp_app_dispatch_action`). Views consume this as a
    /// toast; clear via `clearDispatchError()`. Distinct from
    /// `lastErrorToast` (which mirrors the kernel snapshot's actor-level
    /// toast and is overwritten every tick) so a dispatch-time rejection is
    /// not silently wiped by the next snapshot emit.
    @Published private(set) var lastDispatchError: String?

    var hasActiveAccount: Bool { activeAccount != nil }

    private let kernel = KernelHandle()
    private var lastLogicalInterestSummary = ""
    private var marmotRegistrationRequested = false
    /// Last `(activeAccount, read-eligible-relay-set)` pair that was published
    /// as a kind:10050 NIP-17 DM-relay list. Tracking the pair prevents the
    /// snapshot-driven auto-dispatch in `apply` from re-publishing on every
    /// tick (the pair only differs when the user actually edits relays or
    /// switches account). `nil` until the first publish fires.
    ///
    /// The Rust action rejects empty input outright, so an empty
    /// `readEligibleSet` is never published — the cache is left in its
    /// previous state when the user clears all read-role relays.
    private var lastPublishedDmRelaySet: (account: String, urls: Set<String>)?

    /// Marmot (MLS encrypted groups) projection mirror. Rust owns identity
    /// restore/persist/register policy; this store only mirrors pushed
    /// snapshots and dispatches user intents.
    private(set) lazy var marmot = MarmotStore(kernel: kernel)

    /// NIP-29 group-chat projection mirror — the first real consumer of the
    /// NIP-29 seam. The store registers its read projection
    /// (`nmp_app_chirp_register_group_chat`) in its initializer; that
    /// initializer runs on the first snapshot tick because `apply` below
    /// touches `groupChat` every tick. The group is the single demo room —
    /// multi-group support needs a handle-returning FFI variant (see
    /// `GroupChatBridge.swift`).
    private(set) lazy var groupChat = GroupChatStore(
        groupId: Self.demoGroupId, kernel: kernel)

    /// NIP-17 private direct-message inbox mirror — the first consumer of the
    /// NIP-17 receive seam. The store registers its read projection
    /// (`nmp_app_chirp_register_dm_inbox`) in its initializer; that
    /// initializer runs on the first snapshot tick because `apply` below
    /// touches `dmInbox` every tick. The store re-invokes the FFI once the
    /// active account is known so the kind:1059 gift-wrap interest is pushed.
    private(set) lazy var dmInbox = DmInboxStore(kernel: kernel)

    /// The NIP-29 group the group-chat screen reads and posts to. A single
    /// fixed room for the first-consumer proof; a real multi-group app
    /// would thread a chosen `GroupId` through navigation.
    static let demoGroupId = GroupId(
        hostRelayUrl: "wss://relay.groups.nip29.com",
        localId: "chirp-demo")

    /// Platform capability implementations injected for the kernel to use.
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
        // Register the keychain capability handler before start() so the kernel
        // can route capability requests to the iOS Keychain from the first tick.
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

    func start() {
        guard !isRunning else { return }
        capabilities.start()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        isRunning = true
        kernel.restoreChirpIdentity(testNsec: ProcessInfo.processInfo.environment["NMP_TEST_NSEC"])
    }

    func stop() {
        kernel.stop()
        capabilities.stop()
        isRunning = false
    }

    func resetAndRestart() {
        kernel.reset()
        items = []
        // T146 — `ActorCommand::Reset` preserves the kernel's observer
        // slot so existing registrations stay alive, BUT the projection's
        // internal state (the grouper + per-event card map) lives behind
        // the same `Arc<ChirpModularTimeline>` as before the reset and
        // would otherwise retain the prior session's blocks. Drop and
        // re-register so the new handle's grouper starts empty; the next
        // batch of events repopulates it.
        kernel.reregisterChirpProjection()
        modularTimeline = .empty
        authorView = nil
        threadView = nil
        publishOutbox = []
        outboxSummary = .empty
        metrics = nil
        rev = 0
        relayStatuses = []
        logicalInterests = []
        wireSubscriptions = []
        logs = []
        appMetrics = AppRuntimeMetrics()
        lastLogicalInterestSummary = ""
        // PR-A: a kernel reset abandons every in-flight action — drop the
        // pending set so stale spinners never persist across a session reset.
        pendingActions = []
        lastDispatchError = nil
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
        kernel.signInNsecAndRegisterMarmot(secret)
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

    func nostrConnectURI() -> String? {
        let relay = relayEditRows.first { row in
            let roles = row.role
                .split(separator: ",")
                .map { $0.trimmingCharacters(in: .whitespaces).lowercased() }
            return roles.contains("both") || roles.contains("write")
        }?.url
            ?? "wss://r.f7z.io"
        return kernel.nostrConnectURI(relay: relay)
    }
    func createAccount(
        profile: [String: String] = ["name": "New User"],
        relays: [(String, String)]? = nil,
        mls: Bool = true
    ) {
        kmLog.info("createAccount dispatched")
        let relayFacts = relays ?? onboardingRelayOverride.map { [($0, "")] } ?? []
        marmotRegistrationRequested = mls
        kernel.createAccount(profile: profile, relays: relayFacts, mls: mls)
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

    /// PR-A: returns true while `correlationId` is still in the pending set —
    /// the dispatch was accepted but no terminal verdict has arrived in
    /// `projections["action_results"]` yet. Views key spinners on this.
    func isActionPending(_ correlationId: String) -> Bool {
        pendingActions.contains(correlationId)
    }

    /// PR-A: clear the synchronous dispatch error toast after the user has
    /// seen it. Distinct from `clearErrorToast()` which clears the
    /// snapshot-driven actor toast.
    func clearDispatchError() { lastDispatchError = nil }

    /// PR-A: route a `DispatchResult` from `KernelHandle` through the
    /// `pendingActions` / `lastDispatchError` state machine. On accept the
    /// correlation_id enters the pending set; on failure the message becomes
    /// the dispatch-error toast and nothing enters the set (no spinner can
    /// hang). The result is returned verbatim so call sites can also read it
    /// (e.g. a publish-button view that wants to flash a local "queued"
    /// indicator on accept).
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
        // D0: profile now reads from projections via computed accessor.
        // Guarded so a legacy kernel that elides the projection keeps the
        // prior card (D1) — the current kernel always emits it.
        if let p = update.profile { profile = p }
        authorView = update.authorView
        let timelineItemsChanged = update.items != items
        if timelineItemsChanged {
            items = update.items
        }
        // T146 — refresh the modular timeline snapshot in the same apply
        // pass. The grouper's state is fed by the kernel event observer
        // (which fires synchronously inside `EventStore::insert`), so by
        // the time the actor pushes its snapshot here the projection's
        // blocks have already accepted every event in `items`. One JSON
        // round-trip per snapshot is the cost; reads are O(blocks + cards)
        // and avoid duplicating profile state (Swift looks the author up
        // in `items` for display name / avatar).
        if timelineItemsChanged {
            let nextTimeline = kernel.chirpSnapshot()
            if nextTimeline != modularTimeline {
                modularTimeline = nextTimeline
            }
        }
        if marmotRegistrationRequested, update.activeAccount != activeAccount {
            _ = kernel.registerActiveMarmotIfAvailable()
            marmotRegistrationRequested = false
        }
        marmot.apply(snapshot: kernel.marmotSnapshot(), isRegistered: kernel.isMarmotRegistered)
        // NIP-29 group-chat projection mirror. Push every tick so the store
        // tracks `projections["nip29.group_chat"]`. Touching `groupChat`
        // here forces the lazy `GroupChatStore` init on the first snapshot,
        // which registers the read projection (`nmp_app_chirp_register_group_chat`)
        // — eager, before the screen opens. A registered observer with no UI
        // simply accumulates messages; opening `GroupChatView` then shows them.
        groupChat.apply(snapshot: update.groupChat)
        // NIP-17 DM inbox projection mirror. Push every tick so the store
        // tracks `projections["nip17.dm_inbox"]`. Touching `dmInbox` here
        // forces the lazy `DmInboxStore` init on the first snapshot, which
        // registers the read projection (`nmp_app_chirp_register_dm_inbox`).
        // The active-account pubkey is forwarded so the store can re-invoke
        // the FFI to push the kind:1059 gift-wrap interest once a user is
        // signed in — without that interest the inbox is wired but inert.
        dmInbox.apply(snapshot: update.dmInbox, activePubkey: update.activeAccount)
        metrics = update.metrics
        relayStatuses = update.relayStatuses
        // T66a projections — mirror only; never derive (D8).
        if let a = update.accounts { accounts = a }
        let activeAccountChanged = update.activeAccount != activeAccount
        activeAccount = update.activeAccount
        if let q = update.publishQueue { publishQueue = q }
        if let outbox = update.publishOutbox { publishOutbox = outbox }
        // §6 anti-pattern #1: pre-formatted outbox header strings from Rust.
        // Fall back to the empty-state literal so a kernel build that predates
        // the projection still renders a sensible header (D1).
        outboxSummary = update.outboxSummary ?? .empty
        lastErrorToast = update.lastErrorToast
        if let r = update.relayEditRows { relayEditRows = r }
        threadView = update.threadView

        // NIP-17 § 2 — publish the user's kind:10050 DM-relay list whenever
        // the read-eligible relay set changes (or the active account does).
        // Snapshot-driven: catches account creation, identity restore, and
        // settings-screen edits with one piece of code. The Rust action
        // (`nmp.dm.publish_relay_list`) owns build / sign / publish.
        if activeAccountChanged {
            // Drop the cache so the new identity republishes its set even if
            // the URLs happen to match the previous identity's.
            lastPublishedDmRelaySet = nil
        }
        maybePublishDmRelayList()
        walletStatus = update.walletStatus
        bunkerHandshake = update.bunkerHandshake

        // PR-A: drain `pendingActions` by every terminal verdict surfaced on
        // this tick. Direction review #29 prefers the per-tick `actionResults`
        // array over the sticky scalar — when two actions settle in one tick
        // both correlation_ids arrive together, so neither host spinner is
        // stranded. Removing from a `Set` is O(1) per id and idempotent if a
        // terminal for an id we never tracked happens to arrive.
        if let results = update.actionResults, !results.isEmpty {
            for terminal in results {
                pendingActions.remove(terminal.correlationId)
            }
        }
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

    // ── NIP-17 kind:10050 DM-relay list publish ──────────────────────────
    //
    // The Rust ingest cache (`nmp-core::kernel::ingest::dm_relay_list`) tells
    // the NIP-17 send path WHERE to deliver a gift-wrap envelope, but without
    // a symmetric *publish* of our own list every Chirp user is invisible as
    // a DM recipient. The `nmp.dm.publish_relay_list` action closes that gap;
    // this helper is its only iOS caller — fired snapshot-driven from
    // `apply()` so it covers account creation, identity restore, and
    // settings-screen relay edits with a single seam (no per-call-site hook).
    //
    // Thin-shell concession: per NIP-17 § 2, kind:10050 lists the user's
    // *read*-eligible relays (their DM inbox). The kernel's
    // `RelayEditRow.role` field is the only source of role info on the Swift
    // side, so this helper does a minimal role-token filter mirroring the
    // Rust `relay_roles::has_role` semantics. A future cleanup could collapse
    // this kernel-side by exposing a `dm_relay_urls: Vec<String>` projection,
    // letting Swift drop the role parsing entirely.

    /// If the active account's read-eligible relay set has changed since the
    /// last publish, dispatch `nmp.dm.publish_relay_list`. No-ops when:
    ///   * No active account (nothing to sign with).
    ///   * `relayEditRows` produced an empty read-eligible set — the Rust
    ///     action rejects empty input (a kind:10050 with zero `relay` tags
    ///     would clear the cache on every peer).
    ///   * The set matches what was last published for this account.
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
    /// and `write`. URLs are returned in `rows` order; the kernel already
    /// canonicalizes them on insert so no further normalization is needed
    /// before the FFI hand-off.
    ///
    /// `nonisolated` so unit tests can exercise the pure filter without
    /// awaiting the `@MainActor` `KernelModel` isolation (the function
    /// touches no instance state).
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
