import Foundation
import SwiftUI
import os.log

// ─────────────────────────────────────────────────────────────────────────
// Marmot (MLS encrypted groups) FFI bridge.
//
// Mirrors `Bridge/ModularTimelineBridge.swift`: a thin extension on
// `KernelHandle` that owns the lifetime of the opaque
// `nmp_marmot_register_active` handle, plus an `@Observable`-style
// `ObservableObject` (`MarmotStore`) that receives snapshots from
// `KernelModel.apply` and wraps each `…_marmot_dispatch` user intent.
//
// Conventions matched verbatim from the modular-timeline bridge:
//   • C symbols declared in `Bridge/NmpCore.h` (the project's bridging
//     header — same place `nmp_app_chirp_*` live).
//   • Group/message state is read from the pushed `nmp.marmot.snapshot` /
//     `nmp.marmot.messages` projections in the snapshot `apply()` path
//     (V-107 / ADR-0039) — the old `nmp_marmot_*` pull symbols are gone.
//   • D6 resilience: any nil pointer / decode failure → empty state, never
//     a crash or throw across the bridge.
//
// ── Relay seam status (2026-05-19) ────────────────────────────────────────
//
// Both relay seams are NOW CLOSED at the Rust layer:
//
//   Outbound: `dispatch` ops publish signed events INTERNALLY via the
//   workspace-internal `NmpApp::publish_signed_explicit` kernel API
//   (PR-F replaced the prior `nmp_app_publish_signed_event*` `extern "C"`
//   detour with a typed Rust call) — no Swift relay path needed. The op
//   result still carries the signed event JSON but it is INFORMATIONAL
//   only.
//
//   Inbound: the kernel exposes a `RawEventObserver` tap registered for
//   kinds [444, 445, 1059, 30443]. Every accepted inbound signed
//   event of those kinds is automatically processed by the Rust layer
//   (welcomes / messages / key packages surface in the next snapshot).
//
// ── M14-1c / #2169 — typed FlatBuffers byte doorway ──────────────────────
//
// MLS write ops are routed through the typed byte doorway via
// `GeneratedActionBuilders.marmotXxx(...)` → `kernel.dispatchBytes(bytes)`.
// The generated builders hand-roll the `MarmotActionPayload` FlatBuffers
// buffer (marmot_action.fbs / NMMA) matching `MarmotAction::encode` in
// `nmp_marmot::wire::action_payload`. The Rust decoder unpacks the union
// arm via `MarmotActionModule::decode_payload` (override added in #2169).
//
// The JSON assembly helpers (`dispatchMarmotAction(bodyJson:)`,
// `dispatchAsync`, `dispatchFireAndForget`) are DELETED — replaced by
// per-method builder calls. No `nmp_app_chirp_dispatch_action_bytes` calls
// with a hand-spelled `"nmp.marmot"` literal remain in production code
// (enforced by `ci/check_native_action_boundary.py`).
//
// ── Key-package fetch ─────────────────────────────────────────────────────
//
// Before inviting a peer, their signed kind:30443 KeyPackage event must be
// fetched from relays and cached locally. Rust owns that lookup policy:
// `create_group` / `invite` dispatches enqueue missing KeyPackage fetches and
// `snapshot.cachedKpPubkeys` updates on subsequent kernel snapshots.
//
// ── Remaining limitation ──────────────────────────────────────────────────
//
// Bunker/NIP-46 sign-in never has a local key, so Rust registration returns
// no Marmot handle for those users. NSec/local account sign-in works.
// ─────────────────────────────────────────────────────────────────────────

private let mbLog = Logger(subsystem: "io.f7z.chirp", category: "MarmotBridge")

/// App-scoped keyring service id for the Marmot MLS DB encryption key.
/// Must match `CHIRP_MARMOT_KEYRING_SERVICE_ID` in `nmp-chirp-config` (D0:
/// Chirp product policy lives here, not in the reusable nmp-marmot crate).
private let chirpMarmotKeyringServiceID = "nmp.chirp.marmot"

// Decoded snapshot DTOs (MarmotGroup / …/ MarmotSnapshot / MarmotOpResult)
// live in MarmotModels.swift (extracted for the 500-LOC file-size cap).


