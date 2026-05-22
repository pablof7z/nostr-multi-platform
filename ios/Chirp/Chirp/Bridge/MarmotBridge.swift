import Foundation
import SwiftUI
import os.log

// ─────────────────────────────────────────────────────────────────────────
// Marmot (MLS encrypted groups) FFI bridge.
//
// Mirrors `Bridge/ModularTimelineBridge.swift`: a thin extension on
// `KernelHandle` that owns the lifetime of the opaque
// `nmp_marmot_register` handle, plus an `@Observable`-style
// `ObservableObject` (`MarmotStore`) that receives snapshots from
// `KernelModel.apply` and wraps each `…_marmot_dispatch` user intent.
//
// Conventions matched verbatim from the modular-timeline bridge:
//   • C symbols declared in `Bridge/NmpCore.h` (the project's bridging
//     header — same place `nmp_app_chirp_*` live).
//   • `String(cString:)` decode + free EVERY returned pointer via
//     `nmp_marmot_string_free`.
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
//   kinds [443, 444, 445, 1059, 30443]. Every accepted inbound signed
//   event of those kinds is automatically processed by the Rust layer
//   (welcomes / messages / key packages surface in the next snapshot).
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

// ── Decoded snapshot DTOs (verbatim FFI schema) ──────────────────────────

struct MarmotGroup: Decodable, Identifiable, Equatable {
    let idHex: String
    let name: String
    /// Empty-name fallback already applied by Rust ("Untitled group").
    let displayName: String
    /// 2-char ASCII initials for the avatar tile, Rust-derived.
    let initials: String
    let members: [String]
    /// Pluralised member-count string ("3 members" / "1 member"),
    /// Rust-derived; the UI prepends the lock visual.
    let memberCountDisplay: String
    let unread: UInt64
    /// `Some("3")` when unread > 0, `nil` when no badge should render.
    let unreadDisplay: String?
    let lastMsgAt: UInt64?

    var id: String { idHex }

    enum CodingKeys: String, CodingKey {
        case idHex = "id_hex"
        case name
        case displayName = "display_name"
        case initials
        case members
        case memberCountDisplay = "member_count_display"
        case unread
        case unreadDisplay = "unread_display"
        case lastMsgAt = "last_msg_at"
    }
}

struct MarmotPendingWelcome: Decodable, Identifiable, Equatable {
    let idHex: String
    let groupName: String
    /// Empty-name fallback already applied by Rust ("Group invite").
    let displayName: String
    let inviterNpub: String
    /// Pre-abbreviated bech32 form `npub1abcd…wxyz` (Rust-derived).
    let inviterShort: String

    var id: String { idHex }

    enum CodingKeys: String, CodingKey {
        case idHex = "id_hex"
        case groupName = "group_name"
        case displayName = "display_name"
        case inviterNpub = "inviter_npub"
        case inviterShort = "inviter_short"
    }
}

struct MarmotKeyPackage: Decodable, Equatable {
    let published: Bool
    let dTag: String?
    let ageSecs: UInt64?
    let stale: Bool
    /// Pre-formatted bucketed age ("12s old" / "7m old" / …) — Rust owns the
    /// §6/AP1 string so the iOS shell never re-derives it.
    let ageDisplay: String?
    /// Pre-formatted row subtitle. Encodes the four-branch policy
    /// (registered? · published? · age · stale) so the shell renders one
    /// string verbatim.
    let subtitle: String
    /// Button label ("Publish key package" / "Rotate key package") — the
    /// kernel picks the verb off `published` to keep the §4.4 ternary out of
    /// the shell.
    let actionLabel: String

    enum CodingKeys: String, CodingKey {
        case published
        case dTag = "d_tag"
        case ageSecs = "age_secs"
        case stale
        case ageDisplay = "age_display"
        case subtitle
        case actionLabel = "action_label"
    }

    static let empty = MarmotKeyPackage(
        published: false,
        dTag: nil,
        ageSecs: nil,
        stale: false,
        ageDisplay: nil,
        subtitle: "",
        actionLabel: ""
    )
}

struct MarmotSnapshot: Decodable, Equatable {
    let groups: [MarmotGroup]
    let pendingWelcomes: [MarmotPendingWelcome]
    let keyPackage: MarmotKeyPackage
    let cachedKpPubkeys: [String]
    /// Pluralised label for the top-of-list invites chip
    /// (`"1 invite"` / `"3 invites"`), or `nil` when no pending invites.
    let invitesChipLabel: String?
    /// `true` when this snapshot came from a registered Marmot signing
    /// identity. `false` for the `.empty` fallback the Swift shell uses when
    /// no `MarmotHandle` exists. Lets the iOS row read `keyPackage.subtitle`
    /// verbatim — both branches of the registration policy are now Rust-owned.
    let isRegistered: Bool

