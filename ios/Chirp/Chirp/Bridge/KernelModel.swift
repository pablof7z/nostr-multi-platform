import Combine
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
/// Every kernel-driven projection lives behind a dedicated typed slot
/// (`typedHomeFeed`, `typedEnvelope`, …) assigned in `apply(result:)`; the
/// computed accessors in `KernelModel+Projections` expose the per-field
/// view-facing API by reading those slots directly. Profiles are the exception
/// (ADR-0063 Lane E, #1671): they flow per-key through `keyedRefCache`
/// (`profileCard(forPubkey:)`), never a whole-map `@Published` slot. Chirp no
/// longer decodes the generic `payload:Value` whole-payload tree — the typed
/// sidecars + the Tier-3 `SnapshotFrame` envelope are authoritative. The
/// genuinely-local mutable slots — `lastErrorToast` (clearable by the toast
/// tap), `appMetrics` (timing accumulator), `lastDispatchError` (synchronous
/// FFI rejection slot, distinct from the envelope-driven `lastErrorToast`) —
/// stay individual `@Published` properties.
///
/// V5 thin-shell: action lifecycle tracking lives entirely in Rust. The
/// `action_lifecycle` projection emits `{in_flight, recent_terminal}` on
/// every relevant tick; the shell reads `model.actionLifecycle` and
/// renders verbatim. The previous `pendingActions` / `pendingTerminalStages`
/// / deferred-ACK reducer in this class — a D10 thin-shell violation —
/// was deleted in favour of that projection.
@MainActor
final class KernelModel: ObservableObject, NostrProfileHost {

    // ── Typed projection slots — single source of truth for kernel-driven state ──
    //
    // The generic `payload:Value` whole-payload tree is no longer decoded by
    // Chirp. Every kernel-driven projection now lands in a dedicated typed
    // slot below (assigned in `apply(result:)`), and the per-field accessors in
    // `KernelModel+Projections` read those slots directly — no JSON fallback.

    /// ADR-0038 typed home-feed. Non-nil when the typed NOFS+NFCT decode
    /// succeeded on the most-recent tick; `nil` ⇒ the `modularTimeline`
    /// accessor collapses to `.empty`.
    @Published private(set) var typedHomeFeed: OpFeedSnapshot?

    /// V6 Stage 4 (Wave B) typed `accounts` (`KACC` sidecar). `nil` ⇒ the
    /// `accounts` accessor collapses to `[]`.
    @Published private(set) var typedAccounts: [AccountSummary]?

    /// V6 Stage 4 (Wave B) typed `active_account` (`KACT` sidecar). `nil` ⇒ no
    /// active account (the `activeAccount` accessor returns `nil`).
    @Published private(set) var typedActiveAccount: String?

    /// V6 Stage 4 (Wave B batch #2) typed slots for the relay-settings +
    /// publish-cluster keys (`KCRL` / `KRRO` / `KOXS` / `KPBO` / `KPBQ`). `nil`
    /// ⇒ the matching accessor collapses to its empty default.
    @Published private(set) var typedConfiguredRelays: [AppRelay]?
    @Published private(set) var typedRelayRoleOptions: [RelayRoleOption]?
    @Published private(set) var typedOutboxSummary: OutboxSummary?
    @Published private(set) var typedPublishOutbox: [PublishOutboxItem]?
    @Published private(set) var typedPublishQueue: [PublishQueueEntry]?

    /// V6 Stage 4 (Wave B batch #3) typed slots for the diagnostics +
    /// action-lifecycle keys (`KRDG` / `KALC`). `nil` ⇒ the accessor collapses
    /// to its empty default.
    @Published private(set) var typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    @Published private(set) var typedActionLifecycle: ActionLifecycleSnapshot?