// ── KernelHandle Marmot extension (C-FFI lifetime owner) ──────────────────

extension KernelHandle {
    private static func appSupportDir() -> String? {
        let fm = FileManager.default
        guard let url = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        else { return nil }
        if !fm.fileExists(atPath: url.path) {
            try? fm.createDirectory(at: url, withIntermediateDirectories: true)
        }
        return url.path
    }

    var isMarmotRegistered: Bool { marmotHandle != nil }

    @discardableResult
    func restoreChirpIdentity(testNsec: String?) -> Bool {
        unregisterMarmotIfNeeded()
        let dir = Self.appSupportDir()
        let handle: UnsafeMutableRawPointer?
        if let testNsec {
            handle = testNsec.withCString { testPtr in
                if let dir {
                    return dir.withCString { dirPtr in
                        nmp_app_chirp_identity_restore(raw, dirPtr, testPtr)
                    }
                }
                return nmp_app_chirp_identity_restore(raw, nil, testPtr)
            }
        } else if let dir {
            handle = dir.withCString { dirPtr in
                nmp_app_chirp_identity_restore(raw, dirPtr, nil)
            }
        } else {
            handle = nmp_app_chirp_identity_restore(raw, nil, nil)
        }
        marmotHandle = handle
        return handle != nil
    }

    @discardableResult
    func signInNsecAndRegisterMarmot(_ secret: String) -> Bool {
        unregisterMarmotIfNeeded()
        let dir = Self.appSupportDir()
        let handle: UnsafeMutableRawPointer? = secret.withCString { secretPtr in
            if let dir {
                return dir.withCString { dirPtr in
                    nmp_app_chirp_identity_sign_in_nsec(raw, secretPtr, dirPtr)
                }
            }
            return nmp_app_chirp_identity_sign_in_nsec(raw, secretPtr, nil)
        }
        marmotHandle = handle
        return handle != nil
    }

    func removeAccountAndForgetSecret(identityID: String) {
        unregisterMarmotIfNeeded()
        identityID.withCString { nmp_app_chirp_identity_remove_account(raw, $0) }
    }

    @discardableResult
    func registerActiveMarmotIfAvailable() -> Bool {
        guard marmotHandle == nil, let dir = Self.appSupportDir() else { return false }
        let handle: UnsafeMutableRawPointer? = dir.withCString { dirPtr in
            chirpMarmotKeyringServiceID.withCString { svcPtr in
                nmp_marmot_register_active(raw, dirPtr, svcPtr)
            }
        }
        marmotHandle = handle
        return handle != nil
    }


    /// Drop the Marmot observer registration if one exists. Idempotent.
    /// MUST run before `nmp_app_free` (FFI contract).
    func unregisterMarmotIfNeeded() {
        if let handle = marmotHandle {
            nmp_marmot_unregister(handle)
            marmotHandle = nil
        }
    }
}

// ── MarmotStore — projection mirror pushed by KernelModel.apply ───────────

@MainActor
final class MarmotStore: ObservableObject {
    @Published private(set) var snapshot: MarmotSnapshot = .empty
    @Published private(set) var isRegistered = false

    /// All-group messages map from the `"nmp.marmot.messages"` push projection
    /// (`projections["nmp.marmot.messages"]` on the SnapshotFrame, V-107).
    /// Keyed by group_id_hex → newest-N `MarmotMessage` array. Updated on
    /// every `apply(snapshot:messages:isRegistered:)` call (D8: no polling).
    @Published private(set) var allMessages: [String: [MarmotMessage]] = [:]

    private unowned let kernel: KernelHandle

    init(kernel: KernelHandle) {
        self.kernel = kernel
    }