    enum CodingKeys: String, CodingKey {
        case groups
        case pendingWelcomes = "pending_welcomes"
        case keyPackage = "key_package"
        case cachedKpPubkeys = "cached_kp_pubkeys"
        case invitesChipLabel = "invites_chip_label"
        case isRegistered = "is_registered"
    }

    static let empty = MarmotSnapshot(
        groups: [],
        pendingWelcomes: [],
        keyPackage: .empty,
        cachedKpPubkeys: [],
        invitesChipLabel: nil,
        isRegistered: false
    )
}

struct MarmotMessage: Decodable, Identifiable, Equatable {
    let id: String
    let senderNpub: String
    /// `npub1abcd…wxyz` abbreviation (Rust-derived).
    let senderShort: String
    /// 2-char ASCII initials for the avatar tile (Rust-derived).
    let senderInitials: String
    /// 6-hex deterministic avatar tint (Rust-derived).
    let senderColorHex: String
    let content: String
    let createdAt: UInt64
    /// Relative-time stamp ("3m" / "2h" / "5d"), Rust-formatted against
    /// the snapshot's `now_secs` — the UI renders verbatim.
    let createdAtDisplay: String
    let epoch: UInt64?

    enum CodingKeys: String, CodingKey {
        case id
        case senderNpub = "sender_npub"
        case senderShort = "sender_short"
        case senderInitials = "sender_initials"
        case senderColorHex = "sender_color_hex"
        case content
        case createdAt = "created_at"
        case createdAtDisplay = "created_at_display"
        case epoch
    }
}

/// Decoded `{"ok":Bool,…}` envelope every dispatch op returns. The op-
/// specific fields (`group_id_hex`, `d_tag`, `events`, …) are intentionally
/// not modeled — the Chirp UI only needs the success flag + error string;
/// the signed events those ops emit cannot be published from this shell
/// (see header limitation), so decoding them would be dead weight.
struct MarmotOpResult: Decodable, Equatable {
    let ok: Bool
    let error: String?
    let needs: [String]?
    /// Rust-derived abbreviated npubs paired 1:1 with `needs`. The UI
    /// joins these directly into its error string — no `shortNpub` helper
    /// in Swift.
    let needsDisplay: [String]?
    let errors: [String]?
    let fetchRequested: Int?

    enum CodingKeys: String, CodingKey {
        case ok, error, needs, errors
        case needsDisplay = "needs_display"
        case fetchRequested = "fetch_requested"
    }

    static let bridgeUnavailable = MarmotOpResult.failure("marmot bridge unavailable")

    static func failure(_ message: String) -> MarmotOpResult {
        MarmotOpResult(ok: false, error: message, needs: nil,
                       needsDisplay: nil, errors: nil, fetchRequested: nil)
    }
}

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
            nmp_marmot_register_active(raw, dirPtr)
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

    /// Decode the current Marmot snapshot. `.empty` on any failure (D6).
    func marmotSnapshot() -> MarmotSnapshot {
        guard let handle = marmotHandle else { return .empty }
        guard let ptr = nmp_marmot_snapshot(handle) else { return .empty }
        defer { nmp_marmot_string_free(ptr) }
        let payload = String(cString: ptr)
        guard let data = payload.data(using: .utf8) else { return .empty }
        do {
            return try JSONDecoder().decode(MarmotSnapshot.self, from: data)
        } catch {
            mbLog.error("marmotSnapshot decode failed: \(error.localizedDescription)")
            return .empty
        }
    }

    /// Newest-200 decrypted messages for `groupIDHex`. `[]` on any failure.
    func marmotGroupMessages(groupIDHex: String) -> [MarmotMessage] {
        guard let handle = marmotHandle else { return [] }
        let ptr: UnsafeMutablePointer<CChar>? = groupIDHex.withCString {
            nmp_marmot_group_messages(handle, $0)
        }
        guard let ptr else { return [] }
        defer { nmp_marmot_string_free(ptr) }
        let payload = String(cString: ptr)
        guard let data = payload.data(using: .utf8) else { return [] }
        do {
            return try JSONDecoder().decode([MarmotMessage].self, from: data)
        } catch {
            mbLog.error("marmotGroupMessages decode failed: \(error.localizedDescription)")
            return []
        }
    }

    /// Perform one mutating op. `actionJSON` is the op envelope. Returns the
    /// decoded `{"ok":…}` result; `.bridgeUnavailable` if the handle is
    /// unset, `{ok:false}` on a serialize / decode failure (D6 — never
    /// throws across the bridge).
    func marmotDispatch(actionJSON: String) -> MarmotOpResult {
        guard let handle = marmotHandle else { return .bridgeUnavailable }
        let ptr: UnsafeMutablePointer<CChar>? = actionJSON.withCString {
            nmp_marmot_dispatch(handle, $0)
        }
        guard let ptr else {
            return .failure("dispatch returned null")
        }
        defer { nmp_marmot_string_free(ptr) }
        let payload = String(cString: ptr)
        guard let data = payload.data(using: .utf8) else {
            return .failure("dispatch payload not utf8")
        }
        do {
            return try JSONDecoder().decode(MarmotOpResult.self, from: data)
        } catch {
            mbLog.error("marmotDispatch decode failed: \(error.localizedDescription) — payload: \(payload.prefix(400))")
            return .failure("undecodable dispatch result")
        }
    }

}

