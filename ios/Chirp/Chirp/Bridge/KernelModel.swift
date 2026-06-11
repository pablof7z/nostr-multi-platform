import Foundation
import SwiftUI
import os.log
import os.signpost

private let kmLog = Logger(subsystem: "io.f7z.chirp", category: "KernelModel")

/// PR-L (no_print_in_bridge SwiftLint rule): structured replacement for the
/// prior `print("NMP_DIAG …")` / `print("NMP_PERF …")` stdout lines. The
/// dedicated `org.nmp.chirp.diag` subsystem keeps the perf trace filterable
/// without polluting the primary `io.f7z.chirp` stream.
private let diagLog = Logger(subsystem: "org.nmp.chirp.diag", category: "KernelModel")

#if DEBUG
/// Signpost log for reliability instrumentation (B2 empty-after-nonempty
/// fault). Debug-only — never compiled into a shipped build. Filter in
/// Instruments / `log stream` on subsystem `org.nmp.chirp.diag`,
/// category `reliability`.
private let reliabilityLog = OSLog(subsystem: "org.nmp.chirp.diag", category: "reliability")
#endif

/// `ObservableObject` mirror of the kernel snapshot. The Rust actor pushes
/// binary FlatBuffers updates via the callback; the bridge decodes them and
/// this class republishes the resulting model for SwiftUI consumption.
///
/// PR-L (KernelModel collapse): every kernel-driven projection lives behind
/// the single `@Published var snapshot: KernelUpdate?` slot; the computed
/// accessors below restore the per-field view-facing API (`model.profile`,
/// `model.modularTimeline`, …) verbatim. The genuinely-local mutable slots —
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
final class KernelModel: ObservableObject, NostrProfileHost {

    // ── Snapshot slot — single source of truth for kernel-driven state ──

    /// Latest decoded snapshot. `nil` before the first tick lands.
    @Published private(set) var snapshot: KernelUpdate?

    /// ADR-0038 typed home-feed override. Non-nil when the typed NOFS+NFCT
    /// decode succeeded on the most-recent snapshot tick. Preferred over the
    /// generic `snapshot?.homeFeed` in `modularTimeline`. Falls back to nil
    /// on any tick where the typed path returns nil, at which point
    /// `snapshot?.homeFeed` (generic `Value` decode) is used instead.
    @Published private(set) var typedHomeFeed: ChirpTimelineSnapshot?

    /// V6 Stage 4 (Wave B) typed `accounts` override. Non-nil when the typed
    /// `KACC` sidecar decoded on the most-recent tick. Preferred over the
    /// generic `snapshot?.accounts` in the `accounts` accessor
    /// (`KernelModel+Projections`). Falls back to `nil` on any tick where the
    /// typed path returns nil, at which point the generic JSON path is used.
    @Published private(set) var typedAccounts: [AccountSummary]?

    /// V6 Stage 4 (Wave B) typed `active_account` override. Non-nil when the
    /// typed `KACT` sidecar decoded to an active pubkey on the most-recent tick.
    /// Preferred over the generic `snapshot?.activeAccount` in the
    /// `activeAccount` accessor. `nil` (no sidecar OR no active account) defers
    /// to the generic JSON path — parity-preserving.
    @Published private(set) var typedActiveAccount: String?

    /// V6 Stage 4 (Wave B batch #2) typed overrides for the relay-settings +
    /// publish-cluster thin-glue keys. Each is non-nil only on ticks where the
    /// corresponding sidecar (`KCRL` / `KRRO` / `KOXS` / `KPBO` / `KPBQ`)
    /// decoded; `nil` defers to the generic JSON path through the accessor
    /// (`KernelModel+Projections`). Preferred over the matching
    /// `snapshot?.<field>` in the accessor.
    @Published private(set) var typedConfiguredRelays: [AppRelay]?
    @Published private(set) var typedRelayRoleOptions: [RelayRoleOption]?
    @Published private(set) var typedOutboxSummary: OutboxSummary?
    @Published private(set) var typedPublishOutbox: [PublishOutboxItem]?
    @Published private(set) var typedPublishQueue: [PublishQueueEntry]?

    /// V6 Stage 4 (Wave B batch #3) typed overrides for the diagnostics +
    /// action-lifecycle keys. Each is non-nil only on ticks where the
    /// corresponding sidecar (`KRDG` / `KALC`) decoded; `nil` defers to the
    /// generic JSON path through the accessor (`KernelModel+Projections`).
    @Published private(set) var typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    @Published private(set) var typedActionLifecycle: ActionLifecycleSnapshot?

