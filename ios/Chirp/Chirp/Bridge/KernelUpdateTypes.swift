import Foundation

// Update-frame, typed snapshot envelope, dispatch-result, and create-account
// DTOs for the KernelBridge FFI seam. Extracted from `KernelBridge.swift` so the
// bridge file holds only `KernelHandle` (file-size hard-cap separation). These
// are pure value types; same-module Swift files see each other without import.

enum KernelDecodedUpdateFrame {
    case snapshot(KernelUpdateResult)
    case panic(String)
}

// ─── Typed SnapshotFrame envelope (ADR-0044 Tier-3) ───────────────────────

/// The typed `SnapshotFrame` envelope fields, read DIRECTLY off the
/// `SnapshotFrame` table (ADR-0044) — distinct from the `typed_projections`
/// sidecar list every other `typed*` decode walks. PR #1034 added these
/// first-class fields (`rev`, `running`, `metrics`, the relay/interest/wire
/// vectors, `logs`) on the frame so a migrated host reads them instead of
/// re-walking the generic JSON `payload` tree.
///
/// All seven fields are written by the producer as a UNIT
/// (`encode_snapshot_with_envelope`, `kernel/update.rs`) whenever the frame
/// carries metrics, so this whole struct is gated on the one field whose
/// FlatBuffers accessor reports presence (`SnapshotFrame.metrics != nil`). When
/// the gate is open the host prefers these typed values; when it is closed (a
/// legacy frame, or the test-only `encode_snapshot_with_typed` path) the value
/// is `nil` and every accessor falls through to the generic JSON `payload`
/// (`snapshot?.<field>`) — ADR-0037 Commitment 4. Every value is a raw mirror
/// of the top-level `KernelSnapshot` fields (ADR-0032), field-identical to the
/// JSON decode. This is the LAST consumer of the generic `payload`'s top-level
/// scalars.
struct TypedSnapshotEnvelope: Equatable {
    let rev: UInt64
    let running: Bool
    let metrics: KernelMetrics
    let relayStatuses: [RelayStatus]
    let logicalInterests: [LogicalInterestStatus]
    let wireSubscriptions: [WireSubscriptionStatus]
    let logs: [String]
    /// Snapshot-driven error toast — read DIRECTLY off the `SnapshotFrame`
    /// table (`last_error_toast`), the same first-class envelope tier as the
    /// other fields. `nil` ⇒ no toast on this tick. This re-homes the last
    /// raw whole-payload read (`update.lastErrorToast`) onto the typed
    /// envelope; `KernelModel` copies it into its user-clearable
    /// `lastErrorToast` slot in `apply(result:)`.
    let lastErrorToast: String?
    /// Snapshot-driven machine error CODE — read off `SnapshotFrame`'s
    /// `last_error_category` (issue #1682). `nil` ⇒ no categorized error on
    /// this tick. The shell maps this stable code to LOCALIZED prose
    /// (`KernelModel.localizedErrorToast`); `lastErrorToast` is the English
    /// fallback for codes the shell does not recognize. Rust owns the code +
    /// raw diagnostic detail; the shell owns the prose.
    let lastErrorCategory: String?
}

// ─── Swift-side timing wrapper ────────────────────────────────────────────