    var groups: [MarmotGroup] { snapshot.groups }
    var pendingWelcomes: [MarmotPendingWelcome] { snapshot.pendingWelcomes }
    var keyPackage: MarmotKeyPackage { snapshot.keyPackage }
    /// Pluralised label for the top-of-list invites chip, or `nil` when
    /// no pending invites. Shell-computed from `pendingWelcomes.count`
    /// (aim.md §2 — pluralisation is presentation, not protocol data).
    var invitesChipLabel: String? { snapshot.invitesChipLabel }
    /// Pre-built id-to-row lookup for the live snapshot. Indexing a
    /// dictionary by key is render-grade lookup, not derivation — keeps
    /// `.first(where:)` out of the View layer (chirp/AGENTS.md canonical
    /// bad example). Recomputed only on snapshot apply.
    private(set) var groupsByID: [String: MarmotGroup] = [:]

    /// Lookup a group row by hex MLS id; falls back to the value the View
    /// was constructed with when the row has disappeared (e.g. just left).
    func group(idHex: String, fallback: MarmotGroup) -> MarmotGroup {
        groupsByID[idHex] ?? fallback
    }

    /// Apply a push-projection tick. Both snapshot and messages come from
    /// the kernel's `projections["nmp.marmot.snapshot"]` /
    /// `projections["nmp.marmot.messages"]` frame keys (V-107 / ADR-0039).
    /// `nil` arguments mean the kernel has not yet registered the projection
    /// (e.g. signed-out, first tick before Marmot registered) — fall back to
    /// `.empty` / `[:]` without overwriting existing state with a nil.
    func apply(
        snapshot next: MarmotSnapshot?,
        messages nextMessages: [String: [MarmotMessage]]?,
        isRegistered registered: Bool
    ) {
        isRegistered = registered
        let effective = next ?? .empty
        if effective != snapshot {
            snapshot = effective
            // Rebuild the id-keyed lookup on each apply. O(n) once per
            // snapshot tick beats `.first(where:)` per render.
            var byID: [String: MarmotGroup] = [:]
            byID.reserveCapacity(effective.groups.count)
            for g in effective.groups { byID[g.idHex] = g }
            groupsByID = byID
        }
        let effectiveMessages = nextMessages ?? [:]
        if effectiveMessages != allMessages {
            allMessages = effectiveMessages
        }
    }

    /// Newest-N decrypted messages for `groupIDHex`, read from the push
    /// projection stored in `allMessages` (V-107). `[]` when the group is
    /// unknown or the projection has not arrived yet (D6 / D8 — no poll).
    func messages(groupIDHex: String) -> [MarmotMessage] {
        allMessages[groupIDHex] ?? []
    }

    // ── Dispatch op wrappers ──────────────────────────────────────────────
    // Each encodes the typed FlatBuffers payload via `GeneratedActionBuilders`
    // and dispatches it through `kernel.dispatchBytes(_:)` (M14-1c / #2169).
    //
    // `dispatchBytes` is non-blocking — it validates the typed payload,
    // mints a `correlation_id`, enqueues the op for the actor thread, and
    // returns immediately. The actor in turn invokes the registered
    // `MlsOpHandler` and records `Accepted` / `Failed` in `action_stages` for
    // a future snapshot.
    //
    // Two call-site contracts:
    // • Fire-and-forget (Void return): the outcome arrives as a refreshed
    //   snapshot on the next kernel tick; callers need no result.
    // • Result-dependent (async → MarmotOpResult): the `async` is kept on
    //   the signature for source-compat with existing `Task { let r = await
    //   … }` call sites, even though the body is now synchronous.

    // ── fire-and-forget helper ────────────────────────────────────────────

    /// Dispatch typed bytes fire-and-forget. The Marmot handle is the Swift-
    /// side proof of a local signing key; without it the kernel rejects the
    /// dispatch anyway, but the fast-fail preserves the `.bridgeUnavailable`
    /// UX for bunker users.
    private func fireAndForget(_ bytes: [UInt8]) {
        guard kernel.marmotHandle != nil else { return }
        _ = kernel.dispatchBytes(bytes)
    }

    /// Dispatch typed bytes and translate the result to `MarmotOpResult`.
    private func submitAsync(_ bytes: [UInt8]) async -> MarmotOpResult {
        guard kernel.marmotHandle != nil else { return .bridgeUnavailable }
        let result = kernel.dispatchBytes(bytes)
        switch result {
        case .accepted(let correlationId):
            return .submitted(correlationId: correlationId)
        case .failure(let message):
            return .failure(message)
        }
    }

