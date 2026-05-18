import Foundation
import SwiftUI
import os.log

// ─────────────────────────────────────────────────────────────────────────
// Marmot (MLS encrypted groups) FFI bridge.
//
// Mirrors `Bridge/ModularTimelineBridge.swift`: a thin extension on
// `KernelHandle` that owns the lifetime of the opaque
// `nmp_app_chirp_marmot_register` handle, plus an `@Observable`-style
// `ObservableObject` (`MarmotStore`) that polls `…_marmot_snapshot` on the
// existing kernel tick cadence and wraps each `…_marmot_dispatch` op.
//
// Conventions matched verbatim from the modular-timeline bridge:
//   • C symbols declared in `Bridge/NmpCore.h` (the project's bridging
//     header — same place `nmp_app_chirp_*` live).
//   • `String(cString:)` decode + free EVERY returned pointer via
//     `nmp_app_chirp_marmot_string_free`.
//   • D6 resilience: any nil pointer / decode failure → empty state, never
//     a crash or throw across the bridge.
//
// ── KNOWN LIMITATION: signed-event ingest + relay publish seam ───────────
//
// The Marmot FFI is split-brain by design (ADR-0009 kernel boundary):
//
//   1. `dispatch` ops that produce events to publish (`publish_key_package`,
//      `create_group`, `invite`, `send`, `leave`, `remove`,
//      `accept_welcome`) return the ready-to-publish *signed* event JSON in
//      their result (`events` / `welcome_rumors` / `gift_wraps` / `event`).
//      The Swift relay layer is expected to publish them.
//   2. `ingest_signed_event` MUST be called for every relay-received
//      kind:1059 (gift-wrap) and kind:445 (group message / commit) or
//      welcomes & messages never surface in the snapshot.
//
// Chirp's kernel does NOT expose a raw signed-event stream to Swift (the
// snapshot delivers *projected* `TimelineItem`s, not raw signed events),
// and there is no Swift-side "publish this signed event JSON" hook (only
// `nmp_app_publish_unsigned_event`, which signs kernel-side and is the
// wrong shape for already-signed MLS gift-wraps/commits).
//
// Consequence: group ops land in the local MDK SQLite state and the UI
// exercises end-to-end against it, but produced events do NOT reach relays
// and inbound 1059/445 are NOT ingested from this Chirp shell. The
// milestone's E2E proof is the headless Rust tests; this Chirp surface is
// additive. `MarmotStore.ingestSignedEvent(_:)` is implemented and ready
// for the day a raw-signed-event seam exists — it simply has no caller in
// the current Chirp kernel surface.
// ─────────────────────────────────────────────────────────────────────────

private let mbLog = Logger(subsystem: "com.example.Chirp", category: "MarmotBridge")

// ── Decoded snapshot DTOs (verbatim FFI schema) ──────────────────────────

struct MarmotGroup: Decodable, Identifiable, Equatable {
    let idHex: String
    let name: String
    let members: [String]
    let unread: UInt64
    let lastMsgAt: UInt64?

    var id: String { idHex }

    enum CodingKeys: String, CodingKey {
        case idHex = "id_hex"
        case name
        case members
        case unread
        case lastMsgAt = "last_msg_at"
    }
}

struct MarmotPendingWelcome: Decodable, Identifiable, Equatable {
    let idHex: String
    let groupName: String
    let inviterNpub: String

    var id: String { idHex }

    enum CodingKeys: String, CodingKey {
        case idHex = "id_hex"
        case groupName = "group_name"
        case inviterNpub = "inviter_npub"
    }
}

struct MarmotKeyPackage: Decodable, Equatable {
    let published: Bool
    let dTag: String?
    let ageSecs: UInt64?
    let stale: Bool

    enum CodingKeys: String, CodingKey {
        case published
        case dTag = "d_tag"
        case ageSecs = "age_secs"
        case stale
    }

    static let empty = MarmotKeyPackage(published: false, dTag: nil, ageSecs: nil, stale: false)
}

struct MarmotSnapshot: Decodable, Equatable {
    let groups: [MarmotGroup]
    let pendingWelcomes: [MarmotPendingWelcome]
    let keyPackage: MarmotKeyPackage

    enum CodingKeys: String, CodingKey {
        case groups
        case pendingWelcomes = "pending_welcomes"
        case keyPackage = "key_package"
    }

    static let empty = MarmotSnapshot(groups: [], pendingWelcomes: [], keyPackage: .empty)
}

