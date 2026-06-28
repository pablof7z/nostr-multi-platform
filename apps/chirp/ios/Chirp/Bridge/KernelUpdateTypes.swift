import Foundation

// Update-frame, typed snapshot envelope, dispatch-result, and create-account DTOs.
// Extracted from `KernelBridge.swift`; same-module Swift files need no import.

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
        // ── nmp-nip57 (Zap) ──────────────────────────────────────────
        case "nip57_zap_no_lnurl":
            return NSLocalizedString(
                "error.nip57.zap_no_lnurl",
                value: "This user has no lightning address.",
                comment: "Toast: recipient has no lightning address for zapping")
        case "nip57_zap_lnurl_resolve_failed", "nip57_zap_fetch_failed", "nip57_zap_failed":
            return NSLocalizedString(
                "error.nip57.zap_failed",
                value: "Zap failed. Please try again.",
                comment: "Toast: zap request failed")
        case "nip57_zap_sign_failed":
            return NSLocalizedString(
                "error.nip57.zap_sign_failed",
                value: "Couldn’t sign the zap request.",
                comment: "Toast: signing the zap request failed")
        case "nip57_zap_no_wallet":
            return NSLocalizedString(
                "error.nip57.zap_no_wallet",
                value: "No wallet connected — add a NWC wallet first.",
                comment: "Toast: attempted to zap with no wallet configured")
        // ── nmp-nip05 (NIP-05 lookup) ────────────────────────────────
        case "nip05_lookup_invalid":
            return NSLocalizedString(
                "error.nip05.lookup_invalid",
                value: "That NIP-05 identifier isn’t valid.",
                comment: "Toast: NIP-05 identifier failed shape validation")
        case "nip05_lookup_failed":
            return NSLocalizedString(
                "error.nip05.lookup_failed",
                value: "NIP-05 lookup failed.",
                comment: "Toast: NIP-05 HTTP lookup returned an error")
        case "nip05_lookup_native_unavailable":
            return NSLocalizedString(
                "error.nip05.lookup_native_unavailable",
                value: "NIP-05 lookup isn’t available in this build.",
                comment: "Toast: NIP-05 native fetcher not compiled")
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