    // ── Publish (or rotate) the local MLS key-package ────────────────────

    /// Publish (or rotate) the local MLS key-package.
    ///
    /// Fire-and-forget: the refreshed key-package state arrives via the next
    /// kernel snapshot tick.
    func publishKeyPackage() {
        let bytes = GeneratedActionBuilders.marmotPublishKeyPackage(
            correlationId: UUID().uuidString
        )
        fireAndForget(bytes)
    }

    /// True if all of the given npubs have a cached key package locally.
    func hasKeyPackages(for npubs: [String]) -> Bool {
        let cached = Set(snapshot.cachedKpPubkeys)
        return npubs.allSatisfy { cached.contains($0) }
    }

    // ── CreateGroup ───────────────────────────────────────────────────────

    /// Create a new MLS group. `inviteeText` is the raw text the user
    /// typed; Rust tokenises (whitespace / comma / semicolon / newline)
    /// and validates each entry — Swift does no parsing.
    func createGroup(name: String, description: String, inviteeText: String) async -> MarmotOpResult {
        let bytes = GeneratedActionBuilders.marmotCreateGroup(
            correlationId: UUID().uuidString,
            name: name,
            description: description,
            inviteeText: inviteeText
        )
        return await submitAsync(bytes)
    }

    // ── Invite ────────────────────────────────────────────────────────────

    /// Invite peers to an existing MLS group. `inviteeText` is the raw
    /// user-typed list; tokenisation + validation happen Rust-side.
    func invite(groupIDHex: String, inviteeText: String) async -> MarmotOpResult {
        let bytes = GeneratedActionBuilders.marmotInvite(
            correlationId: UUID().uuidString,
            groupIdHex: groupIDHex,
            inviteeText: inviteeText
        )
        return await submitAsync(bytes)
    }

    // ── Send ──────────────────────────────────────────────────────────────

    func send(groupIDHex: String, text: String) async -> MarmotOpResult {
        let bytes = GeneratedActionBuilders.marmotSend(
            correlationId: UUID().uuidString,
            groupIdHex: groupIDHex,
            text: text
        )
        return await submitAsync(bytes)
    }

    // ── Leave ─────────────────────────────────────────────────────────────

    func leave(groupIDHex: String) async -> MarmotOpResult {
        let bytes = GeneratedActionBuilders.marmotLeave(
            correlationId: UUID().uuidString,
            groupIdHex: groupIDHex
        )
        return await submitAsync(bytes)
    }

    // ── Remove ────────────────────────────────────────────────────────────

    func remove(groupIDHex: String, memberNpubs: [String]) async -> MarmotOpResult {
        let bytes = GeneratedActionBuilders.marmotRemove(
            correlationId: UUID().uuidString,
            groupIdHex: groupIDHex,
            memberNpubs: memberNpubs
        )
        return await submitAsync(bytes)
    }

    // ── AcceptWelcome ─────────────────────────────────────────────────────

    /// Accept a pending MLS group invite. Fire-and-forget: the welcome
    /// disappears from the next snapshot tick.
    func acceptWelcome(welcomeIDHex: String) {
        let bytes = GeneratedActionBuilders.marmotAcceptWelcome(
            correlationId: UUID().uuidString,
            welcomeIdHex: welcomeIDHex
        )
        fireAndForget(bytes)
    }

    // ── DeclineWelcome ────────────────────────────────────────────────────

    /// Decline a pending MLS group invite. Fire-and-forget: the welcome
    /// disappears from the next snapshot tick.
    func declineWelcome(welcomeIDHex: String) {
        let bytes = GeneratedActionBuilders.marmotDeclineWelcome(
            correlationId: UUID().uuidString,
            welcomeIdHex: welcomeIDHex
        )
        fireAndForget(bytes)
    }

    // ── ClearPending ──────────────────────────────────────────────────────

    /// Publish-failure recovery: clear a group's pending MDK commit.
    func clearPending(groupIDHex: String) {
        let bytes = GeneratedActionBuilders.marmotClearPending(
            correlationId: UUID().uuidString,
            groupIdHex: groupIDHex
        )
        fireAndForget(bytes)
    }
}