struct MarmotMessage: Decodable, Identifiable, Equatable {
    let id: String
    let senderNpub: String
    let content: String
    let createdAt: UInt64
    let epoch: UInt64?

    enum CodingKeys: String, CodingKey {
        case id
        case senderNpub = "sender_npub"
        case content
        case createdAt = "created_at"
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

    static let bridgeUnavailable = MarmotOpResult(
        ok: false, error: "marmot bridge unavailable", needs: nil)
}

// ── KernelHandle Marmot extension (C-FFI lifetime owner) ──────────────────

extension KernelHandle {
    /// Register a Marmot projection. `secretKey` is hex OR `nsec…`; the
    /// encrypted MLS SQLite DB is created at
    /// `<appSupportDir>/marmot-mls-state.sqlite`. Idempotent: a prior
    /// handle is dropped first. Returns `true` on success.
    @discardableResult
    func registerMarmot(secretKey: String, appSupportDir: String) -> Bool {
        unregisterMarmotIfNeeded()
        let handle: UnsafeMutableRawPointer? = secretKey.withCString { skPtr in
            appSupportDir.withCString { dirPtr in
                nmp_app_chirp_marmot_register(raw, skPtr, dirPtr)
            }
        }
        if let handle {
            marmotHandle = handle
            return true
        }
        mbLog.error("nmp_app_chirp_marmot_register returned NULL — Marmot unavailable")
        return false
    }

    /// Drop the Marmot observer registration if one exists. Idempotent.
    /// MUST run before `nmp_app_free` (FFI contract).
    func unregisterMarmotIfNeeded() {
        if let handle = marmotHandle {
            nmp_app_chirp_marmot_unregister(handle)
            marmotHandle = nil
        }
    }

    /// Decode the current Marmot snapshot. `.empty` on any failure (D6).
    func marmotSnapshot() -> MarmotSnapshot {
        guard let handle = marmotHandle else { return .empty }
        guard let ptr = nmp_app_chirp_marmot_snapshot(handle) else { return .empty }
        defer { nmp_app_chirp_marmot_string_free(ptr) }
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
            nmp_app_chirp_marmot_group_messages(handle, $0)
        }
        guard let ptr else { return [] }
        defer { nmp_app_chirp_marmot_string_free(ptr) }
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
            nmp_app_chirp_marmot_dispatch(handle, $0)
        }
        guard let ptr else {
            return MarmotOpResult(ok: false, error: "dispatch returned null", needs: nil)
        }
        defer { nmp_app_chirp_marmot_string_free(ptr) }
        let payload = String(cString: ptr)
        guard let data = payload.data(using: .utf8) else {
            return MarmotOpResult(ok: false, error: "dispatch payload not utf8", needs: nil)
        }
        do {
            return try JSONDecoder().decode(MarmotOpResult.self, from: data)
        } catch {
            mbLog.error("marmotDispatch decode failed: \(error.localizedDescription) — payload: \(payload.prefix(200))")
            return MarmotOpResult(ok: false, error: "undecodable dispatch result", needs: nil)
        }
    }
}

// ── MarmotStore — @Published projection on the kernel tick ────────────────

/// Observable mirror of the Marmot snapshot, refreshed every kernel tick
/// (`KernelModel.apply` calls `refresh()` in the same pass it refreshes the
/// modular timeline — one extra JSON round-trip per snapshot, reads are
/// O(groups + welcomes)). Registration happens lazily once a secret key is
/// known (sign-in via nsec); bunker/NIP-46 sign-in never surfaces a secret
/// to Swift so Marmot stays in the empty state then (documented limitation).
@MainActor
final class MarmotStore: ObservableObject {
    @Published private(set) var snapshot: MarmotSnapshot = .empty
    @Published private(set) var isRegistered = false

    /// Default Marmot relays. The dispatch ops take a `relays` array; Chirp
    /// has no per-feature relay config surface so we use a sane default set
    /// (same well-known relays the rest of the app reaches).
    let defaultRelays = ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.primal.net"]

    private unowned let kernel: KernelHandle

    init(kernel: KernelHandle) {
        self.kernel = kernel
    }

    var groups: [MarmotGroup] { snapshot.groups }
    var pendingWelcomes: [MarmotPendingWelcome] { snapshot.pendingWelcomes }
    var keyPackage: MarmotKeyPackage { snapshot.keyPackage }

