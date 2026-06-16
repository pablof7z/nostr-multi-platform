import Foundation
import Observation
import os.log

private let gmLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryModel")

/// Shared real Nostr references for every NmpGallery host.
///
/// The source of truth is `apps/nmp-gallery/showcase-references.json`, embedded
/// by `nmp-app-gallery` and exposed here through
/// `nmp_app_gallery_showcase_references_json`. Swift does not duplicate these
/// pubkeys, URIs, event ids, or relay roles.
struct GalleryShowcaseReferences: Decodable, Sendable {
    let schema: String
    let profile: GalleryShowcaseProfile
    let article: GalleryShowcaseEvent
    let note: GalleryShowcaseEvent
    let highlight: GalleryShowcaseEvent
    let relays: [GalleryShowcaseRelay]

    static func loadFromRust() -> GalleryShowcaseReferences {
        guard let ptr = nmp_app_gallery_showcase_references_json() else {
            fatalError("nmp_app_gallery_showcase_references_json returned null")
        }
        let json = String(cString: ptr)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            return try decoder.decode(GalleryShowcaseReferences.self, from: Data(json.utf8))
        } catch {
            fatalError("failed to decode gallery showcase references: \(error)")
        }
    }
}

struct GalleryShowcaseProfile: Decodable, Sendable {
    let pubkeyHex: String
    let npub: String
    let npubShort: String
}

struct GalleryShowcaseEvent: Decodable, Sendable {
    let uri: String
    let primaryId: String
    let kind: UInt32
    let label: String
    let expectedTitle: String?
}

struct GalleryShowcaseRelay: Decodable, Sendable {
    let url: String
    let role: String
}

/// One entry of the kernel's `relay_role_options` typed sidecar.
struct GalleryRelayRoleOption: Decodable, Equatable, Sendable {
    let value: String
    let label: String
    let tint: String
    let isDefault: Bool
}

let GALLERY_SHOWCASE = GalleryShowcaseReferences.loadFromRust()
let SHOWCASE_PUBKEY_HEX = GALLERY_SHOWCASE.profile.pubkeyHex
let SHOWCASE_NPUB = GALLERY_SHOWCASE.profile.npub
let SHOWCASE_NPUB_SHORT = GALLERY_SHOWCASE.profile.npubShort
let SHOWCASE_ARTICLE_NADDR = GALLERY_SHOWCASE.article.uri
let SHOWCASE_ARTICLE_PRIMARY_ID = GALLERY_SHOWCASE.article.primaryId
let SHOWCASE_NOTE_EVENT_ID = GALLERY_SHOWCASE.note.primaryId
let SHOWCASE_NOTE_NEVENT = GALLERY_SHOWCASE.note.uri
let SHOWCASE_HIGHLIGHT_EVENT_ID = GALLERY_SHOWCASE.highlight.primaryId
let SHOWCASE_HIGHLIGHT_NEVENT = GALLERY_SHOWCASE.highlight.uri

/// Model assembled from the typed FlatBuffers update frame. Tier-3 fields come
/// from `SnapshotFrame` itself; host/kernel projections come from
/// `SnapshotFrame.typed_projections`.
struct GallerySnapshot: Equatable, @unchecked Sendable {
    let running: Bool
    let profiles: [String: ProfileWire]
    let accounts: [AccountWire]
    /// Pre-resolved embed-projection map produced by the `NEMB` typed sidecar.
    let claimedEventEmbeds: [String: EmbeddedEventEnvelope]?
    let relayRoleOptions: [GalleryRelayRoleOption]

    static let empty = GallerySnapshot(running: false, profiles: [:], accounts: [], claimedEventEmbeds: nil)

    init(running: Bool, profiles: [String: ProfileWire], accounts: [AccountWire],
         claimedEventEmbeds: [String: EmbeddedEventEnvelope]? = nil,
         relayRoleOptions: [GalleryRelayRoleOption] = []) {
        self.running = running
        self.profiles = profiles
        self.accounts = accounts
        self.claimedEventEmbeds = claimedEventEmbeds
        self.relayRoleOptions = relayRoleOptions
    }

}

struct AccountWire: Equatable {
    let pubkey: String
    let active: Bool

    init(pubkey: String, active: Bool) {
        self.pubkey = pubkey
        self.active = active
    }
}

/// `@Observable` mirror of the gallery snapshot. The kernel pushes
/// FlatBuffers update frames through `GalleryKernelHandle.listen`; this class
/// decodes them and republishes for SwiftUI consumption.
@MainActor
@Observable
final class GalleryModel: NostrProfileHost {
    private(set) var snapshot: GallerySnapshot = .empty
    private(set) var lastDecodeError: String?
    private let kernel: GalleryKernelHandle

    /// Embed-projection host. Reads `projections.claimed_events` from every
    /// snapshot push (M16 / ADR-0034) so kind-dispatched embed renderers see
    /// resolved envelopes without re-parsing the kernel wire.
    let embedHost = EmbedHost()