    /// V6 Stage 4 (Wave B Tier-1 #4) typed slots for the app-projection keys
    /// (`NF02` / `NZAP` / `NGCS` / `NDGS`). `typedZaps` is read through the
    /// `zaps` accessor; the other three feed their dedicated stores
    /// (`FollowListStore` / `GroupChatStore` / `DiscoveredGroupsStore`) from the
    /// SAME typed value in `apply(result:)`, so store and accessor never diverge.
    @Published private(set) var typedFollowList: FollowListSnapshot?
    @Published private(set) var typedZaps: ZapsAggregateSnapshot?
    @Published private(set) var typedGroupChat: GroupChatSnapshot?
    @Published private(set) var typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed `nmp.nip29.group_defaults` sidecar (`NGDF`, #626) — the crate-owned
    /// suggested public-group relay URL. `nil` ⇒ the `groupDefaults` accessor
    /// collapses to `.empty` and `NewGroupSheet` seeds an empty relay field.
    @Published private(set) var typedGroupDefaults: GroupDefaultsSnapshot?
    // ADR-0063 Lane E (#1671): the profile-cluster `@Published` slots
    // (`typedProfile` / `typedClaimedProfiles` / `typedResolvedProfiles`) are
    // REMOVED — a whole-map broadcast every tick re-rendered the whole view
    // tree on a single kind:0. Profiles flow only through `keyedRefCache`
    // (`refs.profile`), read per-key via `profileCard(forPubkey:)` (D4).
    #if DEBUG
    /// Test-only per-key profile seed `profileCard(forPubkey:)` reads when the
    /// kernel actor is not running (live path is `keyedRefCache`).
    private var debugProfileCardOverrides: [String: ProfileCard] = [:]
    #endif
    /// Typed NIP-17 DM cluster + claimed-event map sidecars (`NDMI` / `NDRL` /
    /// `KCEV`). `typedDmInbox` feeds the `dmInbox` store and `typedClaimedEvents`
    /// feeds `EmbedHost` from the SAME typed value in `apply(result:)`;
    /// `typedDmRelayList` is read through the `dmRelayList` accessor (no consumer
    /// yet — wired for parity).
    /// Issue #1283 Phase 1: the embed resolver moved to Rust. `typedClaimedEventEmbeds`
    /// carries the kernel-resolved `claimed_event_embeds` (`NEMB`) map that feeds
    /// `EmbedHost`; `typedClaimedEvents` (`KCEV`) is retained as a separate live
    /// projection but is no longer the embed-resolution input.
    @Published private(set) var typedDmInbox: DmInboxSnapshot?
    @Published private(set) var typedDmRelayList: DmRelayListSnapshot?
    @Published private(set) var typedClaimedEvents: [String: ClaimedEventDto]?
    @Published private(set) var typedClaimedEventEmbeds: [String: EmbeddedEventEnvelope]?

    /// NIP-46 cluster typed sidecars (`bunker_handshake` / `nip46_onboarding`).
    /// `nil` ⇒ the `bunkerHandshake` / `nip46Onboarding` accessors return nil.
    @Published private(set) var typedBunkerHandshake: BunkerHandshake?
    @Published private(set) var typedNip46Onboarding: Nip46Onboarding?
    /// Typed `signer_state` sidecar (`KSST`, ADR-0048 D6 — generalises the
    /// V-14 `bunker_connection_state` sidecar). `nil` while no remote-signer
    /// session is active — the steady state for local-key accounts. Read
    /// through the `signerState` accessor in `KernelModel+Projections`.
    /// `isReady`/`isAwaitingApproval`/`isReconnecting`/`isUnavailable`/
    /// `isFailed` drive the status badge in `AccountsView` for BOTH NIP-46
    /// and NIP-55 backends.
    @Published private(set) var typedSignerState: SignerState?

    /// Typed `wallet` (`NWST`) + `settings_hub` (`KSHB`) sidecars. `typedWallet`
    /// emits no sidecar while the wallet is disconnected, so nil is the steady
    /// state (the `walletStatus` accessor returns nil). `typedSettingsHub` is a
    /// single-key `["relay_count": Int]` dict the `settingsHub` accessor wraps.
    @Published private(set) var typedWallet: WalletStatusData?
    @Published private(set) var typedSettingsHub: [String: Int]?

    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs` / `lastErrorToast`), read directly off the `SnapshotFrame` table.
    /// Non-nil when the frame carried the typed envelope (gated on `metrics`,
    /// written unconditionally on every production frame). The authoritative
    /// source for those fields, read through the `KernelModel+Projections`
    /// envelope accessors (`isRunning` / `rev` / `metrics` / `relayStatuses` /
    /// `logicalInterests` / `wireSubscriptions` / `logs`).
    @Published private(set) var typedEnvelope: TypedSnapshotEnvelope?

    /// Dynamic flat feeds opened per profile/thread screen. Keys are
    /// `nmp.feed.author.<pubkey>` and `nmp.feed.thread.<event_id>`.
    @Published private(set) var flatFeeds: [String: OpFeedSnapshot] = [:]

    // ── Local mutable state ──────────────────────────────────────────────

    @Published private(set) var snapshotCount: UInt64 = 0
    @Published private(set) var lastSnapshotAt: Date?
    @Published private(set) var appMetrics = AppRuntimeMetrics()
    /// Snapshot-derived AND user-clearable, so we cannot fold this into the
    /// `snapshot` accessor — the clear gesture has nowhere else to land.
    @Published private(set) var lastErrorToast: String?
    /// Snapshot-driven machine error CODE (issue #1682), carried alongside
    /// `lastErrorToast`. The shell maps this stable code to LOCALIZED prose via
    /// `localizedErrorToast`; `lastErrorToast` is the English fallback when the
    /// code is unknown. Rust owns the code; the shell owns the prose.
    @Published private(set) var lastErrorCategory: String?
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

    let kernel = KernelHandle()
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
        seedChirpRelays(into: kernel)  // relay bootstrap + NMP_TEST_RELAYS seam (RelaySeeding.swift)
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
        // ADR-0055 R3-S3: reset the projection cache so the next frame after
        // restart is treated as a full baseline. Must happen BEFORE
        // clearTypedProjections so the cache is clean when the next
        // `listen` callback fires.
        kernel.projectionCache.reset()
        // ADR-0063 Lane E (#1671): reset the keyed-ref row cache too so the
        // next refs.profile / refs.event frame after restart is a full
        // baseline. Same baseline contract as `projectionCache`.
        kernel.keyedRefCache.reset()
        // Clear every typed projection slot so the computed accessors collapse
        // to their empty defaults. The next post-reset tick reassigns them all
        // unconditionally. Local-only slots clear explicitly below.
        clearTypedProjections()
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
        lastErrorCategory = nil
        capabilities.start()
        kernel.start(visibleLimit: visibleLimit, emitHz: emitHz)
        startedKernel = true
    }

    func applyConfiguration() {
        kernel.configure(visibleLimit: visibleLimit, emitHz: emitHz)
    }

    #if DEBUG
    /// Test-only seam (ADR-0063 Lane H, #1671): seed the per-key profile
    /// override `profileCard(forPubkey:)` reads when the kernel actor is not
    /// running, so tests exercise `profile(forPubkey:)` on the live read path
    /// (`keyedRefCache` → `profileCard(forPubkey:)`). ADR-0063 Lane H removed
    /// `claimed_profiles` (KCPR) and `resolved_profiles` (KRPR); callers now
    /// supply the merged map directly.
    func setTypedSnapshotForTesting(
        profileCards: [String: ProfileCard] = [:]
    ) {
        debugProfileCardOverrides = profileCards
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
    func authorFeed(pubkey: String) -> OpFeedSnapshot? {
        flatFeeds["nmp.feed.author.\(pubkey)"]
    }
    func threadFeed(eventID: String) -> OpFeedSnapshot? {
        flatFeeds["nmp.feed.thread.\(eventID)"]
    }
    // ADR-0063 Lane E (#1671): `NostrProfileHost` conformance — the shell
    // resolves/reads profiles ONLY via unified `resolve_ref` + `refs.profile`.
    func resolveProfile(
        pubkey: String, consumerID: String, shape: RefShape, liveness: RefLiveness
    ) {
        kernel.resolveRef(
            namespace: .profile, key: pubkey, consumerID: consumerID,
            shape: shape, liveness: liveness)
    }

    func releaseProfile(pubkey: String, consumerID: String) {
        kernel.releaseRef(namespace: .profile, key: pubkey, consumerID: consumerID)
    }

    func profileCard(forPubkey pubkey: String) -> ProfileCard? {
        #if DEBUG
        if let seeded = debugProfileCardOverrides[pubkey] { return seeded }
        #endif
        return kernel.keyedRefCache.profile(pubkey)
    }

    var profileRowChanged: AnyPublisher<KeyedRowChange, Never> {
        kernel.keyedRefCache.rowChanged.eraseToAnyPublisher()
    }

    /// ADR-0063 Lane E (#1671) — per-key typed EVENT accessor backed by
    /// `refs.event`. Exposed for parity; events still render via the
    /// `claimed_events` projection (Lane H converges them).
    func refEvent(_ primaryId: String) -> ClaimedEventDto? {
        kernel.keyedRefCache.event(primaryId)
    }

    /// ADR-0032 / V-115: bech32-encode a hex pubkey as `npub1…`.
    /// Returns nil on failure; callers fall back to hex display.
    func encodeProfile(pubkey: String) -> String? {
        kernel.encodeProfile(pubkey: pubkey)
    }

    /// NostrProfileHost conformance: look up a profile from the `refs.profile`
    /// keyed-ref cache (the source, D4). Overrides the protocol default only to
    /// keep the DEBUG name-regression instrumentation. `npub` is `nil` (V-115).
    func profile(forPubkey pubkey: String) -> ProfileWire? {
        if let card = profileCard(forPubkey: pubkey) {
            #if DEBUG
            if card.displayName?.isEmpty == false {
                markProfileNameResolved(pubkey)
            }
            #endif
            // ADR-0032 / V-115: bech32 `npub` no longer sent by projection.
            // Pass nil; callers encode bech32 host-side when needed.
            return ProfileWire(
                pubkey: pubkey,
                displayName: (card.displayName?.isEmpty == false) ? card.displayName : nil,
                about: card.about.isEmpty ? nil : card.about,
                pictureUrl: card.pictureUrl,
                nip05: card.nip05.isEmpty ? nil : card.nip05,
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

    /// Cancel an in-flight NIP-46 handshake. The handshake projection rides the
    /// `typedBunkerHandshake` sidecar, so reading `bunkerHandshake` reconciles
    /// automatically when the broker emits `idle` on the next tick.
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

    /// Dispatch `nmp.nip51.block_relay` for the active account.
    ///
    /// Reads the active account pubkey from `activeAccount` and includes it in
    /// the `BlockRelayInput` body so the router-owned ActionModule can read the
    /// current blocked set for idempotency. Fails immediately when no account
    /// is active (no spinner is started, no FFI call is made).
    @discardableResult
    func blockRelay(url: String) -> DispatchResult {
        guard let pubkey = activeAccount else {
            return .failure("block relay: no active account")
        }
        return track(kernel.dispatchChirpIntent(.blockRelay(url: url, accountPubkey: pubkey)))
    }

    /// Dispatch `nmp.nip51.unblock_relay` for the active account.
    ///
    /// Symmetric to `blockRelay`. The router-owned ActionModule rejects with a
    /// Conflict (no publish) when the relay is not currently blocked.
    @discardableResult
    func unblockRelay(url: String) -> DispatchResult {
        guard let pubkey = activeAccount else {
            return .failure("unblock relay: no active account")
        }
        return track(kernel.dispatchChirpIntent(.unblockRelay(url: url, accountPubkey: pubkey)))
    }

    @discardableResult
    func dispatchChirpIntent(_ intent: ChirpActionIntent) -> DispatchResult {
        track(kernel.dispatchChirpIntent(intent))
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
    func clearErrorToast() {
        lastErrorToast = nil
        lastErrorCategory = nil
    }

    /// Localized user-facing error prose for the current error toast
    /// (issue #1682). The shell OWNS the prose: it maps the Rust-supplied
    /// stable machine code (`lastErrorCategory`) to localized copy. Codes the
    /// shell does not recognize (e.g. relay-CLOSED categories, or any
    /// post-dated Rust code) fall back to the Rust English `lastErrorToast`.
    /// `nil` ⇒ no error toast on screen.
    var localizedErrorToast: String? {
        guard let toast = lastErrorToast else { return nil }
        guard let code = lastErrorCategory else { return toast }
        return UiErrorProse.localized(code: code) ?? toast
    }
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
        // Staleness guard on the typed envelope. Production always emits the
        // Tier-3 envelope (gated on `metrics`, written unconditionally by
        // `encode_snapshot_with_envelope`), so a tick with no envelope is not a
        // valid production frame and is dropped. `env.rev` is the authoritative
        // revision; `rev` (the accessor) reads the PREVIOUSLY-stored envelope
        // — assignment of `typedEnvelope` happens later in this body.
        guard let env = result.typedEnvelope, env.rev > rev else { return }

        let applyStart = ContinuousClock.now
        let callbackToApplyMicros = result.callbackReceivedAt.duration(to: applyStart).microseconds

        // ADR-0063 Lane E (#1671): merge the keyed reference row-delta batches
        // (`refs.profile` / `refs.event`) into the per-key `KeyedRefCache`.
        // Done HERE — on `@MainActor` — so the cache's per-key `rowChanged`
        // Combine publisher fires on the main thread and drives the per-key
        // avatar/name observers (exactly one row's subscribers re-render when
        // that one pubkey's kind:0 arrives). The cache is the SOURCE the
        // `profile(_:)` accessor reads; there is NO app-side profile cache (D4).
        for envelope in result.refsRowEnvelopes {
            kernel.keyedRefCache.merge(
                projectionKey: envelope.key,
                payload: envelope.payload,
                sessionId: result.refsSessionId,
                snapshotEpoch: result.refsSnapshotEpoch)
        }

        // Capture pre-assignment values for delta-driven side-effects below.
        // `priorActiveAccount` reads the OLD effective value through the
        // `activeAccount` accessor (the previous tick's typed sidecar).
        let priorActiveAccount = activeAccount
        // The NEW active account is the typed `active_account` sidecar (`nil`
        // when no account is active). Every internal consumer below (delta log,
        // marmot re-registration, follow-list active-pubkey forward) reads this
        // SAME value as the `activeAccount` UI accessor — single source.
        let newActiveAccount = result.typedActiveAccount
        if newActiveAccount != priorActiveAccount {
            kmLog.info(
                "apply: activeAccount \(priorActiveAccount ?? "nil") → \(newActiveAccount ?? "nil")")
        }

        #if DEBUG
        // B2: capture the rendered timeline-card count BEFORE the
        // typedHomeFeed assignment below. `modularTimeline` reads through that
        // slot, so reading it after the assignment would compare a value
        // against itself and the empty-after-nonempty detector would never
        // fire. `cards` is the per-thread-root row set the home feed renders.
        let prevTimelineCount = modularTimeline.cards.count
        #endif

        // ADR-0055 R3-S3 (D7): assign ONLY the @Published slots whose projection
        // key advanced in this frame (`result.changedKeys`). Slots NOT in the
        // set keep their prior value — the ProjectionMergeCache already retained
        // the decoded bytes and the TypedXDecoder.decode(from:) family already
        // read them from the full merged envelope set. This is the SwiftUI
        // broad-invalidation kill: we emit @Published changes only when the
        // underlying data actually changed.
        //
        // The `changedKeys` set uses the projection key strings exactly as
        // the TypedXDecoder enums declare them (TypedAccountsDecoder.key == "accounts",
        // etc.). For non-keyed slots (typedEnvelope, flatFeeds) we always assign.
        let ck = result.changedKeys
        // Issue #1283 Phase 1: EmbedHost is always updated when claimed_event_embeds
        // changed, or on first frame (cache is idempotent for unchanged data).
        if ck.contains(TypedClaimedEventEmbedsDecoder.key) {
            embedHost.update(envelopes: result.typedClaimedEventEmbeds)
        }
        // ADR-0038: typed home-feed slot.
        if ck.contains("nmp.feed.home") { typedHomeFeed = result.typedHomeFeed }
        // V6 Stage 4 (Wave B): accounts / active-account.
        if ck.contains(TypedAccountsDecoder.key) { typedAccounts = result.typedAccounts }
        if ck.contains(TypedActiveAccountDecoder.key) { typedActiveAccount = result.typedActiveAccount }
        // V6 Stage 4 (Wave B batch #2): relay-settings + publish-cluster slots.
        if ck.contains(TypedConfiguredRelaysDecoder.key) { typedConfiguredRelays = result.typedConfiguredRelays }
        if ck.contains(TypedRelayRoleOptionsDecoder.key) { typedRelayRoleOptions = result.typedRelayRoleOptions }
        if ck.contains(TypedOutboxSummaryDecoder.key) { typedOutboxSummary = result.typedOutboxSummary }
        if ck.contains(TypedPublishOutboxDecoder.key) { typedPublishOutbox = result.typedPublishOutbox }
        if ck.contains(TypedPublishQueueDecoder.key) { typedPublishQueue = result.typedPublishQueue }
        // V6 Stage 4 (Wave B batch #3): diagnostics + action-lifecycle slots.
        if ck.contains(TypedRelayDiagnosticsDecoder.key) { typedRelayDiagnostics = result.typedRelayDiagnostics }
        if ck.contains(TypedActionLifecycleDecoder.key) { typedActionLifecycle = result.typedActionLifecycle }
        // V6 Stage 4 (Wave B Tier-1 #4): app-projection typed slots.
        if ck.contains(TypedFollowListDecoder.key) { typedFollowList = result.typedFollowList }
        if ck.contains(TypedZapsDecoder.key) { typedZaps = result.typedZaps }
        if ck.contains(TypedGroupChatDecoder.key) { typedGroupChat = result.typedGroupChat }
        if ck.contains(TypedDiscoveredGroupsDecoder.key) { typedDiscoveredGroups = result.typedDiscoveredGroups }
        if ck.contains(TypedGroupDefaultsDecoder.key) { typedGroupDefaults = result.typedGroupDefaults }
        // ADR-0063 Lane E (#1671): profile slots are NOT mirrored into
        // `@Published` state — whole-map broadcast was the re-render bug.
        if ck.contains(TypedDmInboxDecoder.key) { typedDmInbox = result.typedDmInbox }
        if ck.contains(TypedDmRelayListDecoder.key) { typedDmRelayList = result.typedDmRelayList }
        if ck.contains(TypedClaimedEventsDecoder.key) { typedClaimedEvents = result.typedClaimedEvents }
        if ck.contains(TypedClaimedEventEmbedsDecoder.key) { typedClaimedEventEmbeds = result.typedClaimedEventEmbeds }
        // NIP-46 cluster.
        if ck.contains(TypedBunkerHandshakeDecoder.key) { typedBunkerHandshake = result.typedBunkerHandshake }
        if ck.contains(TypedNip46OnboardingDecoder.key) { typedNip46Onboarding = result.typedNip46Onboarding }
        // ADR-0048 D6: unified remote-signer health.
        if ck.contains(TypedSignerStateDecoder.key) { typedSignerState = result.typedSignerState }
        // Wallet + settings_hub.
        if ck.contains(TypedWalletDecoder.key) { typedWallet = result.typedWallet }
        if ck.contains(TypedSettingsHubDecoder.key) { typedSettingsHub = result.typedSettingsHub }
        // ADR-0044 Tier-3: the typed SnapshotFrame envelope is always updated
        // (it carries rev/metrics/logs/lastErrorToast which are per-tick).
        typedEnvelope = result.typedEnvelope
        // flatFeeds are Tier-1 dynamic keys; always pass through from the result
        // since they route via the extractFlatFeeds path independently.
        flatFeeds = result.flatFeeds
        // Snapshot-driven error toast, re-homed onto the typed envelope. Stays
        // in this distinct slot because tap-to-dismiss has nowhere else to land.
        lastErrorToast = env.lastErrorToast
        lastErrorCategory = env.lastErrorCategory

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
                "rev=%llu prev_count=%ld", env.rev, prevTimelineCount)
        }
        #endif

        let activeAccountChanged = newActiveAccount != priorActiveAccount
        if marmotRegistrationRequested, activeAccountChanged {
            _ = kernel.registerActiveMarmotIfAvailable()
            marmotRegistrationRequested = false
        }
        // V-107 / ADR-0039: Marmot state comes from push projections
        // (`nmp.marmot.snapshot` / `nmp.marmot.messages`) on the SnapshotFrame —
        // no more pull calls to `nmp_marmot_snapshot` / `nmp_marmot_group_messages`.
        // `isMarmotRegistered` still reads the handle slot (unchanged — it just
        // checks whether the handle is non-nil).
        //
        // The typed `NMMS`/`NMMG` sidecars are the sole source. `MarmotGroupChatView`
        // / `MarmotGroupsView` read off this same `marmot` store. A signed-out
        // tick yields nil from the typed decode → `apply` maps it to `.empty` /
        // `[:]` (the existing nil-handling is preserved).
        marmot.apply(
            snapshot: result.typedMarmotSnapshot,
            messages: result.typedMarmotMessages,
            isRegistered: kernel.isMarmotRegistered
        )
        // NIP-29 + NIP-17 stores — pushed every tick so their lazy init fires
        // on the first snapshot (registering the read projections in the
        // process). Rust owns the DM inbox interest lifecycle.
        // The typed `NGCS` sidecar is the sole source — the SAME value the
        // `typedGroupChat` slot holds, so the store never diverges from the UI.
        groupChat.apply(snapshot: result.typedGroupChat)
        // NIP-17 DM cluster: the typed `NDMI` sidecar is the sole source.
        // `DmListView` / `DmThreadView` read off this same `dmInbox` store.
        dmInbox.apply(snapshot: result.typedDmInbox)
        // NIP-02 follow list projection mirror. Push every tick so the store
        // tracks `projections["nmp.follow_list"]`. Touching `followList`
        // here forces the lazy `FollowListStore` init on the first snapshot,
        // which registers the read projection (`nmp_app_chirp_register_follow_list`).
        // The active-account pubkey is forwarded so the store can re-invoke
        // the FFI to update the projection's active-pubkey slot after sign-in.
        // The typed `NF02` sidecar is the sole source. `DmListView` reads
        // `model.followList.follows` off this same store.
        followList.apply(
            snapshot: result.typedFollowList,
            activePubkey: newActiveAccount
        )

        // NIP-29 group-discovery projection mirror. Push every tick so the
        // store tracks `projections["nmp.nip29.discovered_groups"]`. The store
        // is unwired until the user enters a relay and taps Search
        // (`searchGroups`); the snapshot key is `nil` until then, and the
        // store ignores stale snapshots from a previously-registered
        // relay during a switch.
        // The typed `NDGS` sidecar is the sole source.
        discoveredGroups.apply(snapshot: result.typedDiscoveredGroups)

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
                "NMP_DIAG logical_interests rev=\(env.rev, privacy: .public) \(logicalInterestSummary, privacy: .public)")
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
        let lastEventToEmit = env.metrics.lastEventToEmitMs.map(String.init) ?? "none"
        diagLog.debug(
            "NMP_PERF swift_apply rev=\(env.rev, privacy: .public) total_events=\(env.metrics.eventsRx, privacy: .public) batch_events=\(env.metrics.eventsSinceLastUpdate, privacy: .public) visible=\(env.metrics.visibleItems, privacy: .public) payload_bytes=\(result.payloadBytes, privacy: .public) rust_event_to_emit_ms=\(lastEventToEmit, privacy: .public) decode_us=\(result.decodeMicros, privacy: .public) callback_to_apply_us=\(callbackToApplyMicros, privacy: .public) apply_us=\(applyMicros, privacy: .public) callback_to_applied_us=\(callbackToAppliedMicros, privacy: .public)"
        )

        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }

    /// Null every typed projection slot so the computed accessors collapse to
    /// their empty defaults. Used by `resetAndRestart()`: the next tick
    /// reassigns each slot, so this is a transient blank, not a steady state.
    private func clearTypedProjections() {
        typedHomeFeed = nil
        typedAccounts = nil
        typedActiveAccount = nil
        typedConfiguredRelays = nil
        typedRelayRoleOptions = nil
        typedOutboxSummary = nil
        typedPublishOutbox = nil
        typedPublishQueue = nil
        typedRelayDiagnostics = nil
        typedActionLifecycle = nil
        typedFollowList = nil
        typedZaps = nil
        typedGroupChat = nil
        typedDiscoveredGroups = nil
        typedGroupDefaults = nil
        // ADR-0063 Lane E (#1671): profile slots removed; rows cleared by
        // `keyedRefCache.reset()`.
        typedDmInbox = nil
        typedDmRelayList = nil
        typedClaimedEvents = nil
        typedClaimedEventEmbeds = nil
        typedBunkerHandshake = nil
        typedNip46Onboarding = nil
        typedSignerState = nil
        typedWallet = nil
        typedSettingsHub = nil
        typedEnvelope = nil
    }

}
