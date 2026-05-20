import Foundation
import SwiftUI
import os.log

// ─────────────────────────────────────────────────────────────────────────
// Marmot (MLS encrypted groups) FFI bridge.
//
// Mirrors `Bridge/ModularTimelineBridge.swift`: a thin extension on
// `KernelHandle` that owns the lifetime of the opaque
// `nmp_app_chirp_marmot_register` handle, plus an `@Observable`-style
// `ObservableObject` (`MarmotStore`) that refreshes `…_marmot_snapshot` when
// the kernel pushes snapshots and wraps each `…_marmot_dispatch` op.
//
// Conventions matched verbatim from the modular-timeline bridge:
//   • C symbols declared in `Bridge/NmpCore.h` (the project's bridging
//     header — same place `nmp_app_chirp_*` live).
//   • `String(cString:)` decode + free EVERY returned pointer via
//     `nmp_app_chirp_marmot_string_free`.
//   • D6 resilience: any nil pointer / decode failure → empty state, never
//     a crash or throw across the bridge.
//
// ── Relay seam status (2026-05-19) ────────────────────────────────────────
//
// Both relay seams are NOW CLOSED at the Rust layer:
//
//   Outbound: `dispatch` ops publish signed events INTERNALLY via
//   `nmp_app_publish_signed_event*` kernel capabilities — no Swift relay
//   path needed. The op result still carries the signed event JSON but
//   it is INFORMATIONAL only.
//
//   Inbound: the kernel exposes a `RawEventObserver` tap registered for
//   kinds [443, 444, 445, 1059, 30443]. Every accepted inbound signed
//   event of those kinds is automatically processed by the Rust layer
//   (welcomes / messages / key packages surface in the next snapshot).
//
// ── Key-package fetch ─────────────────────────────────────────────────────
//
// Before inviting a peer, their signed kind:30443 KeyPackage event must
// be fetched from relays and cached locally (MDK requires it for MLS
// group creation). Use `nmp_app_chirp_marmot_fetch_key_packages` to
// register kernel relay interests for specific pubkeys; observe
// `snapshot.cachedKpPubkeys` to know when they're available.
//
// ── Remaining limitation ──────────────────────────────────────────────────
//
// Bunker/NIP-46 sign-in never surfaces the secret key to Swift, so
// Marmot stays in the empty state for NIP-46 users. NSec sign-in works.
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
    let cachedKpPubkeys: [String]

    enum CodingKeys: String, CodingKey {
        case groups
        case pendingWelcomes = "pending_welcomes"
        case keyPackage = "key_package"
        case cachedKpPubkeys = "cached_kp_pubkeys"
    }

    static let empty = MarmotSnapshot(groups: [], pendingWelcomes: [], keyPackage: .empty, cachedKpPubkeys: [])
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
    let errors: [String]?

    enum CodingKeys: String, CodingKey {
        case ok, error, needs, errors
    }

    static let bridgeUnavailable = MarmotOpResult.failure("marmot bridge unavailable")

    static func failure(_ message: String) -> MarmotOpResult {
        MarmotOpResult(ok: false, error: message, needs: nil,
                       errors: nil)
    }
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

    /// Register a Marmot projection using the actor-owned active key.
    /// Swift never sees the nsec — it stays in Rust. Call from `apply()`
    /// after `createAccount` succeeds. Idempotent: drops a prior handle first.
    @discardableResult
    func registerMarmotActive(appSupportDir: String) -> Bool {
        unregisterMarmotIfNeeded()
        let handle: UnsafeMutableRawPointer? = appSupportDir.withCString { dirPtr in
            nmp_app_chirp_marmot_register_active(raw, dirPtr)
        }
        if let handle {
            marmotHandle = handle
            return true
        }
        mbLog.error("nmp_app_chirp_marmot_register_active returned NULL — no active local key?")
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
            return .failure("dispatch returned null")
        }
        defer { nmp_app_chirp_marmot_string_free(ptr) }
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

    /// Register kernel interests for kind:30443/443 KeyPackage events for the
    /// given pubkeys. Fire-and-forget; results arrive asynchronously via the
    /// Marmot tap.
    func fetchKeyPackagesForPeers(npubs: [String]) {
        guard let handle = marmotHandle else { return }
        guard let data = try? JSONSerialization.data(withJSONObject: npubs),
              let json = String(data: data, encoding: .utf8) else { return }
        json.withCString { ptr in
            nmp_app_chirp_marmot_fetch_key_packages(handle, ptr)
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

    private unowned let kernel: KernelHandle
    private let relayURLsProvider: () -> [String]

    init(kernel: KernelHandle, relayURLsProvider: @escaping () -> [String]) {
        self.kernel = kernel
        self.relayURLsProvider = relayURLsProvider
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

    /// Register using the Rust-side active key (no nsec needed from Swift).
    /// Called from `KernelModel.apply()` after `createAccount` — the actor
    /// writes the key to its slot before emitting the snapshot, so by the
    /// time this runs the slot is guaranteed to be populated. Idempotent.
    func registerActive() {
        guard !isRegistered else { return }
        guard let dir = Self.appSupportDir() else {
            mbLog.error("application-support dir unavailable — Marmot registerActive skipped")
            return
        }
        let ok = kernel.registerMarmotActive(appSupportDir: dir)
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
            return .failure("could not encode action")
        }
        let result = kernel.marmotDispatch(actionJSON: json)
        refresh()
        return result
    }

    @discardableResult
    func publishKeyPackage() -> MarmotOpResult {
        dispatch(["op": "publish_key_package"])
    }

    /// Trigger key-package fetch for the given npubs from relays. Fire-and-
    /// forget; the kernel pushes matching signed events through the Marmot tap.
    func fetchKeyPackages(npubs: [String]) {
        kernel.fetchKeyPackagesForPeers(npubs: npubs)
    }

    /// True if all of the given npubs have a cached key package locally.
    func hasKeyPackages(for npubs: [String]) -> Bool {
        let cached = Set(snapshot.cachedKpPubkeys)
        return npubs.allSatisfy { cached.contains($0) }
    }

    @discardableResult
    func createGroup(name: String, description: String, inviteeNpubs: [String]) -> MarmotOpResult {
        if !inviteeNpubs.isEmpty {
            fetchKeyPackages(npubs: inviteeNpubs)
        }
        return dispatch([
            "op": "create_group",
            "name": name,
            "description": description,
            "invitee_npubs": inviteeNpubs,
            "signed_key_package_events_json": [String](),
        ])
    }

    @discardableResult
    func invite(groupIDHex: String, inviteeNpubs: [String]) -> MarmotOpResult {
        // Trigger a background fetch of invitees' key packages (if not cached).
        // The dispatch op uses the kp_cache; if still missing it returns
        // key_package_unavailable with `needs` populated.
        if !inviteeNpubs.isEmpty {
            fetchKeyPackages(npubs: inviteeNpubs)
        }
        return dispatch([
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