struct KernelUpdateResult {
    /// Typed home-feed decode result (ADR-0038 typed path). Non-nil when the
    /// snapshot carried a well-formed `NOFS` typed projection that the Swift
    /// `NFCT` decoder could fully populate. `nil` means the generic
    /// `projections.homeFeed` fallback applies (ADR-0037 Commitment 4).
    let typedHomeFeed: OpFeedSnapshot?
    /// Typed `accounts` projection decode (V6 Stage 4 / Wave B `KACC` sidecar).
    /// Non-nil when the snapshot carried a well-formed `accounts` typed sidecar;
    /// `nil` means the generic `projections.accounts` JSON fallback applies.
    let typedAccounts: [AccountSummary]?
    /// Typed `active_account` projection decode (V6 Stage 4 / Wave B `KACT`
    /// sidecar). Non-nil when the snapshot carried a well-formed `active_account`
    /// typed sidecar that resolved to an active pubkey; `nil` means either no
    /// sidecar OR no active account — both defer to the generic
    /// `projections.active_account` JSON fallback (parity-preserving).
    let typedActiveAccount: String?
    /// Typed `configured_relays` projection decode (V6 Stage 4 / Wave B `KCRL`
    /// sidecar). `nil` ⇒ the generic `projections.configured_relays` JSON
    /// fallback applies.
    let typedConfiguredRelays: [AppRelay]?
    /// Typed `relay_role_options` projection decode (`KRRO`). `nil` ⇒ generic
    /// `projections.relay_role_options` JSON fallback.
    let typedRelayRoleOptions: [RelayRoleOption]?
    /// Typed `outbox_summary` projection decode (`KOXS`). `nil` ⇒ generic
    /// `projections.outbox_summary` JSON fallback.
    let typedOutboxSummary: OutboxSummary?
    /// Typed `publish_outbox` projection decode (`KPBO`). `nil` ⇒ generic
    /// `projections.publish_outbox` JSON fallback.
    let typedPublishOutbox: [PublishOutboxItem]?
    /// Typed `publish_queue` projection decode (`KPBQ`). The domain type is a
    /// field-subset of the wire. `nil` ⇒ generic `projections.publish_queue`
    /// JSON fallback.
    let typedPublishQueue: [PublishQueueEntry]?
    /// Typed `relay_diagnostics` projection decode (`KRDG`). `nil` ⇒ generic
    /// `projections.relay_diagnostics` JSON fallback.
    let typedRelayDiagnostics: RelayDiagnosticsSnapshot?
    /// Typed `action_lifecycle` projection decode (`KALC`). `nil` ⇒ generic
    /// `projections.action_lifecycle` JSON fallback.
    let typedActionLifecycle: ActionLifecycleSnapshot?
    /// Typed `nmp.follow_list` projection decode (`NF02`; envelope key
    /// `nmp.follow_list`, schema_id `nmp.nip02.follow_list`). `nil` ⇒ generic
    /// `projections["nmp.follow_list"]` JSON fallback.
    let typedFollowList: FollowListSnapshot?
    /// Typed `nmp.nip57.zaps` projection decode (`NZAP`). `nil` ⇒ generic
    /// `projections["nmp.nip57.zaps"]` JSON fallback.
    let typedZaps: ZapsAggregateSnapshot?
    /// Typed `nmp.nip29.group_chat` projection decode (`NGCS`). `nil` ⇒ generic
    /// `projections["nmp.nip29.group_chat"]` JSON fallback.
    let typedGroupChat: GroupChatSnapshot?
    /// Typed `nmp.nip29.discovered_groups` projection decode (`NDGS`). `nil` ⇒
    /// generic `projections["nmp.nip29.discovered_groups"]` JSON fallback.
    let typedDiscoveredGroups: DiscoveredGroupsSnapshot?
    /// Typed `nmp.nip29.group_defaults` projection decode (`NGDF`, #626). `nil` ⇒
    /// generic `projections["nmp.nip29.group_defaults"]` JSON fallback. The
    /// crate-owned suggested public-group relay URL; read typed-first through the
    /// `groupDefaults` accessor and seeded into `NewGroupSheet`'s editable relay
    /// field. The output-only producer registers once at app init, so a current
    /// kernel emits this on every tick (nil only on an older build).
    let typedGroupDefaults: GroupDefaultsSnapshot?
    /// Typed `profile` projection decode (`KPRF`). `nil` ⇒ generic
    /// `projections["profile"]` JSON fallback.
    let typedProfile: ProfileCard?
    /// Typed `claimed_profiles` projection decode (`KCPR`). `nil` ⇒ generic
    /// `projections["claimed_profiles"]` JSON fallback.
    let typedClaimedProfiles: [String: ProfileCard]?
    /// Typed `resolved_profiles` projection decode (`KRPR`). `nil` ⇒ generic
    /// `projections["resolved_profiles"]` JSON fallback.
    let typedResolvedProfiles: [String: ProfileCard]?
    /// Typed `nmp.nip17.dm_inbox` projection decode (`NDMI`). `nil` ⇒ generic
    /// `projections["nmp.nip17.dm_inbox"]` JSON fallback. Routed to the
    /// `dmInbox` store (typed-first effective value) in `KernelModel.apply`.
    let typedDmInbox: DmInboxSnapshot?
    /// Typed `nmp.nip17.dm_relay_list` projection decode (`NDRL`). `nil` ⇒ generic
    /// `projections["nmp.nip17.dm_relay_list"]` JSON fallback. No Swift read
    /// consumer yet — read through the `dmRelayList` accessor (added for parity).
    let typedDmRelayList: DmRelayListSnapshot?
    /// Typed `claimed_events` projection decode (`KCEV`). `nil` ⇒ generic
    /// `projections.claimedEvents` JSON fallback. Still a live projection; no
    /// longer the embed-resolution input (issue #1283 Phase 1 — see below).
    let typedClaimedEvents: [String: ClaimedEventDto]?
    /// Typed `claimed_event_embeds` projection decode (`NEMB`, issue #1283
    /// Phase 1). `nil` ⇒ generic `projections.claimedEventEmbeds` JSON fallback.
    /// The kernel-resolved (`nmp_content::resolve_embed_projection`) embed map;
    /// routed to `EmbedHost.update(envelopes:)` in `KernelModel.apply`. Replaces
    /// the deleted in-Swift resolver — this is what closes the EmbedHost D0
    /// violation and fixes the #1299 display_name precedence.
    let typedClaimedEventEmbeds: [String: EmbeddedEventEnvelope]?
    /// Typed `bunker_handshake` projection decode (`KBHS`). `nil` ⇒ generic
    /// `projections["bunker_handshake"]` JSON fallback. The producer emits no
    /// sidecar while the handshake slot is idle, so nil is the steady state.
    let typedBunkerHandshake: BunkerHandshake?
    /// Typed `nip46_onboarding` projection decode (`KN46`). `nil` ⇒ generic
    /// `projections["nip46_onboarding"]` JSON fallback. Always present from a
    /// current kernel (the static signer-app table is emitted every tick).
    let typedNip46Onboarding: Nip46Onboarding?
    /// Typed `signer_state` projection decode (`KSST`). ADR-0048 D6 —
    /// generalises the V-14 `bunker_connection_state` sidecar. `nil` while no
    /// remote-signer session is active — the steady state for local-key
    /// accounts; no JSON fallback available because iOS is typed-sidecar-only
    /// (ADR-0037 §4). When non-nil, `isReady` drives the green dot,
    /// `isAwaitingApproval` the "Waiting for Amber…" affordance,
    /// `isReconnecting` the amber badge, and `isUnavailable`/`isFailed` the
    /// red re-auth prompt (ADR-0032 / relay_diagnostics pattern).
    let typedSignerState: SignerState?
    /// Typed `nmp.marmot.snapshot` projection decode (`NMMS`, V-107 / ADR-0039).
    /// `nil` ⇒ generic `projections["nmp.marmot.snapshot"]` JSON fallback. Routed
    /// to `MarmotStore.apply` (typed-first effective value) in `KernelModel.apply`.
    /// The producer emits no sidecar while signed-out, so nil is the steady state.
    let typedMarmotSnapshot: MarmotSnapshot?
    /// Typed `nmp.marmot.messages` projection decode (`NMMG`, V-107 / ADR-0039).
    /// `nil` ⇒ generic `projections["nmp.marmot.messages"]` JSON fallback. The
    /// flattened-vector wire rebuilds the `group_id_hex -> [MarmotMessage]` map.
    /// Routed to `MarmotStore.apply` (typed-first effective value) in
    /// `KernelModel.apply`.
    let typedMarmotMessages: [String: [MarmotMessage]]?
    /// Typed `wallet` projection decode (`NWST`). `nil` ⇒ generic
    /// `projections["wallet"]` JSON fallback. Read typed-first through the
    /// `walletStatus` accessor (`typedWallet ?? snapshot?.walletStatus`) in
    /// `KernelModel+Projections`. The producer emits no sidecar while the wallet
    /// is disconnected (slot is `None`), so nil is the steady state. The
    /// `wallet_pubkey_hex` producer field-add unblocked this flip.
    let typedWallet: WalletStatusData?
    /// Typed `settings_hub` projection decode (`KSHB`, kernel built-in). `nil` ⇒
    /// generic `projections["settings_hub"]` JSON fallback. The single-key
    /// `["relay_count": Int]` dict is read typed-first through the `settingsHub`
    /// accessor in `KernelModel+Projections` and wrapped into `SettingsHubSummary`.
    let typedSettingsHub: [String: Int]?
    /// Wave C: Typed `action_results` projection decode (`KARS`). `nil` ⇒ generic
    /// `projections.action_results` JSON fallback. The per-tick drain array; maps
    /// each `ActionResult` row to `LastActionResult`. NOTE: no read site wired yet
    /// (foundation only; wire typed-first in `KernelModel.apply` as follow-up).
    let typedActionResults: [LastActionResult]?
    /// Wave C: Typed `action_stages` projection decode (`KAST`). `nil` ⇒ generic
    /// `projections.action_stages` JSON fallback. The flat-vector wire rebuilds
    /// the `[correlation_id: [ActionStageEntry]]` dictionary. NOTE: no read site
    /// wired yet (foundation only; wire typed-first in `KernelModel.apply` as
    /// follow-up).
    let typedActionStages: [String: [ActionStageEntry]]?
    // V-112 (ADR-0042): typedAuthorView (AuthorProfileSnapshot) and
    // typedThreadView (ThreadView) deleted — author_view / thread_view typed
    // sidecars removed with AuthorViewState / ThreadViewState.
    /// ADR-0044 Tier-3: the typed `SnapshotFrame` envelope (`rev` / `running` /
    /// `metrics` / `relayStatuses` / `logicalInterests` / `wireSubscriptions` /
    /// `logs`), read directly off the `SnapshotFrame` table. Non-nil when the
    /// frame carried the typed envelope (gated on `metrics`); `nil` ⇒ the
    /// generic JSON `payload` top-level scalars apply (read through the
    /// `KernelModel+Projections` accessors).
    let typedEnvelope: TypedSnapshotEnvelope?
    /// Dynamic per-screen flat feeds keyed as `nmp.feed.author.<pubkey>` or
    /// `nmp.feed.thread.<event_id>`. These keys are opened per navigation
    /// target, so they cannot be codegen'd as fixed projection fields.
    let flatFeeds: [String: OpFeedSnapshot]
    let payloadBytes: Int
    let callbackReceivedAt: ContinuousClock.Instant
    let decodeMicros: Int
    /// R3-S3 (ADR-0055 D7): the set of projection keys whose `projectionRev`
    /// advanced in this frame. `KernelModel.apply(result:)` assigns ONLY the
    /// `@Published` slots corresponding to these keys; slots NOT in the set
    /// keep their prior value (the `ProjectionMergeCache` already retained the
    /// decoded bytes). This is the SwiftUI broad-invalidation kill.
    let changedKeys: Set<String>
    /// R3-S3 (ADR-0055 D3-4): latched `true` when the cache-merge layer
    /// encountered a typed-decode failure for at least one row. The prior cache
    /// entry is retained (no silent corruption), but the host is
    /// known-degraded for that key until the next genuine rev bump. Rung 3
    /// logs this; Rung 4 drains it via `nmp_app_request_full_snapshot()`.
    let needsResync: Bool
}