    /// V6 Stage 4 (Wave B Tier-1 #4) typed overrides for the app-projection
    /// keys. Each is non-nil only on ticks where the corresponding sidecar
    /// (`NF02` / `NZAP` / `NGCS` / `NDGS`) decoded; `nil` defers to the generic
    /// JSON path. `typedZaps` is read through the `KernelModel+Projections`
    /// accessor (`zaps`); the other three feed their dedicated stores
    /// (`FollowListStore` / `GroupChatStore` / `DiscoveredGroupsStore`) through
    /// the SAME typed-first effective value in `apply(result:)` so the store and
    /// any accessor never split-brain (typed vs JSON).
    @Published private(set) var typedFollowList: FollowListSnapshot?
    @Published private(set) var typedZaps: ZapsAggregateSnapshot?
    @Published private(set) var typedGroupChat: GroupChatSnapshot?
    @Published private(set) var typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed profile-cluster sidecars (`KPRF` / `KCPR` / `KRPR`). `nil` ⇒ the
    /// `KernelModel+Projections` accessor falls back to the generic JSON
    /// `snapshot?.profile` / `.claimedProfiles` / `.resolvedProfiles`.
    @Published private(set) var typedProfile: ProfileCard?
    @Published private(set) var typedClaimedProfiles: [String: ProfileCard]?
    @Published private(set) var typedResolvedProfiles: [String: ProfileCard]?
    /// Typed NIP-17 DM cluster + claimed-event map sidecars (`NDMI` / `NDRL` /
    /// `KCEV`). `nil` ⇒ the generic JSON path applies. `typedDmInbox` feeds the
    /// `dmInbox` store and `typedClaimedEvents` feeds `EmbedHost` through the
    /// SAME typed-first effective value in `apply(result:)` (no split-brain);
    /// `typedDmRelayList` is read through the `dmRelayList` accessor (no consumer
    /// yet — wired for parity).
    @Published private(set) var typedDmInbox: DmInboxSnapshot?
    @Published private(set) var typedDmRelayList: DmRelayListSnapshot?
    @Published private(set) var typedClaimedEvents: [String: ClaimedEventDto]?

    /// NIP-46 cluster typed sidecars (`bunker_handshake` / `nip46_onboarding`).
    /// `nil` ⇒ the generic JSON path applies, read through the `bunkerHandshake`
    /// / `nip46Onboarding` accessors (`typed<Key> ?? snapshot?.<key>`).
    @Published private(set) var typedBunkerHandshake: BunkerHandshake?
    @Published private(set) var typedNip46Onboarding: Nip46Onboarding?

    /// Typed `wallet` (`NWST`) + `settings_hub` (`KSHB`) sidecars. Each is
    /// non-nil only on ticks where the corresponding sidecar decoded; `nil` ⇒
    /// the generic JSON path applies, read through the `walletStatus` /
    /// `settingsHub` accessors (`KernelModel+Projections`). `typedWallet` emits
    /// no sidecar while the wallet is disconnected, so nil is the steady state.
    @Published private(set) var typedWallet: WalletStatusData?
    @Published private(set) var typedSettingsHub: [String: Int]?

    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs`). Non-nil when the frame carried the typed envelope (gated on
    /// `metrics`); `nil` ⇒ the generic JSON `payload` top-level scalars apply.
    /// Read through the `KernelModel+Projections` envelope accessors
    /// (`isRunning` / `rev` / `metrics` / `relayStatuses` / `logicalInterests` /
    /// `wireSubscriptions` / `logs`), each `typedEnvelope?.<field> ??
    /// snapshot?.<field>`. This is the LAST consumer of `payload`'s top-level
    /// scalars.
    @Published private(set) var typedEnvelope: TypedSnapshotEnvelope?

    /// Dynamic flat feeds opened per profile/thread screen. Keys are
    /// `nmp.feed.author.<pubkey>` and `nmp.feed.thread.<event_id>`.
    @Published private(set) var flatFeeds: [String: ChirpTimelineSnapshot] = [:]

    // ── Local mutable state ──────────────────────────────────────────────

