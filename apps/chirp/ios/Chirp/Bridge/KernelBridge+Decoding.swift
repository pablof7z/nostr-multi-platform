import Foundation
import os.log

// ── FlatBuffers snapshot decode ───────────────────────────────────────────────
// Extracted from KernelBridge.swift to satisfy the 500-LOC ceiling (#962).

extension KernelHandle {
    static func decodeFlatBuffer(
        bytes: UnsafeRawPointer,
        count: Int,
        cache: ProjectionMergeCache
    ) -> KernelDecodedUpdateFrame? {
        let start = ContinuousClock.now
        let data = Data(bytes: bytes, count: count)
        do {
            let frame = try KernelUpdateFrameDecoder.decode(data)
            guard case let .snapshot(frameSchemaVersion, sessionId, snapshotEpoch, rawEnvelopes, flatFeeds, typedEnvelope) = frame else {
                if case let .panic(message) = frame {
                    kbLog.fault("NMP_ACTOR_PANIC detected bytes=\(data.count) msg=\(message, privacy: .public)")
                    return .panic(message)
                }
                return nil
            }
            // Enforce the schema version contract: a mismatch means Rust's
            // field layout changed in a way the host cannot safely interpret.
            // Return nil so the update is dropped rather than misparsed. The
            // generic `payload` is no longer decoded, so the frame-level
            // `schema_version` is the sole gate (it mirrors the former
            // payload-level check — both were written from the same value).
            guard frameSchemaVersion == KERNEL_SCHEMA_VERSION else {
                kbLog.error("schema version mismatch: frame=\(frameSchemaVersion) host=\(KERNEL_SCHEMA_VERSION) — snapshot rejected")
                return nil
            }
            // ADR-0055 R3-S3: Run the cache-merge BEFORE the TypedXDecoder
            // family. The merge re-feeds decoders the FULL merged envelope set
            // (retained cached rows for omitted keys, Cleared keys removed),
            // so they keep their exact current behavior. The merge also
            // surfaces the set of keys whose rev advanced in this frame and
            // the sticky needsResync flag.
            //
            // `sessionId` + `snapshotEpoch` were read off the SAME `frame.snapshot`
            // table in `KernelUpdateFrameDecoder.decode`'s single pass and threaded
            // out through the `.snapshot(...)` case — no second parse of the buffer
            // here (the whole point of this ladder is to stop paying O(buffer) per
            // tick). The `rawEnvelopes` already carry rev+state from
            // `extractTypedProjections`.
            let mergeResult = cache.merge(
                envelopes: rawEnvelopes,
                sessionId: sessionId,
                snapshotEpoch: snapshotEpoch
            )
            let envelopes = mergeResult.mergedEnvelopes
            let changedKeys = mergeResult.changedKeys
            let needsResync = mergeResult.needsResync
            // ADR-0063 Lane E (#1671): the keyed reference projections
            // (`refs.profile` / `refs.event`) are NOT routed through the
            // ProjectionMergeCache (which is keyed per WHOLE projection, not
            // per row). They carry an `nmp.refs.RefRowDeltaBatch` (NRRD) payload
            // that the per-key `KeyedRefCache` merges row-by-row. We carry the
            // RAW pre-merge envelopes through to `KernelModel.apply` so the
            // merge runs on `@MainActor` (its per-key `rowChanged` publisher
            // drives SwiftUI). Filter off `rawEnvelopes` — they hold the
            // verbatim wire payload, untouched by the projection-cache pass.
            let refsRowEnvelopes = rawEnvelopes.filter {
                KeyedRefCache.namespace(forProjectionKey: $0.key) != nil
            }
            if needsResync {
                kbLog.error("ProjectionMergeCache needsResync=true — one or more projection decode-before-commit failures; will be repaired on next genuine rev bump")
            }
            // ADR-0038 typed path: prefer the typed home-feed decode when the
            // NOFS sidecar is present and fully decodable (NFCT bytes filled).
            // Returns nil when absent or malformed → generic path stays active
            // (ADR-0037 Commitment 4 graceful fallback).
            // NOTE: flat feeds are extracted BEFORE the cache merge re-filters
            // the envelope set, so dynamic per-view feeds (author/thread) still
            // route correctly. They are Tier-1 always-Changed so the cache
            // pass-through is a no-op for them.
            let typedHomeFeed = TypedHomeFeedDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B): prefer the typed `accounts` / `active_account`
            // sidecars when present and well-formed. Each returns nil when the
            // sidecar is absent or malformed → the generic `projections.<field>`
            // JSON path stays active (ADR-0037 Commitment 4 graceful fallback),
            // exactly mirroring `typedHomeFeed` above.
            let typedAccounts = TypedAccountsDecoder.decode(from: envelopes)
            let typedActiveAccount = TypedActiveAccountDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B batch #2): the relay-settings + publish-cluster
            // thin-glue keys. Each returns nil when its sidecar is absent or
            // malformed → the generic `projections.<field>` JSON path stays
            // active (ADR-0037 Commitment 4), mirroring `typedAccounts` above.
            let typedConfiguredRelays = TypedConfiguredRelaysDecoder.decode(from: envelopes)
            let typedRelayRoleOptions = TypedRelayRoleOptionsDecoder.decode(from: envelopes)
            let typedOutboxSummary = TypedOutboxSummaryDecoder.decode(from: envelopes)
            let typedPublishOutbox = TypedPublishOutboxDecoder.decode(from: envelopes)
            let typedPublishQueue = TypedPublishQueueDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B batch #3): the diagnostics + action-lifecycle
            // keys. Each returns nil when its sidecar is absent or malformed →
            // the generic `projections.<field>` JSON path stays active
            // (ADR-0037 Commitment 4), mirroring `typedAccounts` above.
            let typedRelayDiagnostics = TypedRelayDiagnosticsDecoder.decode(from: envelopes)
            let typedActionLifecycle = TypedActionLifecycleDecoder.decode(from: envelopes)
            // V6 Stage 4 (Wave B Tier-1 #4): the app-projection keys
            // (`nmp.follow_list` / `nmp.nip29.group_events` /
            // `nmp.nip29.discovered_groups`). Each returns nil when its sidecar is
            // absent or malformed → the generic `projections.<field>` JSON path
            // stays active (ADR-0037 Commitment 4), mirroring `typedAccounts`
            // above. `nmp.follow_list`'s envelope KEY (`nmp.follow_list`) differs
            // from its payload SCHEMA_ID (`nmp.nip02.follow_list`); the generated
            // decoder matches on both.
            let typedFollowList = TypedFollowListDecoder.decode(from: envelopes)
            let typedGroupChat = TypedGroupEventsDecoder.decode(from: envelopes)
            let typedDiscoveredGroups = TypedDiscoveredGroupsDecoder.decode(from: envelopes)
            // #626/#1924: NIP-29 group-create defaults (NGDF). The
            // app/operator-owned suggested public-group relay URL. Output-only
            // projection; the producer registers it once at app init, so the
            // sidecar is present on every tick. Nil means the typed sidecar is
            // absent or malformed; there is no generic JSON projection for this
            // key.
            let typedGroupDefaults = TypedGroupDefaultsDecoder.decode(from: envelopes)
            // Profile-cluster typed sidecar (`profile` / KPRF). Returns nil when
            // its sidecar is absent/malformed. `claimed_profiles` (KCPR) and
            // `resolved_profiles` (KRPR) deleted — ADR-0063 Lane H. Profile data
            // is now served via the refs.profile KPRF NRRD row-delta sidecar.
            let typedProfile = TypedProfileDecoder.decode(from: envelopes)
            // NIP-17 DM cluster (`nmp.nip17.dm_inbox` /
            // `nmp.nip17.dm_relay_list`). Each returns nil when
            // its sidecar is absent/malformed → the generic `projections.<field>`
            // JSON path stays active (ADR-0037 Commitment 4), mirroring
            // `typedAccounts` above. `dm_relay_list` has no Swift read consumer
            // yet — the decode is wired for parity and unit-tested.
            let typedDmInbox = TypedDmInboxDecoder.decode(from: envelopes)
            let typedDmRelayList = TypedDmRelayListDecoder.decode(from: envelopes)
            // Issue #1283: the kernel-resolved embed map (`NEMB`). Returns nil
            // when the derived refs.event.envelopes sidecar is absent or
            // malformed. This feeds `EmbedHost` after the in-Swift resolver was
            // deleted.
            let typedRefEventEnvelopes = TypedRefEventEnvelopesDecoder.decode(from: envelopes)
            // NIP-46 cluster (`bunker_handshake` / `nip46_onboarding`). Each
            // returns nil when its sidecar is absent/malformed → the generic
            // `projections.<field>` JSON path stays active (ADR-0037 Commitment
            // 4), mirroring `typedAccounts` above. `bunker_handshake`'s typed
            // closure emits NO sidecar while idle (slot is `None`), so nil there
            // is the steady-state — the generic JSON `null` is the fallback.
            let typedBunkerHandshake = TypedBunkerHandshakeDecoder.decode(from: envelopes)
            let typedNip46Onboarding = TypedNip46OnboardingDecoder.decode(from: envelopes)
            // ADR-0048 D6: unified remote-signer health (`signer_state`, KSST —
            // generalises the V-14 `bunker_connection_state` sidecar). Nil while
            // no remote-signer session is active (slot is `None`) — the steady
            // state for local-key accounts. `isReady`/`isAwaitingApproval`/
            // `isReconnecting`/`isUnavailable`/`isFailed` drive status badges
            // for BOTH NIP-46 and NIP-55 backends; no generic JSON fallback
            // needed because iOS has always needed the sidecar (ADR-0037 §4).
            let typedSignerState = TypedSignerStateDecoder.decode(from: envelopes)
            // Marmot push-projection cluster (`nmp.marmot.snapshot` /
            // `nmp.marmot.messages`, V-107 / ADR-0039). Each returns nil when its
            // sidecar is absent/malformed → the generic `projections.<field>` JSON
            // path stays active (ADR-0037 Commitment 4), mirroring `typedAccounts`
            // above. The typed closures emit NO sidecar while signed-out (slot is
            // `None`), so nil there is the steady-state — the generic JSON
            // empty-object fallback applies and `MarmotStore.apply` maps it to
            // `.empty` / `[:]`.
            let typedMarmotSnapshot = TypedMarmotSnapshotDecoder.decode(from: envelopes)
            let typedMarmotMessages = TypedMarmotMessagesDecoder.decode(from: envelopes)
            // `wallet` (NWST, producer field-add) + `settings_hub` (KSHB, kernel
            // built-in). Each returns nil when its sidecar is absent/malformed →
            // the generic `projections.<field>` JSON path stays active (ADR-0037
            // Commitment 4), mirroring `typedAccounts` above. The wallet typed
            // closure emits NO sidecar while disconnected (slot is `None`), so nil
            // there is the steady-state — the generic JSON `null` fallback applies.
            let typedWallet = TypedWalletDecoder.decode(from: envelopes)
            let typedSettingsHub = TypedSettingsHubDecoder.decode(from: envelopes)
            // Wave C: action_results, action_stages.
            // V-112 (ADR-0042): author_view / thread_view typed sidecars deleted.
            let typedActionResults = TypedActionResultsDecoder.decode(from: envelopes)
            let typedActionStages = TypedActionStagesDecoder.decode(from: envelopes)
            let duration = start.duration(to: .now)
            kbLog.info("decoded ok rev=\(typedEnvelope?.rev ?? 0) activeAccount=\(typedActiveAccount ?? "nil")")
            return .snapshot(
                KernelUpdateResult(
                    typedHomeFeed: typedHomeFeed,
                    typedAccounts: typedAccounts,
                    typedActiveAccount: typedActiveAccount,
                    typedConfiguredRelays: typedConfiguredRelays,
                    typedRelayRoleOptions: typedRelayRoleOptions,
                    typedOutboxSummary: typedOutboxSummary,
                    typedPublishOutbox: typedPublishOutbox,
                    typedPublishQueue: typedPublishQueue,
                    typedRelayDiagnostics: typedRelayDiagnostics,
                    typedActionLifecycle: typedActionLifecycle,
                    typedFollowList: typedFollowList,
                    typedGroupChat: typedGroupChat,
                    typedDiscoveredGroups: typedDiscoveredGroups,
                    typedGroupDefaults: typedGroupDefaults,
                    typedProfile: typedProfile,
                    typedDmInbox: typedDmInbox,
                    typedDmRelayList: typedDmRelayList,
                    typedRefEventEnvelopes: typedRefEventEnvelopes,
                    typedBunkerHandshake: typedBunkerHandshake,
                    typedNip46Onboarding: typedNip46Onboarding,
                    typedSignerState: typedSignerState,
                    typedMarmotSnapshot: typedMarmotSnapshot,
                    typedMarmotMessages: typedMarmotMessages,
                    typedWallet: typedWallet,
                    typedSettingsHub: typedSettingsHub,
                    typedActionResults: typedActionResults,
                    typedActionStages: typedActionStages,
                    // V-112 (ADR-0042): typedAuthorView / typedThreadView removed.
                    typedEnvelope: typedEnvelope,
                    flatFeeds: flatFeeds,
                    payloadBytes: data.count,
                    callbackReceivedAt: start,
                    decodeMicros: duration.microseconds,
                    changedKeys: changedKeys,
                    needsResync: needsResync,
                    refsRowEnvelopes: refsRowEnvelopes,
                    refsSessionId: sessionId,
                    refsSnapshotEpoch: snapshotEpoch
                )
            )
        } catch let error as DecodingError {
            switch error {
            case let .keyNotFound(key, ctx):
                kbLog.error("FlatBuffers decode: keyNotFound '\(key.stringValue)' at \(ctx.codingPath.map(\.stringValue).joined(separator: ".")) bytes=\(data.count)")
            case let .typeMismatch(_, ctx):
                kbLog.error("FlatBuffers decode: typeMismatch at \(ctx.codingPath.map(\.stringValue).joined(separator: ".")) — \(ctx.debugDescription) bytes=\(data.count)")
            default:
                kbLog.error("FlatBuffers decode error: \(error.localizedDescription) bytes=\(data.count)")
            }
            return nil
        } catch {
            kbLog.error("FlatBuffers snapshot decode error: \(error.localizedDescription) bytes=\(data.count)")
            return nil
        }
    }
}