// ─── dispatch_action return envelope (PR-A) ───────────────────────────────

/// Synchronous outcome of `nmp_app_dispatch_action`. The Rust kernel returns
/// `{"correlation_id":"<id>"}` on accept (the action was validated, minted a
/// correlation id, and routed to its executor), or `{"error":"<message>"}` on
/// reject (null app, unknown namespace, malformed JSON, module validator
/// rejection). PR-A: the Swift bridge parses this envelope so a caller can
/// drive a spinner keyed on the correlation_id and surface the error message
/// as a toast on the reject path.
///
/// The terminal verdict ("published" / "failed" / "cancelled") is a SEPARATE
/// async signal — match the `correlation_id` against
/// `projections["action_results"]` on subsequent snapshot ticks.
enum DispatchResult: Equatable {
    /// The action was accepted and enqueued. Carries the `correlation_id`
    /// minted by `ActionRegistry::start`. V5: the kernel's
    /// `action_lifecycle` projection automatically surfaces this id under
    /// `inFlight` until the action settles, then under `recentTerminal`
    /// for a 3-second window. The host does NOT maintain its own pending
    /// set — read `model.actionLifecycle` to drive the UI.
    case accepted(correlationId: String)
    /// The action was rejected synchronously. Carries the human-readable
    /// error from the Rust kernel — show it as a toast.
    case failure(_ message: String)