    @Published private(set) var snapshotCount: UInt64 = 0
    @Published private(set) var lastSnapshotAt: Date?
    @Published private(set) var appMetrics = AppRuntimeMetrics()
    /// Snapshot-derived AND user-clearable, so we cannot fold this into the
    /// `snapshot` accessor — the clear gesture has nowhere else to land.
    @Published private(set) var lastErrorToast: String?
    /// Success toast — set by Swift (not the Rust snapshot) when an async
    /// action settles with `Accepted`. Cleared by the overlay's `.task` TTL,
    /// same lifecycle as `lastErrorToast`.
    @Published private(set) var lastSuccessToast: String?
    /// Synchronous dispatch-error toast slot, distinct from the
    /// snapshot-driven `lastErrorToast`. Carries the human-readable reason
    /// returned by `dispatch_action` when it rejects a request synchronously
    /// (malformed body, unknown namespace, registry not initialised). NOT
    /// an action-lifecycle signal — a lifecycle failure surfaces through
    /// `actionLifecycle.recentTerminal[.failed(reason)]` from the projection.
    @Published private(set) var lastDispatchError: String?
    @Published var visibleLimit: UInt32 = 80
    @Published var emitHz: UInt32 = 4
    #if DEBUG
    private var debugPubkeysWithResolvedProfileNames: Set<String> = []
    private var debugPubkeysMissingAfterResolvedProfileName: Set<String> = []
    #endif

    /// Embed host — updated on every snapshot push so EmbeddedEvent views
    /// see resolved envelopes as soon as the kernel delivers them (D8).
    let embedHost = EmbedHost()

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

    // ── Stores & capabilities (non-published) ────────────────────────────

    private let kernel = KernelHandle()
    /// Re-entrance guard for `start()`. The snapshot-driven `isRunning`
    /// accessor only flips after the first tick lands, so a re-entrant
    /// `start()` before then would dispatch the FFI twice.
    private var startedKernel = false
    private var lastLoadMoreCursor: TimelineWindowCursor?
    private var lastLogicalInterestSummary = ""
    private var marmotRegistrationRequested = false

    private(set) lazy var marmot = MarmotStore(kernel: kernel)
    @Published private(set) var groupChat: GroupChatStore
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
    ///
    /// D7/B1: This hardcoded relay is intentional — it's a fixed demo group
    /// identifier for the first-consumer proof, not a bootstrap relay. The
    /// kernel's actual relay defaults flow through the snapshot
    /// (`relayStatuses`, `configuredRelays`) populated by nmp-core.
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
        groupChat = GroupChatStore(groupId: Self.demoGroupId, kernel: kernel)
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
        // Seed Chirp's default relays before start. nmp-core no longer carries
        // a hardcoded relay fallback — the app owns its default relay set. These
        // pre-start `addRelay` calls populate `configured_relays` so the kernel
        // has discovery/content relays on a fresh install; the actor dedups
        // them against any session-restored relay list, so re-seeding existing
        // rows is a no-op. (Mirrors the Rust `NmpAppBuilder` default-relay path.)
        kernel.addRelay(url: "wss://r.f7z.io", role: "both")
        kernel.addRelay(url: "wss://purplepag.es", role: "indexer")
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        // Cold-launch Marmot fallback. The kernel actor owns identity
        // restoration through its `nmp.identity.local_nsec.<pubkey>` slot (see
        // `crates/nmp-core/src/actor/session_persistence.rs`). Pre-arming
        // `marmotRegistrationRequested` lets the existing `apply()` fallback
        // call `registerActiveMarmotIfAvailable()` on the first tick where
        // `activeAccount` flips from nil -> restored pubkey; by then the actor
        // has populated `mls_local_nsec` so the active-key registration path
        // succeeds.
        marmotRegistrationRequested = true
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
        flatFeeds = [:]
        // T146 — Reset preserves the observer slot but the grouper retains
        // the prior session's blocks; re-register so it starts empty.
        kernel.reregisterChirpProjection()
        lastLoadMoreCursor = nil
        appMetrics = AppRuntimeMetrics()
        #if DEBUG
        debugPubkeysWithResolvedProfileNames.removeAll()
        debugPubkeysMissingAfterResolvedProfileName.removeAll()
        #endif
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

    #if DEBUG
    /// Test-only seam: inject a synthetic decoded snapshot directly into the
    /// `snapshot` slot so unit tests can exercise the projection accessors
    /// (`claimedProfiles`, `mentionProfiles`, `profile(forPubkey:)`) on the
    /// real read path — including the `.convertFromSnakeCase` CodingKey
    /// mapping the kernel relies on — without starting the Rust actor.
    /// Never compiled into a shipped build.
    func setSnapshotForTesting(_ update: KernelUpdate) {
        snapshot = update
    }
    #endif