    /// Concrete `EventClaimSinkProtocol` impl forwarded into the SwiftUI
    /// environment so `EmbeddedEvent` views can fire `claim`/`release` against
    /// the gallery's live kernel. Stored (not computed / lazy) so the
    /// `@Observable` macro can synthesize storage.
    let embedClaimSink: EventClaimSinkProtocol

    init() {
        let kernel = GalleryKernelHandle()
        self.kernel = kernel
        self.embedClaimSink = KernelEventClaimSink(kernel: kernel)
    }

    /// One-shot bootstrap. Wires the push callback, starts the kernel actor,
    /// seeds the bootstrap relay set so component-owned claims have relays.
    func start() {
        // Wire the push callback BEFORE start so the very first snapshot
        // tick lands in our model. The callback fires from the kernel actor
        // thread; we hop to the main actor before touching `@Observable`
        // state.
        kernel.listen { [weak self] payload in
            Task { @MainActor [weak self] in
                self?.decode(frame: payload)
            }
        }
        kernel.start()
        // Seed bootstrap relays. The gallery has no logged-in user → no
        // kind:10002 → empty `app_relays` and no routing target. Adding these
        // before any component-owned profile claim means the first claim
        // already has candidates instead of waiting for an external mailbox
        // to arrive.
        for relay in GALLERY_SHOWCASE.relays {
            kernel.addRelay(url: relay.url, role: relay.role)
        }
        // Do not open the showcase author here. The user-avatar registry component
        // claims `SHOWCASE_PUBKEY_HEX` when it mounts, and the kernel surfaces the
        // result through `projections.claimed_profiles`.
    }

    /// Decode a FlatBuffers update frame received from the push callback. A
    /// decode failure logs and keeps the previous snapshot intact (soft-fail).
    ///
    /// Gallery decodes `SnapshotFrame.typed_projections` plus Tier-3 fields
    /// directly. The legacy `snapshot.payload` generic tree is not a source.
    func decode(frame: Data) {
        do {
            let next = try GalleryTypedSnapshotDecoder.snapshot(from: frame) { [kernel] pubkey in
                kernel.encodeProfile(pubkey: pubkey)
            }
            self.snapshot = next
            self.lastDecodeError = nil
            embedHost.update(claimedEventEmbeds: next.claimedEventEmbeds)
        } catch {
            let msg = "Gallery typed snapshot decode failed: \(error.localizedDescription)"
            gmLog.error("\(msg, privacy: .public)")
            self.lastDecodeError = msg
        }
    }

    /// Convenience accessor for the showcase profile. Returns nil while kind:0
    /// is still in flight — most call sites should prefer
    /// [`bestEffortProfile`] which never returns nil.
    var showcaseProfile: ProfileWire? {
        snapshot.profiles[SHOWCASE_PUBKEY_HEX]
    }

    /// Always-renderable `ProfileWire` for the showcase identity. Returns the
    /// real kernel-supplied profile when kind:0 has arrived; otherwise a
    /// fallback built from `(SHOWCASE_PUBKEY_HEX, SHOWCASE_NPUB, SHOWCASE_NPUB_SHORT)`
    /// with every optional field set to nil.
    ///
    /// The registry components are designed to degrade gracefully on
    /// missing fields (identicon avatar fallback, `npubShort` display name
    /// fallback, hidden NIP-05 badge), so user-* component pages can render
    /// immediately on first frame and update reactively when the real
    /// profile lands — no spinner.
    ///
    /// `GalleryModel` is `@Observable`; SwiftUI re-evaluates this
    /// computed property on every snapshot change, so the cutover from
    /// fallback → real profile is automatic.
    var bestEffortProfile: ProfileWire {
        if let real = snapshot.profiles[SHOWCASE_PUBKEY_HEX] {
            return real
        }
        return ProfileWire(
            pubkey: SHOWCASE_PUBKEY_HEX,
            displayName: nil,
            about: nil,
            pictureUrl: nil,
            nip05: nil,
            npub: SHOWCASE_NPUB,
            npubShort: SHOWCASE_NPUB_SHORT
        )
    }

    /// Lookup any profile that arrived through the gallery's profiles map.
    func profile(forPubkey pubkey: String) -> ProfileWire? {
        snapshot.profiles[pubkey]
    }

    /// Kernel-emitted relay-role presentation tokens (issue #996). The
    /// relay-list page resolves each `configured_relays.role` against this
    /// list for its `label`/`tint` — the same kernel source of truth Chirp
    /// uses, with no Swift-side role derivation.
    var relayRoleOptions: [GalleryRelayRoleOption] {
        snapshot.relayRoleOptions
    }

    /// NostrProfileHost: demand a profile projection for a mounted component.
    func claimProfile(pubkey: String, consumerID: String) {
        kernel.claimProfile(pubkey: pubkey, consumerID: consumerID)
    }

    /// NostrProfileHost: release a component's profile interest on unmount.
    func releaseProfile(pubkey: String, consumerID: String) {
        kernel.releaseProfile(pubkey: pubkey, consumerID: consumerID)
    }

    /// Showcase write surface (phase 2). Dispatches a sign-in action without
    /// holding the secret on the Swift side beyond this call.
    func signInShowcase(nsec: String) {
        kernel.signInNsec(nsec)
    }
}