    var correlationId: String? {
        if case let .accepted(id) = self { return id }
        return nil
    }

    var errorMessage: String? {
        if case let .failure(msg) = self { return msg }
        return nil
    }

    /// Parse the JSON envelope returned by `nmp_app_dispatch_action`.
    ///
    /// The kernel's contract (`ffi/action.rs`): every non-null app returns
    /// either `{"correlation_id":"<32-hex or event-id>"}` (accepted) or an
    /// envelope carrying an `error`. A *synchronous rejection that still minted
    /// a correlation_id* returns BOTH fields
    /// (`{"correlation_id":…,"error":…}`) — the action was assigned an id but
    /// then refused before any work was enqueued.
    ///
    /// #1676 BUG-C: `error` is inspected FIRST. The prior order read
    /// `correlation_id` first and returned `.accepted` whenever it was present,
    /// silently discarding the `error` string on the both-fields envelope — the
    /// sync failure vanished and only ever surfaced (if at all) via a later
    /// async terminal. Surfacing the error here means the caller shows the
    /// rejection toast immediately; the kernel still records the matching
    /// `Failed` terminal under the same id for any host watching the lifecycle
    /// projection.
    static func parse(envelope: String) -> DispatchResult {
        guard let data = envelope.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return .failure("dispatch envelope was not a JSON object (bytes=\(envelope.utf8.count))")
        }
        if let message = object["error"] as? String {
            return .failure(message)
        }
        if let correlationId = object["correlation_id"] as? String, !correlationId.isEmpty {
            return .accepted(correlationId: correlationId)
        }
        return .failure("dispatch envelope missing both correlation_id and error (bytes=\(envelope.utf8.count))")
    }
}

