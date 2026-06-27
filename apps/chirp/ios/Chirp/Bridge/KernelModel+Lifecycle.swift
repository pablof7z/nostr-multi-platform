import Foundation
import SwiftUI
import Combine
import os.log

private let kmLifecycleLog = Logger(subsystem: "io.f7z.chirp", category: "KernelModel")

// ── Lifecycle, liveness, open/close, profile resolution ──────────────────────

@MainActor
extension KernelModel {

    /// Set the actor-death flag. Idempotent: a second call is a no-op so the
    /// foreground-resume probe and the push-side panic frame cannot
    /// double-flip (or flicker on / off, which would be worse — the banner
    /// must stay up once raised).
    func markKernelDead() {
        if kernelIsDead { return }
        kmLifecycleLog.fault("kernelIsDead set — actor thread terminated")
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

    static func launchArgument(_ key: String) -> String? {
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
        kernel.resolveProfile(key: pubkey, consumerID: consumerID, shape: shape, liveness: liveness)
    }

    func releaseProfile(pubkey: String, consumerID: String) {
        kernel.releaseProfile(key: pubkey, consumerID: consumerID)
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
    /// `refs.event`.
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
    func markProfileNameResolved(_ pubkey: String) {
        debugPubkeysWithResolvedProfileNames.insert(pubkey)
        debugPubkeysMissingAfterResolvedProfileName.remove(pubkey)
    }

    func recordProfileNameMissIfRegression(_ pubkey: String) {
        guard debugPubkeysWithResolvedProfileNames.contains(pubkey) else { return }
        guard !debugPubkeysMissingAfterResolvedProfileName.contains(pubkey) else { return }
        debugPubkeysMissingAfterResolvedProfileName.insert(pubkey)
        appMetrics.recordNameRegression()
    }
    #endif
}
