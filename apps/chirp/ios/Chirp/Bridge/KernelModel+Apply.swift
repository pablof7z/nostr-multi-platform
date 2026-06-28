import Foundation
import os.log
import os.signpost

private let kmApplyLog = Logger(subsystem: "io.f7z.chirp", category: "KernelModel")
private let diagApplyLog = Logger(subsystem: "org.nmp.chirp.diag", category: "KernelModel")

#if DEBUG
private let applyReliabilityLog = OSLog(subsystem: "org.nmp.chirp.diag", category: "reliability")
#endif

// ── Snapshot apply ────────────────────────────────────────────────────────────

@MainActor
extension KernelModel {

    func apply(result: KernelUpdateResult) {
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
            kmApplyLog.info(
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
        // Issue #1283 Phase 1: EmbedHost is always updated when refs.event.envelopes
        // changed, or on first frame (cache is idempotent for unchanged data).
        if ck.contains(TypedRefEventEnvelopesDecoder.key) {
            embedHost.update(envelopes: result.typedRefEventEnvelopes)
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
        if ck.contains(TypedGroupEventsDecoder.key) { typedGroupChat = result.typedGroupChat }
        if ck.contains(TypedDiscoveredGroupsDecoder.key) { typedDiscoveredGroups = result.typedDiscoveredGroups }
        if ck.contains(TypedGroupDefaultsDecoder.key) { typedGroupDefaults = result.typedGroupDefaults }
        // ADR-0063 Lane E (#1671): profile slots are NOT mirrored into
        // `@Published` state — whole-map broadcast was the re-render bug.
        if ck.contains(TypedDmInboxDecoder.key) { typedDmInbox = result.typedDmInbox }
        if ck.contains(TypedDmRelayListDecoder.key) { typedDmRelayList = result.typedDmRelayList }
        if ck.contains(TypedRefEventEnvelopesDecoder.key) { typedRefEventEnvelopes = result.typedRefEventEnvelopes }
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
                .event, log: applyReliabilityLog, name: "timeline_empty_after_nonempty",
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
        // The typed `NGEV` sidecar is the sole source — the SAME value the
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
            diagApplyLog.debug(
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
        diagApplyLog.debug(
            "NMP_PERF swift_apply rev=\(env.rev, privacy: .public) total_events=\(env.metrics.eventsRx, privacy: .public) batch_events=\(env.metrics.eventsSinceLastUpdate, privacy: .public) visible=\(env.metrics.visibleItems, privacy: .public) payload_bytes=\(result.payloadBytes, privacy: .public) rust_event_to_emit_ms=\(lastEventToEmit, privacy: .public) decode_us=\(result.decodeMicros, privacy: .public) callback_to_apply_us=\(callbackToApplyMicros, privacy: .public) apply_us=\(applyMicros, privacy: .public) callback_to_applied_us=\(callbackToAppliedMicros, privacy: .public)"
        )

        snapshotCount &+= 1
        lastSnapshotAt = Date()
    }

    /// Null every typed projection slot so the computed accessors collapse to
    /// their empty defaults. Used by `resetAndRestart()`: the next tick
    /// reassigns each slot, so this is a transient blank, not a steady state.
    func clearTypedProjections() {
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
        typedGroupChat = nil
        typedDiscoveredGroups = nil
        typedGroupDefaults = nil
        // ADR-0063 Lane E (#1671): profile slots removed; rows cleared by
        // `keyedRefCache.reset()`.
        typedDmInbox = nil
        typedDmRelayList = nil
        typedRefEventEnvelopes = nil
        typedBunkerHandshake = nil
        typedNip46Onboarding = nil
        typedSignerState = nil
        typedWallet = nil
        typedSettingsHub = nil
        typedEnvelope = nil
    }
}