// ─── createAccount FFI payload (Codable, PR-L) ────────────────────────────

/// JSON payload for `nmp_app_create_new_account` — typed wrapper for the
/// profile metadata + onboarding relay seed list. The wire shape mirrors
/// what the Rust FFI expects exactly: a flat profile object
/// (`{"name":"…","about":"…"}`) and an array of two-element relay tuples
/// (`[["wss://…", "both"], …]`).
///
/// PR-L: replaces the `JSONSerialization.data(withJSONObject:)` + `try!`
/// path in `KernelBridge.createAccount` so a typed-but-impossible encode
/// failure surfaces as a toast instead of trapping the process.
struct CreateAccountFFIPayload: Encodable {
    let profile: [String: String]
    let relays: [[String]]

    init(profile: [String: String], relays: [(String, String)]) {
        self.profile = profile
        self.relays = relays.map { [$0.0, $0.1] }
    }
}

extension Duration {
    var microseconds: Int {
        let parts = components
        return Int(parts.seconds) * 1_000_000 + Int(parts.attoseconds / 1_000_000_000_000)
    }
}

/// Shell-owned localized prose for Rust-supplied structured error tokens
/// (issue #1682). Rust emits a stable machine `code` (carried on the snapshot
/// as `last_error_category`) plus an English fallback (`last_error_toast`); the
/// shell maps the code to localized user-facing copy here. This is the
/// presentation half of the codex ruling: Rust owns error semantics + raw
/// diagnostics, the shell owns prose.
///
/// `localized(code:)` returns `nil` for unrecognized codes (e.g. relay-CLOSED
/// categories, or any newer Rust code this build predates) so the caller falls
/// back to the Rust English prose. The keys mirror the producer crates'
/// `ui_codes` / `ui_token::codes` constants (the closed catalog).
enum UiErrorProse {
    static func localized(code: String) -> String? {
        switch code {
        // ── nmp-nip17 (DM send) ──────────────────────────────────────────
        case "nip17_dm_send_failed":
            return NSLocalizedString(
                "error.nip17.dm_send_failed",
                value: "Couldn’t send the message.",
                comment: "Toast: a direct message failed to send")
        case "nip17_dm_giftwrap_failed":
            return NSLocalizedString(
                "error.nip17.dm_giftwrap_failed",
                value: "Couldn’t send the message — delivery failed.",
                comment: "Toast: DM gift-wrap publish failed")
        // ── nmp-nip47 (NWC wallet) ───────────────────────────────────────
        case "nip47_invalid_uri":
            return NSLocalizedString(
                "error.nip47.invalid_uri",
                value: "That wallet connection link isn’t valid.",
                comment: "Toast: invalid NWC URI")
        case "nip47_invalid_client_secret":
            return NSLocalizedString(
                "error.nip47.invalid_client_secret",
                value: "That wallet connection link is malformed.",
                comment: "Toast: invalid NWC client secret")
        case "nip47_req_encode_failed", "nip47_encrypt_failed",
             "nip47_sign_failed", "nip47_event_encode_failed":
            return NSLocalizedString(
                "error.nip47.request_failed",
                value: "Couldn’t reach your wallet. Please try again.",
                comment: "Toast: an NWC request could not be built/sent")
        case "nip47_wallet_error", "nip47_wallet_auth_error":
            return NSLocalizedString(
                "error.nip47.wallet_error",
                value: "Your wallet reported an error.",
                comment: "Toast: the wallet service returned an error")
        case "nip47_wallet_not_ready":
            return NSLocalizedString(
                "error.nip47.wallet_not_ready",
                value: "Your wallet is still connecting.",
                comment: "Toast: wallet not ready for a payment")
        case "nip47_wallet_not_connected":
            return NSLocalizedString(
                "error.nip47.wallet_not_connected",
                value: "No wallet is connected.",
                comment: "Toast: no wallet connected for a payment")
        case "nip47_payment_aborted_no_durable_record":
            return NSLocalizedString(
                "error.nip47.payment_aborted",
                value: "Payment cancelled to keep it safe — please try again.",
                comment: "Toast: payment aborted because its record could not be saved")
        // ── nmp-core (kernel / actor) ────────────────────────────────────
        case "core_keyring_write_failed":
            return NSLocalizedString(
                "error.core.keyring_write_failed",
                value: "Couldn’t save your sign-in securely — it may not persist.",
                comment: "Toast: keychain write failed")
        case "core_relay_processing_error":
            return NSLocalizedString(
                "error.core.relay_processing_error",
                value: "A relay update hit a snag — continuing.",
                comment: "Toast: a relay event handler panicked but was contained")
        case "signer_bunker_invalid_uri":
            return NSLocalizedString(
                "error.signer.bunker_invalid_uri",
                value: "That remote signer link isn’t valid.",
                comment: "Toast: invalid bunker:// URI")
        case "signer_broker_not_initialised":
            return NSLocalizedString(
                "error.signer.broker_not_initialised",
                value: "Remote signing isn’t available right now.",
                comment: "Toast: NIP-46 broker not initialised")
        case "signer_nip55_driver_not_initialised":
            return NSLocalizedString(
                "error.signer.nip55_not_initialised",
                value: "External signing isn’t available right now.",
                comment: "Toast: NIP-55 driver not initialised")
        default:
            return nil
        }
    }
}