    /// App-support directory (created if missing). The Marmot DB lives at
    /// `<dir>/marmot-mls-state.sqlite` (owned by the Rust crate).
    private static func appSupportDir() -> String? {
        let fm = FileManager.default
        guard let url = fm.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
        else { return nil }
        if !fm.fileExists(atPath: url.path) {
            try? fm.createDirectory(at: url, withIntermediateDirectories: true)
        }
        return url.path
    }

    /// Register the projection with a known secret key. Idempotent / safe to
    /// call repeatedly (the bridge drops a prior handle first). No-op if the
    /// app-support dir can't be resolved.
    func registerIfNeeded(secretKey: String) {
        guard !isRegistered else { return }
        guard let dir = Self.appSupportDir() else {
            mbLog.error("application-support dir unavailable — Marmot register skipped")
            return
        }
        let ok = kernel.registerMarmot(secretKey: secretKey, appSupportDir: dir)
        isRegistered = ok
        if ok { refresh() }
    }

    /// Pull the latest snapshot. Called from `KernelModel.apply` each tick.
    func refresh() {
        guard isRegistered else { return }
        let next = kernel.marmotSnapshot()
        if next != snapshot { snapshot = next }
    }

    func messages(groupIDHex: String) -> [MarmotMessage] {
        kernel.marmotGroupMessages(groupIDHex: groupIDHex)
    }

    // ── Dispatch op wrappers ──────────────────────────────────────────────
    // Each encodes the op envelope, dispatches, refreshes the snapshot, and
    // returns the decoded result so the caller can surface errors.

    @discardableResult
    private func dispatch(_ action: [String: Any]) -> MarmotOpResult {
        guard let data = try? JSONSerialization.data(withJSONObject: action),
              let json = String(data: data, encoding: .utf8)
        else {
            return MarmotOpResult(ok: false, error: "could not encode action", needs: nil)
        }
        let result = kernel.marmotDispatch(actionJSON: json)
        refresh()
        return result
    }

    @discardableResult
    func publishKeyPackage() -> MarmotOpResult {
        dispatch(["op": "publish_key_package", "relays": defaultRelays])
    }

    @discardableResult
    func createGroup(name: String, description: String, inviteeNpubs: [String]) -> MarmotOpResult {
        dispatch([
            "op": "create_group",
            "name": name,
            "description": description,
            "relays": defaultRelays,
            "invitee_npubs": inviteeNpubs,
            "signed_key_package_events_json": [String](),
        ])
    }

    @discardableResult
    func invite(groupIDHex: String, inviteeNpubs: [String]) -> MarmotOpResult {
        dispatch([
            "op": "invite",
            "group_id_hex": groupIDHex,
            "invitee_npubs": inviteeNpubs,
            "signed_key_package_events_json": [String](),
        ])
    }

    @discardableResult
    func send(groupIDHex: String, text: String) -> MarmotOpResult {
        dispatch(["op": "send", "group_id_hex": groupIDHex, "text": text])
    }

    @discardableResult
    func leave(groupIDHex: String) -> MarmotOpResult {
        dispatch(["op": "leave", "group_id_hex": groupIDHex])
    }

    @discardableResult
    func remove(groupIDHex: String, memberNpubs: [String]) -> MarmotOpResult {
        dispatch(["op": "remove", "group_id_hex": groupIDHex, "member_npubs": memberNpubs])
    }

    @discardableResult
    func acceptWelcome(welcomeIDHex: String) -> MarmotOpResult {
        dispatch(["op": "accept_welcome", "welcome_id_hex": welcomeIDHex])
    }

    @discardableResult
    func declineWelcome(welcomeIDHex: String) -> MarmotOpResult {
        dispatch(["op": "decline_welcome", "welcome_id_hex": welcomeIDHex])
    }

    /// Ingest a relay-received signed kind:1059 / kind:445 event. Wired and
    /// ready, but has NO caller in the current Chirp kernel surface — Chirp
    /// does not expose a raw signed-event stream to Swift. See the header
    /// limitation. Present so a future seam can plug in without bridge work.
    @discardableResult
    func ingestSignedEvent(_ eventJSON: String) -> MarmotOpResult {
        dispatch(["op": "ingest_signed_event", "event_json": eventJSON])
    }

    /// Publish-failure recovery: clear a group's pending MDK commit.
    @discardableResult
    func clearPending(groupIDHex: String) -> MarmotOpResult {
        dispatch(["op": "clear_pending", "group_id_hex": groupIDHex])
    }
}