    func loadOlderTimeline(after cursor: TimelineWindowCursor) {
        // When Rust reaches the feed cap, `hasMore` flips false and this
        // returns before the repeated last-row `onAppear` can retry.
        guard modularTimeline.page?.hasMore == true else { return }
        // Swift treats the cursor as an opaque render-edge marker. Rust owns
        // page size, cap, and the next window; this guard only de-dupes
        // repeated `onAppear` calls for the same visible tail row.
        guard lastLoadMoreCursor != cursor else { return }
        lastLoadMoreCursor = cursor
        kernel.loadOlderHomeFeed()
    }

    // ── View/Author/Thread open + close ──────────────────────────────────

    func openAuthor(pubkey: String) { kernel.openAuthor(pubkey: pubkey) }
    func closeAuthor(pubkey: String) { kernel.closeAuthor(pubkey: pubkey) }
    func openThread(eventID: String) { kernel.openThread(eventID: eventID) }
    func closeThread(eventID: String) { kernel.closeThread(eventID: eventID) }
    func authorFeed(pubkey: String) -> ChirpTimelineSnapshot? {
        flatFeeds["nmp.feed.author.\(pubkey)"]
    }
    func threadFeed(eventID: String) -> ChirpTimelineSnapshot? {
        flatFeeds["nmp.feed.thread.\(eventID)"]
    }
    func claimProfile(pubkey: String, consumerID: String) {
        kernel.claimProfile(pubkey: pubkey, consumerID: consumerID)
    }
    func releaseProfile(pubkey: String, consumerID: String) {
        kernel.releaseProfile(pubkey: pubkey, consumerID: consumerID)
    }

    /// NostrProfileHost conformance: look up a profile by pubkey.
    /// First checks claimed profiles, then falls back to mention profiles.
    ///
    /// `ProfileWire.npub` is `nil` on the mention-profiles path because the
    /// mention projection carries no bech32 encoding. Callers that need npub
    /// for copy/share must guard for nil — only the claimedProfiles path
    /// guarantees a non-nil npub.
    func profile(forPubkey pubkey: String) -> ProfileWire? {
        if let card = claimedProfiles[pubkey] {
            #if DEBUG
            if card.displayName?.isEmpty == false {
                markProfileNameResolved(pubkey)
            }
            #endif
            return ProfileWire(
                pubkey: pubkey,
                displayName: (card.displayName?.isEmpty == false) ? card.displayName : nil,
                about: card.about.isEmpty ? nil : card.about,
                pictureUrl: card.pictureUrl,
                nip05: card.nip05.isEmpty ? nil : card.nip05,
                npub: card.npub,
                npubShort: card.npub.count > 12
                    ? "\(card.npub.prefix(9))…\(card.npub.suffix(4))"
                    : card.npub
            )
        }
        if let mention = mentionProfiles[pubkey] {
            let isRawKey = mention.display == pubkey.shortHex
            #if DEBUG
            if !isRawKey && !mention.display.isEmpty {
                markProfileNameResolved(pubkey)
            }
            #endif
            return ProfileWire(
                pubkey: pubkey,
                displayName: isRawKey ? nil : mention.display,
                about: nil,
                pictureUrl: mention.pictureUrl,
                nip05: nil,
                npub: nil,
                npubShort: pubkey.shortHex
            )
        }
        #if DEBUG
        // A2: name-regression instrumentation. Count only the first nil after
        // this pubkey has resolved to a real name, then re-arm once the name is
        // seen again. First-load misses stay invisible to the counter.
        recordProfileNameMissIfRegression(pubkey)
        #endif
        return nil
    }

    #if DEBUG
    private func markProfileNameResolved(_ pubkey: String) {
        debugPubkeysWithResolvedProfileNames.insert(pubkey)
        debugPubkeysMissingAfterResolvedProfileName.remove(pubkey)
    }

    private func recordProfileNameMissIfRegression(_ pubkey: String) {
        guard debugPubkeysWithResolvedProfileNames.contains(pubkey) else { return }
        guard !debugPubkeysMissingAfterResolvedProfileName.contains(pubkey) else { return }
        debugPubkeysMissingAfterResolvedProfileName.insert(pubkey)
        appMetrics.recordNameRegression()
    }
    #endif

    // ── T66a command surface (identity / publish / multi-account) ────────
    // Every method is a pass-through to a real kernel dispatch. No Swift-side
    // business logic, no cached state (D5/D8) — every accessor above is a
    // pure read of the kernel snapshot.