/// Localized prose for NIP-46/NIP-55 handshake PROGRESS labels (#1711), the
/// parallel of `UiErrorProse` for `Severity.Progress` tokens. The kernel +
/// signer-broker ship a stable `progress_code`; this maps it to localized copy,
/// returning `nil` for an unrecognized key so the caller falls back to the
/// English `progressMessage` the wire still carries.
enum UiProgressProse {
    static func localized(code: String) -> String? {
        switch code {
        case "signer_progress_waiting_for_broker":
            return NSLocalizedString(
                "progress.signer.waiting_for_broker",
                value: "Waiting for the remote signer…",
                comment: "Progress: opening a NIP-46 bunker session")
        case "signer_progress_restoring_broker_session":
            return NSLocalizedString(
                "progress.signer.restoring_broker_session",
                value: "Restoring your remote signer…",
                comment: "Progress: restoring a persisted NIP-46 session at launch")
        case "signer_progress_sending_connect_to_bunker":
            return NSLocalizedString(
                "progress.signer.sending_connect",
                value: "Connecting to the bunker…",
                comment: "Progress: sending the NIP-46 connect request")
        case "signer_progress_awaiting_bunker_approval":
            return NSLocalizedString(
                "progress.signer.awaiting_bunker_approval",
                value: "Approve the request in your bunker app.",
                comment: "Progress: waiting for the user to approve in the bunker app")
        case "signer_progress_nostrconnect_scan_qr":
            return NSLocalizedString(
                "progress.signer.nostrconnect_scan_qr",
                value: "Scan the QR code with your signer app.",
                comment: "Progress: waiting for the signer to scan the NostrConnect QR")
        case "signer_progress_nostrconnect_awaiting_confirmation":
            return NSLocalizedString(
                "progress.signer.nostrconnect_awaiting_confirmation",
                value: "Confirm the connection in your signer app.",
                comment: "Progress: waiting for the user to confirm in the signer app")
        default:
            return nil
        }
    }
}