// ── MarmotStore — projection mirror pushed by KernelModel.apply ───────────

@MainActor
final class MarmotStore: ObservableObject {
    @Published private(set) var snapshot: MarmotSnapshot = .empty
    @Published private(set) var isRegistered = false

    private unowned let kernel: KernelHandle

    init(kernel: KernelHandle) {
        self.kernel = kernel
    }

    var groups: [MarmotGroup] { snapshot.groups }
    var pendingWelcomes: [MarmotPendingWelcome] { snapshot.pendingWelcomes }
    var keyPackage: MarmotKeyPackage { snapshot.keyPackage }
    /// Pre-formatted label for the top-of-list invites chip
    /// (Rust-owned plural form), or `nil` when no pending invites.
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

    func apply(snapshot next: MarmotSnapshot, isRegistered registered: Bool) {
        isRegistered = registered
        if next != snapshot {
            snapshot = next
            // Rebuild the id-keyed lookup on each apply. O(n) once per
            // snapshot tick beats `.first(where:)` per render.
            var byID: [String: MarmotGroup] = [:]
            byID.reserveCapacity(next.groups.count)
            for g in next.groups { byID[g.idHex] = g }
            groupsByID = byID
        }
    }

    func messages(groupIDHex: String) -> [MarmotMessage] {
        kernel.marmotGroupMessages(groupIDHex: groupIDHex)
    }

    // ── Dispatch op wrappers ──────────────────────────────────────────────
    // Each encodes the op envelope and dispatches. The next kernel snapshot
    // pushes the refreshed Marmot view; the UI does not poll from Swift.
    //
    // `nmp-marmot` dispatch opens a synchronous WebSocket connection with a
    // wall-clock timeout per relay (up to 6 s × N relays) — blocking the
    // main actor is not acceptable. All wrappers move the blocking FFI call
    // off the main actor via `DispatchQueue.global().async`.
    //
    // `KernelHandle` is not `Sendable` so `Task.detached` cannot capture it;
    // `DispatchQueue.global().async` has no such constraint.
    //
    // Two call-site contracts:
    // • Fire-and-forget (Void return): the outcome arrives as a refreshed
    //   snapshot on the next kernel tick; callers need no result.
    // • Result-dependent (async → MarmotOpResult): callers await inside
    //   `Task { }`, which keeps the continuation on @MainActor for safe
    //   @State mutation after the await.

