import Foundation
import Observation
import os.log

private let gmLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryModel")

/// Pubkey of the demo account whose profile the gallery claims on startup.
/// pablof7z — picked as a known well-populated kind:0 source for live data.
let DEMO_PUBKEY_HEX = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"

/// Stable consumer id for the gallery's profile interest. The kernel
/// refcounts profile claims per `(pubkey, consumer_id)` pair; using one stable
/// id means claim/release stays balanced even across re-renders.
let GALLERY_PROFILE_CONSUMER_ID = "gallery"

/// Snapshot wire-shape pushed through `nmp_app_set_update_callback`. The
/// parallel `nmp-app-gallery` crate is authoritative; this decoder treats
/// every key as optional so a missing `profiles` (the kernel is still
/// fetching kind:0) or missing `accounts` (phase 1, no sign-in surface)
/// degrades to empty rather than failing the whole tick.
struct GallerySnapshot: Decodable, Equatable {
    let running: Bool
    let profiles: [String: ProfileWire]
    let accounts: [AccountWire]

    private enum CodingKeys: String, CodingKey {
        case running, profiles, accounts
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.running = try container.decodeIfPresent(Bool.self, forKey: .running) ?? false
        self.profiles = try container.decodeIfPresent([String: ProfileWire].self, forKey: .profiles) ?? [:]
        self.accounts = try container.decodeIfPresent([AccountWire].self, forKey: .accounts) ?? []
    }

    static let empty = GallerySnapshot(running: false, profiles: [:], accounts: [])

    init(running: Bool, profiles: [String: ProfileWire], accounts: [AccountWire]) {
        self.running = running
        self.profiles = profiles
        self.accounts = accounts
    }
}

/// Optional `{ "t":"snapshot", "v": GallerySnapshot }` outer envelope. The
/// Chirp kernel update channel wraps payloads in this shape; the parallel
/// gallery crate may follow the same convention. We try the direct
/// `GallerySnapshot` decode first and fall back to the envelope.
private struct GalleryEnvelope: Decodable {
    let t: String?
    let v: GallerySnapshot
}

/// Minimal `accounts` row decoder. Phase 1 doesn't render accounts but
/// keeping a typed slot here means phase 2 (sign-in demo) can wire UI
/// without re-writing the model.
struct AccountWire: Decodable, Equatable {
    let pubkey: String
    let active: Bool

    private enum CodingKeys: String, CodingKey {
        case pubkey, active
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.pubkey = try container.decodeIfPresent(String.self, forKey: .pubkey) ?? ""
        self.active = try container.decodeIfPresent(Bool.self, forKey: .active) ?? false
    }

    init(pubkey: String, active: Bool) {
        self.pubkey = pubkey
        self.active = active
    }
}

/// `@Observable` mirror of the gallery snapshot. The kernel pushes JSON
/// updates through `GalleryKernelHandle.listen`; this class decodes them
/// and republishes for SwiftUI consumption.
@MainActor
@Observable
final class GalleryModel {
    private(set) var snapshot: GallerySnapshot = .empty
    private(set) var lastDecodeError: String?
    private let kernel: GalleryKernelHandle

    init() {
        self.kernel = GalleryKernelHandle()
    }

    /// One-shot bootstrap. Wires the push callback, starts the kernel actor,
    /// then claims the demo pubkey's profile so user-* component pages have
    /// real data to render.
    func start() {
        // Wire the push callback BEFORE start so the very first snapshot
        // tick lands in our model. The callback fires from the kernel actor
        // thread; we hop to the main actor before touching `@Observable`
        // state.
        kernel.listen { [weak self] payload in
            Task { @MainActor [weak self] in
                self?.decode(payload: payload)
            }
        }
        kernel.start()
        // Claim the demo profile (pablof7z). The kernel opens the right
        // relay subscriptions and pushes the ProfileWire through the update
        // callback under `snapshot.profiles[DEMO_PUBKEY_HEX]` when kind:0
        // arrives.
        kernel.claimProfile(pubkey: DEMO_PUBKEY_HEX, consumerID: GALLERY_PROFILE_CONSUMER_ID)
    }

    /// Decode a snapshot JSON payload received from the push callback.
    /// Tries the direct `GallerySnapshot` shape first, then the
    /// Chirp-style `{t,v}` envelope. A decode failure logs once and keeps
    /// the previous snapshot intact (soft-fail).
    func decode(payload: String) {
        guard !payload.isEmpty else { return }
        let data = Data(payload.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase

        // Attempt 1: direct payload — `{ running, profiles, accounts }`.
        if let next = try? decoder.decode(GallerySnapshot.self, from: data) {
            self.snapshot = next
            self.lastDecodeError = nil
            return
        }
        // Attempt 2: envelope-wrapped — `{ "t":"snapshot", "v":{…} }`.
        do {
            let envelope = try decoder.decode(GalleryEnvelope.self, from: data)
            self.snapshot = envelope.v
            self.lastDecodeError = nil
        } catch {
            let msg = "GallerySnapshot decode failed: \(error.localizedDescription)"
            gmLog.error("\(msg, privacy: .public)")
            self.lastDecodeError = msg
        }
    }

    /// Convenience accessor for the demo profile. Returns nil while kind:0
    /// is still in flight; the UI should show a `ProgressView` until it lands.
    var demoProfile: ProfileWire? {
        snapshot.profiles[DEMO_PUBKEY_HEX]
    }

    /// Lookup any profile that arrived through the gallery's profiles map.
    func profile(forPubkey pubkey: String) -> ProfileWire? {
        snapshot.profiles[pubkey]
    }

    /// Demo write surface (phase 2). Dispatches a sign-in action without
    /// holding the secret on the Swift side beyond this call.
    func signInDemo(nsec: String) {
        kernel.signInNsec(nsec)
    }
}