/// Maps a kernel `action_lifecycle` `reason_code` (#1735) to localized
/// failure copy — the parallel of `UiErrorProse` / `UiProgressProse` for the
/// `LifecycleStage.failed` reason. The kernel ships a stable `reason_code` only
/// for its OWN curated copy; opaque upstream / diagnostic text stays prose-only
/// (`reason_code` absent), so the caller falls back to the English `reason`
/// string the wire always carries. Returns `nil` for an unrecognized key.
///
/// `subject` is the optional contextual value the kernel attaches
/// (`reason_subject`) for interpolation — none of the current codes use it, but
/// the signature carries it so a future subject-bearing code lands without a
/// surface change.
enum UiLifecycleReasonProse {
    static func localized(code: String, subject: String?) -> String? {
        switch code {
        case "lifecycle_no_active_account":
            return NSLocalizedString(
                "lifecycle.reason.no_active_account",
                value: "Sign in to an account first.",
                comment: "Action failed: no account is signed in")
        case "lifecycle_publish_no_explicit_target":
            return NSLocalizedString(
                "lifecycle.reason.publish_no_explicit_target",
                value: "This private note needs an explicit relay to publish to.",
                comment: "Action failed: a private/encrypted publish had no explicit relay pin")
        default:
            return nil
        }
    }
}