    /// Encode the op envelope and dispatch it on the global concurrent queue,
    /// bridging back to the caller via `CheckedContinuation`. JSON encoding is
    /// done on the calling actor (cheap); only the blocking FFI call crosses to
    /// the background thread. Never throws across the bridge (D6).
    private func dispatchAsync(_ action: [String: Any]) async -> MarmotOpResult {
        guard let data = try? JSONSerialization.data(withJSONObject: action),
              let json = String(data: data, encoding: .utf8)
        else { return .failure("could not encode action") }
        let handle = kernel.marmotHandle
        return await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                guard let handle else {
                    continuation.resume(returning: .bridgeUnavailable)
                    return
                }
                let ptr: UnsafeMutablePointer<CChar>? = json.withCString {
                    nmp_marmot_dispatch(handle, $0)
                }
                guard let ptr else {
                    continuation.resume(returning: .failure("dispatch returned null"))
                    return
                }
                let payload = String(cString: ptr)
                nmp_marmot_string_free(ptr)
                guard let d = payload.data(using: .utf8),
                      let result = try? JSONDecoder().decode(MarmotOpResult.self, from: d)
                else {
                    continuation.resume(returning: .failure("undecodable dispatch result"))
                    return
                }
                continuation.resume(returning: result)
            }
        }
    }

    /// Encode the op envelope and dispatch fire-and-forget on the global
    /// concurrent queue. The outcome arrives as a refreshed snapshot on the
    /// next kernel tick.
    private func dispatchFireAndForget(_ action: [String: Any]) {
        guard let data = try? JSONSerialization.data(withJSONObject: action),
              let json = String(data: data, encoding: .utf8)
        else { return }
        guard let handle = kernel.marmotHandle else { return }
        DispatchQueue.global(qos: .userInitiated).async {
            let ptr: UnsafeMutablePointer<CChar>? = json.withCString {
                nmp_marmot_dispatch(handle, $0)
            }
            if let ptr { nmp_marmot_string_free(ptr) }
        }
    }

    /// Publish (or rotate) the local MLS key-package.
    ///
    /// Fire-and-forget: the refreshed key-package state arrives via the next
    /// kernel snapshot tick.
    func publishKeyPackage() {
        dispatchFireAndForget(["op": "publish_key_package"])
    }

    /// True if all of the given npubs have a cached key package locally.
    func hasKeyPackages(for npubs: [String]) -> Bool {
        let cached = Set(snapshot.cachedKpPubkeys)
        return npubs.allSatisfy { cached.contains($0) }
    }

    /// Create a new MLS group. `inviteeText` is the raw text the user
    /// typed; Rust tokenises (whitespace / comma / semicolon / newline)
    /// and validates each entry — Swift does no parsing.
    func createGroup(name: String, description: String, inviteeText: String) async -> MarmotOpResult {
        await dispatchAsync([
            "op": "create_group",
            "name": name,
            "description": description,
            "invitee_text": inviteeText,
            "signed_key_package_events_json": [String](),
        ])
    }

    /// Invite peers to an existing MLS group. `inviteeText` is the raw
    /// user-typed list; tokenisation + validation happen Rust-side.
    func invite(groupIDHex: String, inviteeText: String) async -> MarmotOpResult {
        await dispatchAsync([
            "op": "invite",
            "group_id_hex": groupIDHex,
            "invitee_text": inviteeText,
            "signed_key_package_events_json": [String](),
        ])
    }

    func send(groupIDHex: String, text: String) async -> MarmotOpResult {
        await dispatchAsync(["op": "send", "group_id_hex": groupIDHex, "text": text])
    }

    func leave(groupIDHex: String) async -> MarmotOpResult {
        await dispatchAsync(["op": "leave", "group_id_hex": groupIDHex])
    }

    func remove(groupIDHex: String, memberNpubs: [String]) async -> MarmotOpResult {
        await dispatchAsync(["op": "remove", "group_id_hex": groupIDHex, "member_npubs": memberNpubs])
    }

    /// Accept a pending MLS group invite. Fire-and-forget: the welcome
    /// disappears from the next snapshot tick.
    func acceptWelcome(welcomeIDHex: String) {
        dispatchFireAndForget(["op": "accept_welcome", "welcome_id_hex": welcomeIDHex])
    }

    /// Decline a pending MLS group invite. Fire-and-forget: the welcome
    /// disappears from the next snapshot tick.
    func declineWelcome(welcomeIDHex: String) {
        dispatchFireAndForget(["op": "decline_welcome", "welcome_id_hex": welcomeIDHex])
    }

    /// Ingest a relay-received signed kind:1059 / kind:445 event. Wired and
    /// ready, but has NO caller in the current Chirp kernel surface — Chirp
    /// does not expose a raw signed-event stream to Swift. See the header
    /// limitation. Present so a future seam can plug in without bridge work.
    func ingestSignedEvent(_ eventJSON: String) {
        dispatchFireAndForget(["op": "ingest_signed_event", "event_json": eventJSON])
    }

    /// Publish-failure recovery: clear a group's pending MDK commit.
    func clearPending(groupIDHex: String) {
        dispatchFireAndForget(["op": "clear_pending", "group_id_hex": groupIDHex])
    }
}