    /// Add a local-key (nsec) signer. Mirrors the Rust `add_signer(source:
    /// SignerSource::LocalNsec, make_active:)` API. The nsec path routes
    /// through the Chirp/Marmot identity FFI so the MLS registration
    /// side-effect is preserved (a bare `add_signer` on `NmpApp` would not
    /// register Marmot). `makeActive` is plumbed for API parity; the current
    /// Chirp identity FFI always activates the imported account.
    func addSigner(localNsec secret: String, makeActive: Bool = true) {
        kmLog.info("addSigner(localNsec) dispatched (len=\(secret.count), makeActive=\(makeActive))")
        kernel.signInNsecAndRegisterMarmot(secret)
    }

    /// Add a NIP-46 remote (bunker) signer. Mirrors the Rust `add_signer(source:
    /// SignerSource::BunkerUri, make_active:)` API. Flows through the signer
    /// broker, which drives the connect handshake and emits
    /// `AddSigner(source: RemoteHandle, make_active:)`.
    func addSigner(bunkerUri uri: String, makeActive: Bool = true) {
        kernel.signInBunker(uri)
    }

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
        return track(kernel.publishProfile(name: name, about: about, picture: picture))
    }

    func switchActive(_ identityID: String) {
        marmotRegistrationRequested = true
        kernel.switchActive(identityID: identityID)
    }

    func removeAccount(_ identityID: String) {
        kernel.removeAccountAndForgetSecret(identityID: identityID)
    }

    @discardableResult
    func publishNote(_ content: String, replyTo: ChirpReplyTarget? = nil) -> DispatchResult {
        track(kernel.publishNote(content: content, replyTo: replyTo))
    }

    func retryPublish(handle: String) { kernel.retryPublish(handle: handle) }
    func cancelPublish(handle: String) { kernel.cancelPublish(handle: handle) }

    @discardableResult
    func react(targetEventID: String, reaction: String = "❤") -> DispatchResult {
        track(kernel.react(targetEventID: targetEventID, reaction: reaction))
    }

    @discardableResult
    func repost(eventID: String, authorPubkey: String) -> DispatchResult {
        track(kernel.repost(eventID: eventID, authorPubkey: authorPubkey))
    }

    func claimVisibleNoteRelations(eventID: String) {
        kernel.claimVisibleNoteRelations(eventID: eventID)
    }

    func releaseVisibleNoteRelations(eventID: String) {
        kernel.releaseVisibleNoteRelations(eventID: eventID)
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
    ///
    /// V-106: `amountMsats` is required — there is no hardcoded default. The
    /// host surfaces `ZapAmountSheet` to let the user pick the amount (preset
    /// or custom), and passes the chosen msats here. This removes the old
    /// "every zap is 21 sats" behaviour.
    ///
    /// V-07: relay selection is kernel policy. We pass an empty `relays`
    /// list; the actor auto-selects from the recipient's kind:10002
    /// (NIP-65) write/both relays via `kernel.author_write_relays`. The
    /// shell never decides where the LN provider should publish the
    /// kind:9735 receipt.
    func zap(
        targetEventID: String,
        authorPubkey: String,
        lnurl: String,
        amountMsats: UInt64,
        comment: String? = nil
    ) -> DispatchResult {
        return track(
            kernel.zap(
                targetEventID: targetEventID,
                authorPubkey: authorPubkey,
                lnurl: lnurl,
                amountMsats: amountMsats,
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

    @discardableResult
    func createPublicGroup(group: GroupId, name: String, about: String?) -> DispatchResult {
        let result = track(kernel.createPublicGroup(group: group, name: name, about: about))
        if case .accepted = result {
            groupChat = GroupChatStore(groupId: group, kernel: kernel)
        }
        return result
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
    func publishRelayList(relays: [AppRelay]) -> DispatchResult {
        track(kernel.publishRelayList(relays: relays))
    }
    func openTimeline() { kernel.openTimeline() }
    func clearErrorToast() { lastErrorToast = nil }
    func showSuccessToast(_ message: String) { lastSuccessToast = message }
    func clearSuccessToast() { lastSuccessToast = nil }

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
        // `priorActiveAccount` reads the OLD effective value through the
        // `activeAccount` accessor (typed-or-JSON of the previous tick).
        let priorActiveAccount = activeAccount
        // V6 Stage 4 (Wave B): the NEW effective active account is the typed
        // sidecar when present, else the generic JSON `update.activeAccount`.
        // Every internal consumer below (delta log, marmot re-registration,
        // follow-list active-pubkey forward) MUST read this same value so the
        // UI accessor and the side-effects never split-brain across the two
        // sources — the parity contract is the safety net, not the design.
        let newActiveAccount = result.typedActiveAccount ?? update.activeAccount
        if newActiveAccount != priorActiveAccount {
            kmLog.info(
                "apply: activeAccount \(priorActiveAccount ?? "nil") → \(newActiveAccount ?? "nil")")
        }

        #if DEBUG
        // B2: capture the rendered timeline-card count BEFORE the
        // snapshot/typedHomeFeed assignments below. `modularTimeline` reads
        // through those slots, so reading it after the assignment would
        // compare a value against itself and the empty-after-nonempty
        // detector would never fire. `cards` is the per-thread-root row set
        // the home feed renders.
        let prevTimelineCount = modularTimeline.cards.count
        #endif

        // Single source-of-truth assignment — every projection accessor
        // reads through this slot. `lastErrorToast` stays distinct because
        // tap-to-dismiss has nowhere else to land.
        snapshot = update
        // Claimed-event map: typed-first effective value
        // (`typedClaimedEvents ?? update.projections?.claimedEvents`). The `KCEV`
        // sidecar wins when present; the generic JSON projection is the fallback.
        // `EmbedHost` rebuilds the embed envelope map from the same effective
        // value either path yields — no split-brain.
        embedHost.update(
            claimedEvents: result.typedClaimedEvents ?? update.projections?.claimedEvents
        )
        // ADR-0038: store the typed home-feed result. Nil means the generic
        // projections.homeFeed fallback applies for this tick.
        typedHomeFeed = result.typedHomeFeed
        // V6 Stage 4 (Wave B): store the typed accounts / active-account decode.
        // Nil means the generic `projections.accounts` / `projections.active_account`
        // JSON fallback applies for this tick (read through the accessors).
        typedAccounts = result.typedAccounts
        typedActiveAccount = result.typedActiveAccount
        // V6 Stage 4 (Wave B batch #2): store the relay-settings + publish-cluster
        // typed decodes. Nil ⇒ the generic JSON projection fallback applies for
        // this tick (read through the `KernelModel+Projections` accessors).
        typedConfiguredRelays = result.typedConfiguredRelays
        typedRelayRoleOptions = result.typedRelayRoleOptions
        typedOutboxSummary = result.typedOutboxSummary
        typedPublishOutbox = result.typedPublishOutbox
        typedPublishQueue = result.typedPublishQueue
        // V6 Stage 4 (Wave B batch #3): store the diagnostics + action-lifecycle
        // typed decodes. Nil ⇒ the generic JSON projection fallback applies for
        // this tick (read through the `KernelModel+Projections` accessors).
        typedRelayDiagnostics = result.typedRelayDiagnostics
        typedActionLifecycle = result.typedActionLifecycle
        // V6 Stage 4 (Wave B Tier-1 #4): store the app-projection typed decodes.
        // Nil ⇒ the generic JSON projection fallback applies for this tick.
        // `typedZaps` is read through the `zaps` accessor; the other three are
        // consumed below via the effective `typed<Key> ?? update.<key>` value
        // fed to their stores (no split-brain).
        typedFollowList = result.typedFollowList
        typedZaps = result.typedZaps
        typedGroupChat = result.typedGroupChat
        typedDiscoveredGroups = result.typedDiscoveredGroups
        typedProfile = result.typedProfile
        typedClaimedProfiles = result.typedClaimedProfiles
        typedResolvedProfiles = result.typedResolvedProfiles
        // NIP-17 DM cluster + claimed-event map. Nil ⇒ the generic JSON
        // projection fallback applies for this tick. `typedDmInbox` is consumed
        // below via the effective `typedDmInbox ?? update.dmInbox` value fed to
        // the `dmInbox` store; `typedClaimedEvents` via the effective map fed to
        // `EmbedHost.update`; `typedDmRelayList` is read through the `dmRelayList`
        // accessor (no consumer yet).
        typedDmInbox = result.typedDmInbox
        typedDmRelayList = result.typedDmRelayList
        typedClaimedEvents = result.typedClaimedEvents
        // NIP-46 cluster: store the typed bunker-handshake / onboarding decodes.
        // Nil ⇒ the generic JSON projection fallback applies for this tick (read
        // through the `bunkerHandshake` / `nip46Onboarding` accessors).
        typedBunkerHandshake = result.typedBunkerHandshake
        typedNip46Onboarding = result.typedNip46Onboarding
        // `wallet` (NWST) + `settings_hub` (KSHB): store the typed decodes. Nil ⇒
        // the generic JSON projection fallback applies for this tick (read through
        // the `walletStatus` / `settingsHub` accessors in `KernelModel+Projections`).
        typedWallet = result.typedWallet
        typedSettingsHub = result.typedSettingsHub
        // ADR-0044 Tier-3: store the typed `SnapshotFrame` envelope decode. Nil
        // ⇒ the generic JSON `payload` top-level scalars apply for this tick
        // (read through the `KernelModel+Projections` envelope accessors). The
        // staleness guard above stays on raw `update.rev` (a guaranteed mirror
        // per ADR-0032, so effective-rev ≡ update.rev) — flipping it would add
        // risk with no behavior change.
        typedEnvelope = result.typedEnvelope
        flatFeeds = result.flatFeeds
        lastErrorToast = update.lastErrorToast

        #if DEBUG
        // B1: track the typed-decode success rate. A nil `typedHomeFeed` means
        // this tick fell back to the generic `projections.homeFeed` decode.
        appMetrics.recordTypedDecode(success: result.typedHomeFeed != nil)

        // B2: empty-after-nonempty detection. If the freshly-applied snapshot
        // emptied a previously-populated timeline, flag a fault signpost so
        // the churn is visible in Instruments and bump the counter for tests.
        if modularTimeline.cards.isEmpty && prevTimelineCount > 0 {
            appMetrics.recordEmptyAfterNonEmpty()
            os_signpost(
                .event, log: reliabilityLog, name: "timeline_empty_after_nonempty",
                "rev=%llu prev_count=%ld", update.rev, prevTimelineCount)
        }
        #endif

        let activeAccountChanged = newActiveAccount != priorActiveAccount
        if marmotRegistrationRequested, activeAccountChanged {
            _ = kernel.registerActiveMarmotIfAvailable()
            marmotRegistrationRequested = false
        }
        // V-107 / ADR-0039: Marmot state now comes from push projections
        // (`nmp.marmot.snapshot` / `nmp.marmot.messages`) on the SnapshotFrame —
        // no more pull calls to `nmp_marmot_snapshot` / `nmp_marmot_group_messages`.
        // `isMarmotRegistered` still reads the handle slot (unchanged — it is NOT
        // a deleted symbol; it just checks whether the handle is non-nil).
        //
        // Typed-first effective values (`typedMarmotSnapshot ?? update.projections?.marmotSnapshot`,
        // `typedMarmotMessages ?? update.projections?.marmotMessages`). The `NMMS`/`NMMG`
        // sidecars win when present; the generic JSON projections are the fallback —
        // the SAME effective value either path yields. `MarmotGroupChatView` /
        // `MarmotGroupsView` read off this same `marmot` store, so routing here keeps
        // every Marmot consumer typed-first with no split-brain. A signed-out tick
        // yields nil from both the typed decode and the JSON path → `apply` maps it
        // to `.empty` / `[:]` (parity preserved).
        marmot.apply(
            snapshot: result.typedMarmotSnapshot ?? update.projections?.marmotSnapshot,
            messages: result.typedMarmotMessages ?? update.projections?.marmotMessages,
            isRegistered: kernel.isMarmotRegistered
        )
        // NIP-29 + NIP-17 stores — pushed every tick so their lazy init fires
        // on the first snapshot (registering the read projections in the
        // process). Rust owns the DM inbox interest lifecycle.
        // V6 Stage 4 (Wave B Tier-1 #4): feed the store the typed-first effective
        // value (`typedGroupChat ?? update.groupChat`). The `NGCS` sidecar wins
        // when present; the generic JSON projection is the fallback — the SAME
        // effective value the accessor would yield, so the store never diverges.
        groupChat.apply(snapshot: result.typedGroupChat ?? update.groupChat)
        // NIP-17 DM cluster: typed-first effective value
        // (`typedDmInbox ?? update.dmInbox`). The `NDMI` sidecar wins when
        // present; the generic JSON projection is the fallback. `DmListView` /
        // `DmThreadView` read off this same `dmInbox` store, so routing here
        // keeps every DM consumer typed-first — no split-brain.
        dmInbox.apply(snapshot: result.typedDmInbox ?? update.dmInbox)
        // NIP-02 follow list projection mirror. Push every tick so the store
        // tracks `projections["nmp.follow_list"]`. Touching `followList`
        // here forces the lazy `FollowListStore` init on the first snapshot,
        // which registers the read projection (`nmp_app_chirp_register_follow_list`).
        // The active-account pubkey is forwarded so the store can re-invoke
        // the FFI to update the projection's active-pubkey slot after sign-in.
        // V6 Stage 4 (Wave B Tier-1 #4): typed-first effective value
        // (`typedFollowList ?? update.followList`). `NF02` wins when present; the
        // generic JSON projection is the fallback. `DmListView` reads
        // `model.followList.follows` off this same store, so routing here keeps
        // the picker typed-first too — no split-brain.
        followList.apply(
            snapshot: result.typedFollowList ?? update.followList,
            activePubkey: newActiveAccount
        )

        // NIP-29 group-discovery projection mirror. Push every tick so the
        // store tracks `projections["nmp.nip29.discovered_groups"]`. The store
        // is unwired until the user enters a relay and taps Search
        // (`searchGroups`); the snapshot key is `nil` until then, and the
        // store ignores stale snapshots from a previously-registered
        // relay during a switch.
        // V6 Stage 4 (Wave B Tier-1 #4): typed-first effective value
        // (`typedDiscoveredGroups ?? update.discoveredGroups`). `NDGS` wins when
        // present; the generic JSON projection is the fallback.
        discoveredGroups.apply(
            snapshot: result.typedDiscoveredGroups ?? update.discoveredGroups
        )

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
        let lastEventToEmit = update.metrics.lastEventToEmitMs.map(String.init) ?? "none"
        diagLog.debug(
            "NMP_PERF swift_apply rev=\(update.rev, privacy: .public) total_events=\(update.metrics.eventsRx, privacy: .public) batch_events=\(update.metrics.eventsSinceLastUpdate, privacy: .public) visible=\(update.metrics.visibleItems, privacy: .public) payload_bytes=\(result.payloadBytes, privacy: .public) rust_event_to_emit_ms=\(lastEventToEmit, privacy: .public) decode_us=\(result.decodeMicros, privacy: .public) callback_to_apply_us=\(callbackToApplyMicros, privacy: .public) apply_us=\(applyMicros, privacy: .public) callback_to_applied_us=\(callbackToAppliedMicros, privacy: .public)"
        )

        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }

}

extension KernelModel: EventClaimSinkProtocol {
    func claim(uri: String, consumerId: String) {
        kernel.claimEvent(uri: uri, consumerID: consumerId)
    }
    func release(uri: String, consumerId: String) {
        kernel.releaseEvent(uri: uri, consumerID: consumerId)
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

    #if DEBUG
    // ── Reliability instrumentation (debug-only) ─────────────────────────
    // These counters exist purely to quantify the profile-name flicker
    // defect and the typed-decode reliability of the snapshot pipeline.
    // They are NOT shipped to users (`#if DEBUG`) and feed no production
    // view — they are read by tests and `os_signpost` diagnostics only.

    /// A2: Name-regression counter — how many times a pubkey that should
    /// resolve to a real name had no claimed/resolved profile on the next
    /// accessor read. First-load misses and repeated reads during the same
    /// outage are excluded. See `KernelModel.profile(forPubkey:)`.
    private(set) var nameRegressionCount: Int = 0

    /// B1: Typed-decode tick counters. `typedHomeFeed` is the ADR-0038
    /// typed NOFS+NFCT decode; a nil result on a tick means the generic
    /// `projections.homeFeed` fallback was used instead.
    private(set) var typedDecodeSuccessCount: UInt64 = 0
    private(set) var typedDecodeFailCount: UInt64 = 0

    var typedDecodeSuccessRate: Double {
        let total = typedDecodeSuccessCount + typedDecodeFailCount
        guard total > 0 else { return 1.0 }
        return Double(typedDecodeSuccessCount) / Double(total)
    }

    /// B2: Empty-after-nonempty counter — the timeline went from a
    /// populated set of items to empty across a single tick, a strong
    /// signal of a projection churn / wipe defect.
    private(set) var emptyAfterNonEmptyCount: Int = 0

    mutating func recordNameRegression() {
        nameRegressionCount += 1
    }

    mutating func recordTypedDecode(success: Bool) {
        if success {
            typedDecodeSuccessCount &+= 1
        } else {
            typedDecodeFailCount &+= 1
        }
    }

    mutating func recordEmptyAfterNonEmpty() {
        emptyAfterNonEmptyCount += 1
    }
    #endif

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
