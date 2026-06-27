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
    @Published var typedHomeFeed: OpFeedSnapshot?

    /// V6 Stage 4 (Wave B) typed `accounts` (`KACC` sidecar). `nil` ⇒ the
    /// `accounts` accessor collapses to `[]`.
    @Published var typedAccounts: [AccountSummary]?

    /// V6 Stage 4 (Wave B) typed `active_account` (`KACT` sidecar). `nil` ⇒ no
    /// active account (the `activeAccount` accessor returns `nil`).
    @Published var typedActiveAccount: String?

    /// V6 Stage 4 (Wave B batch #2) typed slots for the relay-settings +
    /// publish-cluster keys (`KCRL` / `KRRO` / `KOXS` / `KPBO` / `KPBQ`). `nil`
    /// ⇒ the matching accessor collapses to its empty default.
    @Published var typedConfiguredRelays: [AppRelay]?
    @Published var typedRelayRoleOptions: [RelayRoleOption]?
    @Published var typedOutboxSummary: OutboxSummary?
    @Published var typedPublishOutbox: [PublishOutboxItem]?
    @Published var typedPublishQueue: [PublishQueueEntry]?

    /// V6 Stage 4 (Wave B batch #3) typed slots for the diagnostics +
    /// action-lifecycle keys (`KRDG` / `KALC`). `nil` ⇒ the accessor collapses
    /// to its empty default.
    @Published var typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    @Published var typedActionLifecycle: ActionLifecycleSnapshot?

    /// V6 Stage 4 (Wave B Tier-1 #4) typed slots for the app-projection keys
    /// (`NF02` / `NGEV` / `NDGS`). These feed their dedicated stores
    /// (`FollowListStore` / `GroupChatStore` / `DiscoveredGroupsStore`) from the
    /// SAME typed value in `apply(result:)`, so store and accessor never diverge.
    @Published var typedFollowList: FollowListSnapshot?
    @Published var typedGroupChat: GroupEventsSnapshot?
    @Published var typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed `nmp.nip29.group_defaults` sidecar (`NGDF`, #626/#1924) — the
    /// app/operator-owned suggested public-group relay URL. `nil` ⇒ the
    /// `groupDefaults` accessor collapses to `.empty` and `NewGroupSheet` seeds
    /// an empty relay field.
    @Published var typedGroupDefaults: GroupDefaultsSnapshot?
    // ADR-0063 Lane E (#1671): the profile-cluster `@Published` slots
    // (`typedProfile` / `typedClaimedProfiles` / `typedResolvedProfiles`) are
    // REMOVED — a whole-map broadcast every tick re-rendered the whole view
    // tree on a single kind:0. Profiles flow only through `keyedRefCache`
    // (`refs.profile`), read per-key via `profileCard(forPubkey:)` (D4).
    #if DEBUG
    /// Test-only per-key profile seed `profileCard(forPubkey:)` reads when the
    /// kernel actor is not running (live path is `keyedRefCache`).
    var debugProfileCardOverrides: [String: ProfileCard] = [:]
    #endif
    /// Typed NIP-17 DM cluster sidecars (`NDMI` / `NDRL`). `typedDmInbox`
    /// feeds the `dmInbox` store; `typedDmRelayList` is read through the
    /// `dmRelayList` accessor (no consumer yet — wired for parity).
    /// Issue #1283 Phase 1: the embed resolver moved to Rust. `typedRefEventEnvelopes`
    /// carries the kernel-resolved `refs.event.envelopes` (`NEMB`) map that feeds
    /// `EmbedHost`; raw event refs flow through the keyed `refs.event` cache.
    @Published var typedDmInbox: DmInboxSnapshot?
    @Published var typedDmRelayList: DmRelayListSnapshot?
    @Published var typedRefEventEnvelopes: [String: EmbeddedEventEnvelope]?

    /// NIP-46 cluster typed sidecars (`bunker_handshake` / `nip46_onboarding`).
    /// `nil` ⇒ the `bunkerHandshake` / `nip46Onboarding` accessors return nil.
    @Published var typedBunkerHandshake: BunkerHandshake?
    @Published var typedNip46Onboarding: Nip46Onboarding?
    /// Typed `signer_state` sidecar (`KSST`, ADR-0048 D6 — generalises the
    /// V-14 `bunker_connection_state` sidecar). `nil` while no remote-signer
    /// session is active — the steady state for local-key accounts. Read
    /// through the `signerState` accessor in `KernelModel+Projections`.
    /// `isReady`/`isAwaitingApproval`/`isReconnecting`/`isUnavailable`/
    /// `isFailed` drive the status badge in `AccountsView` for BOTH NIP-46
    /// and NIP-55 backends.
    @Published var typedSignerState: SignerState?

    /// Typed `wallet` (`NWST`) + `settings_hub` (`KSHB`) sidecars. `typedWallet`
    /// emits no sidecar while the wallet is disconnected, so nil is the steady
    /// state (the `walletStatus` accessor returns nil). `typedSettingsHub` is a
    /// single-key `["relay_count": Int]` dict the `settingsHub` accessor wraps.
    @Published var typedWallet: WalletStatusData?
    @Published var typedSettingsHub: [String: Int]?

    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs` / `lastErrorToast`), read directly off the `SnapshotFrame` table.
    /// Non-nil when the frame carried the typed envelope (gated on `metrics`,
    /// written unconditionally on every production frame). The authoritative
    /// source for those fields, read through the `KernelModel+Projections`
    /// envelope accessors (`isRunning` / `rev` / `metrics` / `relayStatuses` /
    /// `logicalInterests` / `wireSubscriptions` / `logs`).
    @Published var typedEnvelope: TypedSnapshotEnvelope?

    /// Dynamic flat feeds opened per profile/thread screen. Keys are
    /// `nmp.feed.author.<pubkey>` and `nmp.feed.thread.<event_id>`.
    @Published var flatFeeds: [String: OpFeedSnapshot] = [:]

    // ── Local mutable state ──────────────────────────────────────────────

    @Published var snapshotCount: UInt64 = 0
    @Published var lastSnapshotAt: Date?
    @Published var appMetrics = AppRuntimeMetrics()
    /// Snapshot-derived AND user-clearable, so we cannot fold this into the
    /// `snapshot` accessor — the clear gesture has nowhere else to land.
    @Published var lastErrorToast: String?
    /// Snapshot-driven machine error CODE (issue #1682), carried alongside
    /// `lastErrorToast`. The shell maps this stable code to LOCALIZED prose via
    /// `localizedErrorToast`; `lastErrorToast` is the English fallback when the
    /// code is unknown. Rust owns the code; the shell owns the prose.
    @Published var lastErrorCategory: String?
    /// Success toast — set by Swift (not the Rust snapshot) when an async
    /// action settles with `Accepted`. Cleared by the overlay's `.task` TTL,
    /// same lifecycle as `lastErrorToast`.
    @Published var lastSuccessToast: String?
    /// Synchronous dispatch-error toast slot, distinct from the
    /// snapshot-driven `lastErrorToast`. Carries the human-readable reason
    /// returned by `dispatch_action` when it rejects a request synchronously
    /// (malformed body, unknown namespace, registry not initialised). NOT
    /// an action-lifecycle signal — a lifecycle failure surfaces through
    /// `actionLifecycle.recentTerminal[.failed(reason)]` from the projection.
    @Published var lastDispatchError: String?
    @Published var visibleLimit: UInt32 = 80
    @Published var emitHz: UInt32 = 4
    #if DEBUG
    var debugPubkeysWithResolvedProfileNames: Set<String> = []
    var debugPubkeysMissingAfterResolvedProfileName: Set<String> = []
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
    @Published var kernelIsDead: Bool = false

    // ── Stores & capabilities (non-published) ────────────────────────────

    let kernel = KernelHandle()
    /// Re-entrance guard for `start()`. The snapshot-driven `isRunning`
    /// accessor only flips after the first tick lands, so a re-entrant
    /// `start()` before then would dispatch the FFI twice.
    var startedKernel = false
    var lastLoadMoreCursor: TimelineWindowCursor?
    var lastLogicalInterestSummary = ""
    var marmotRegistrationRequested = false

    private(set) lazy var marmot = MarmotStore(kernel: kernel)
    @Published var groupChat: GroupChatStore
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
    /// D7/B1: This hardcoded relay is intentional app-owned demo-group
    /// identity for the first-consumer proof, not an NMP/shared-crate bootstrap
    /// relay. Kernel relay defaults flow through explicit app configuration
    /// and snapshots (`relayStatuses`, `configuredRelays`).
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
}
