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
/// `model.items`, …) verbatim. The genuinely-local mutable slots —
/// `lastErrorToast` (clearable by the toast tap), `appMetrics` (timing
/// accumulator), `lastDispatchError` (synchronous FFI rejection slot,
/// distinct from the snapshot-driven `lastErrorToast`) — stay individual
/// `@Published` properties.
///
/// V5 thin-shell: action lifecycle tracking lives entirely in Rust. The
/// `action_lifecycle` projection emits `{in_flight, recent_terminal}` on
/// every relevant tick; the shell reads `model.actionLifecycle` and
/// renders verbatim. The previous `pendingActions` / `pendingTerminalStages`
/// / deferred-ACK reducer in this class — a D10 thin-shell violation —
/// was deleted in favour of that projection.
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
    /// Synchronous dispatch-error toast slot, distinct from the
    /// snapshot-driven `lastErrorToast`. Carries the human-readable reason
    /// returned by `dispatch_action` when it rejects a request synchronously
    /// (malformed body, unknown namespace, registry not initialised). NOT
    /// an action-lifecycle signal — a lifecycle failure surfaces through
    /// `actionLifecycle.recentTerminal[.failed(reason)]` from the projection.
    @Published private(set) var lastDispatchError: String?
    @Published var visibleLimit: UInt32 = 80
    @Published var emitHz: UInt32 = 4

    /// D7 actor-death surface — flips to `true` exactly once when the Rust
    /// supervisor emits an `{"t":"panic",...}` update frame (the actor thread
    /// died inside `catch_unwind`) OR when the foreground-resume probe
    /// (`nmp_app_is_alive`, ADR-0028) reports the actor as not running. The
    /// kernel is terminally dead for this process from that point: every
    /// FFI command is a silent no-op, no further snapshots will arrive, and
    /// the only recovery is a process restart. `RootShell` reads this flag
    /// and overlays the red "Background service stopped — please relaunch"
    /// banner unconditionally on top of every other view.
    ///
    /// Set once, never cleared in-process. A future restart-actor path (if
    /// any) would clear it, but the current disposition is "tell the user
    /// to relaunch" — restart-in-process is unsafe because the kernel's
    /// event store / MLS DB / NIP-77 watermarks are in an unknown state
    /// after a panic.
    @Published private(set) var kernelIsDead: Bool = false

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
    var relayRoleOptions: [RelayRoleOption] { snapshot?.relayRoleOptions ?? [] }
    var settingsHub: SettingsHubSummary { snapshot?.settingsHub ?? .empty }
    var threadView: ThreadView? { snapshot?.threadView }
    var walletStatus: WalletStatusData? { snapshot?.walletStatus }
    var logicalInterests: [LogicalInterestStatus] { snapshot?.logicalInterests ?? [] }
    var wireSubscriptions: [WireSubscriptionStatus] { snapshot?.wireSubscriptions ?? [] }
    var relayDiagnostics: RelayDiagnosticsSnapshot { snapshot?.relayDiagnostics ?? .empty }
    var logs: [String] { snapshot?.logs ?? [] }
    var bunkerHandshake: BunkerHandshake? { snapshot?.bunkerHandshake }
    var nip46Onboarding: Nip46Onboarding? { snapshot?.nip46Onboarding }
    /// V5 thin-shell display projection — Rust-owned action lifecycle.
    /// Carries `{ inFlight, recentTerminal }` arrays the views render
    /// verbatim (spinner per in-flight, success/failure toast per
    /// recent terminal). `nil` in steady state.
    var actionLifecycle: ActionLifecycleSnapshot? { snapshot?.actionLifecycle }

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

    private(set) lazy var marmot = MarmotStore(kernel: kernel)
    private(set) lazy var groupChat = GroupChatStore(groupId: Self.demoGroupId, kernel: kernel)
    /// Rust owns the NIP-17 kind:1059 active-account interest and kind:10050
    /// DM-relay-list publish lifecycle; this store only mirrors snapshots.
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
        kernel.listen({ [weak self] result in
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                MainActor.assumeIsolated { self.apply(result: result) }
            }
        }, onPanic: { [weak self] in
            // D7 actor-death — the C callback runs on the Rust listener
            // thread; bounce onto the main runloop so the @Published flip
            // happens on the actor (@MainActor). The Rust supervisor only
            // emits the panic frame once, but `markKernelDead` is idempotent
            // (a stuck-at-true latch) so a stray re-invoke is safe.
            DispatchQueue.main.async { [weak self] in
                guard let self else { return }
                MainActor.assumeIsolated { self.markKernelDead() }
            }
        })
        // Register the keychain capability handler before start() so the
        // kernel can route capability requests from the first tick.
        kernel.registerCapabilityHandler(capabilities)
    }

    /// Set the actor-death flag. Idempotent: a second call is a no-op so the
    /// foreground-resume probe and the push-side panic frame cannot
    /// double-flip (or flicker on / off, which would be worse — the banner
    /// must stay up once raised).
    private func markKernelDead() {
        if kernelIsDead { return }
        kmLog.fault("kernelIsDead set — actor thread terminated")
        kernelIsDead = true
    }

    /// Probe the actor liveness through the FFI (`nmp_app_is_alive`,
    /// ADR-0028) and flip `kernelIsDead` if the actor is gone. Pulled by the
    /// `ChirpApp` scenePhase observer on every `.active` transition: if the
    /// app was backgrounded across an actor panic, the Swift listener thread
    /// may have already exited (the channel closed) and the push-side panic
    /// frame is unreachable. The probe lets the host learn the same fact
    /// on resume so the red banner still shows.
    func checkAlive() {
        // If we already know the kernel is dead, the FFI call is unnecessary
        // (and the `nmp_app_is_alive` symbol on a freshly-`nmp_app_free`'d
        // pointer would be UB — though the current `KernelHandle` keeps the
        // pointer alive for its lifetime, so this is belt + braces).
        if kernelIsDead { return }
        if !kernel.isAlive() {
            markKernelDead()
        }
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
        // V5 thin-shell: action lifecycle state lives in Rust and resets
        // with the kernel `reset()` above — no Swift-side mirror to clear.
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
        // Chirp registers `chirp://` as a custom URL scheme (Info.plist
        // `CFBundleURLTypes`); the signer app deep-links back to
        // `chirp://nip46?...` on approval (handled in `ChirpApp.onOpenURL`).
        // Rust chooses the relay and composes the protocol URL; Swift only
        // supplies the platform callback route.
        return kernel.nostrConnectURI(callbackScheme: "chirp://nip46")
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

    /// Dispatch a NIP-57 zap through the `nmp.nip57.zap` ActionModule.
    /// The recipient's `lnurl` is sourced from `TimelineItem.authorLnurl`
    /// (pre-extracted from kind:0 by Rust — the shell never parses metadata).
    /// `amountMsats` defaults to 21,000 msats (21 sats) until an amount
    /// picker lands.
    ///
    /// V-07: relay selection is kernel policy. We pass an empty `relays`
    /// list; the actor auto-selects from the recipient's kind:10002
    /// (NIP-65) write/both relays via `kernel.author_write_relays`. The
    /// shell never decides where the LN provider should publish the
    /// kind:9735 receipt.
    @discardableResult
    func zap(
        targetEventID: String,
        authorPubkey: String,
        lnurl: String,
        amountMsats: UInt64 = 21_000,
        comment: String? = nil
    ) -> DispatchResult {
        return track(
            kernel.zap(
                targetEventID: targetEventID,
                authorPubkey: authorPubkey,
                lnurl: lnurl,
                amountMsats: amountMsats,
                relays: [],
                comment: comment
            )
        )
    }

    /// Fire a write action authored by Rust through the namespace-keyed
    /// dispatch seam. Rust composes both `namespace` and `bodyJson` (aim.md §4.4).
    @discardableResult
    func dispatchProfileAction(_ spec: ProfileDispatchSpec) -> DispatchResult {
        track(kernel.dispatchRawAction(namespace: spec.namespace, bodyJson: spec.bodyJson))
    }

    /// V5 thin-shell: read the kernel's `action_lifecycle` projection for
    /// a given correlation_id's terminal entry. Returns `nil` when the
    /// kernel has no terminal recorded (either still in flight or
    /// dropped past the TTL window). The kernel handles all the
    /// retention bookkeeping — Swift does NOT track pending sets, NOT
    /// cache terminal stages locally, NOT acknowledge anything.
    func recentTerminal(correlationId: String) -> ActionLifecycleEntry? {
        actionLifecycle?.recentTerminal.first { $0.correlationId == correlationId }
    }

    /// V5 thin-shell: read the kernel's `action_lifecycle` projection for
    /// a given correlation_id's in-flight entry. Returns `nil` when the
    /// action either has not been dispatched, has already settled, or
    /// the kernel has not yet recorded its first stage.
    func inFlight(correlationId: String) -> ActionLifecycleEntry? {
        actionLifecycle?.inFlight.first { $0.correlationId == correlationId }
    }

    func clearDispatchError() { lastDispatchError = nil }

    /// V5 thin-shell: route a `DispatchResult` only through the
    /// synchronous-rejection slot. Successful dispatches surface entirely
    /// through the Rust-owned `action_lifecycle` projection — there is no
    /// Swift-side pending-actions set to populate.
    @discardableResult
    private func track(_ result: DispatchResult) -> DispatchResult {
        if case let .failure(message) = result {
            kmLog.error("dispatch_action rejected: \(message, privacy: .public)")
            lastDispatchError = message
        }
        return result
    }

    func addRelay(url: String, role: String) { kernel.addRelay(url: url, role: role) }
    func removeRelay(url: String) { kernel.removeRelay(url: url) }
    @discardableResult
    func publishDmRelayList(relays: [String]) -> DispatchResult {
        track(kernel.publishDmRelayList(relays: relays))
    }
    @discardableResult
    func publishRelayList(relays: [RelayEditRow]) -> DispatchResult {
        track(kernel.publishRelayList(relays: relays))
    }
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
        // process). Rust owns the DM inbox interest lifecycle.
        groupChat.apply(snapshot: update.groupChat)
        dmInbox.apply(snapshot: update.dmInbox)
        // NIP-02 follow list projection mirror. Push every tick so the store
        // tracks `projections["nmp.follow_list"]`. Touching `followList`
        // here forces the lazy `FollowListStore` init on the first snapshot,
        // which registers the read projection (`nmp_app_chirp_register_follow_list`).
        // The active-account pubkey is forwarded so the store can re-invoke
        // the FFI to update the projection's active-pubkey slot after sign-in.
        followList.apply(snapshot: update.followList, activePubkey: update.activeAccount)

        // NIP-29 group-discovery projection mirror. Push every tick so the
        // store tracks `projections["nmp.nip29.discovered_groups"]`. The store
        // is unwired until the user enters a relay and taps Search
        // (`searchGroups`); the snapshot key is `nil` until then, and the
        // store ignores stale snapshots from a previously-registered
        // relay during a switch.
        discoveredGroups.apply(snapshot: update.discoveredGroups)

        // V5 thin-shell: action lifecycle tracking is fully Rust-owned.
        // The kernel emits `projections["action_lifecycle"]` with `inFlight`
        // and `recentTerminal` arrays already collapsed and TTL-pruned —
        // views read `model.actionLifecycle` and render verbatim. The
        // previous PR-A/PR-G/PR-G2 reducer (pendingActions / pendingTerminalStages
        // / deferred ackActionStage) was a D10 thin-shell violation and is
        // gone. `action_stages` still rides the snapshot for legacy
        // consumers; new code reads only `action_lifecycle`.

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
