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

/// Wire-shape of `projections.author_view.profile` — the kernel's
/// `ProfileCard`. Field names use snake_case in JSON; the decoder uses the
/// global `.convertFromSnakeCase` strategy so Swift sees camelCase.
private struct AuthorProfileWire: Decodable {
    let pubkey: String
    let npub: String
    let displayName: String?
    let pictureUrl: String?
    let nip05: String?
    let about: String?
    let hasProfile: Bool?
}

/// Wire-shape of one entry in `projections.mention_profiles` — the kernel's
/// `MentionProfilePayload`. Carries the bare minimum (no `npub`, no `nip05`,
/// no `about`) so the gallery decoder falls back to deriving an `npubShort`
/// from the hex pubkey when only this surface is available.
private struct MentionProfileWire: Decodable {
    let pubkey: String
    let displayName: String?
    let pictureUrl: String?
}

/// Wire-shape of `projections.author_view` (or null when no view is open).
private struct AuthorViewWire: Decodable {
    let pubkey: String
    let profile: AuthorProfileWire
}

/// Snapshot wire-shape pushed through `nmp_app_set_update_callback`. The
/// kernel's `KernelSnapshot` ships a host-extensible `projections` map; the
/// gallery reads three profile keys from it:
///
///   * `projections.claimed_profiles[pubkey]` — populated by component-owned
///     `claim_profile` lifecycles. This is the registry component happy path.
///   * `projections.author_view.profile` — populated by `open_author`,
///     carries a full `ProfileCard` with `npub`, `nip05`, and `about`.
///   * `projections.mention_profiles[pubkey]` — populated for every author
///     whose notes appear in a visible timeline / author view / thread
///     view. Carries `display_name` + `picture_url` only (no `npub`).
///
/// `snapshot.profiles[pubkey] -> ProfileWire?` is synthesised from those
/// surfaces so the per-component pages stay decoupled from the wire
/// shape. Decoding is fault-tolerant — a missing/null projection key
/// degrades to an empty map instead of failing the whole tick.
struct GallerySnapshot: Decodable, Equatable {
    let running: Bool
    let profiles: [String: ProfileWire]
    let accounts: [AccountWire]

    static let empty = GallerySnapshot(running: false, profiles: [:], accounts: [])

    init(running: Bool, profiles: [String: ProfileWire], accounts: [AccountWire]) {
        self.running = running
        self.profiles = profiles
        self.accounts = accounts
    }

    private enum CodingKeys: String, CodingKey {
        case running, projections, accounts
    }

    private enum ProjectionsKeys: String, CodingKey {
        case claimedProfiles, authorView, mentionProfiles, accounts
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.running = try container.decodeIfPresent(Bool.self, forKey: .running) ?? false

        // `accounts` may live either at the top level (legacy / test fixtures)
        // or under `projections.accounts` (current kernel snapshot shape).
        var resolvedAccounts: [AccountWire] = []

        var assembled: [String: ProfileWire] = [:]
        if let projections = try? container.nestedContainer(
            keyedBy: ProjectionsKeys.self,
            forKey: .projections
        ) {
            if let claimed = try? projections.decodeIfPresent(
                [String: AuthorProfileWire].self,
                forKey: .claimedProfiles
            ) {
                for (pubkey, card) in claimed {
                    assembled[pubkey] = profileWire(fromAuthorProfile: card, pubkey: pubkey)
                }
            }
            if let view = try? projections.decodeIfPresent(
                AuthorViewWire.self,
                forKey: .authorView
            ) {
                let card = view.profile
                let pubkey = card.pubkey.isEmpty ? view.pubkey : card.pubkey
                assembled[pubkey] = profileWire(fromAuthorProfile: card, pubkey: pubkey)
            }
            if let mentions = try? projections.decodeIfPresent(
                [String: MentionProfileWire].self,
                forKey: .mentionProfiles
            ) {
                for (pubkey, payload) in mentions where assembled[pubkey] == nil {
                    assembled[pubkey] = profileWire(fromMention: payload, pubkey: pubkey)
                }
            }
            if let accs = try? projections.decodeIfPresent(
                [AccountWire].self,
                forKey: .accounts
            ) {
                resolvedAccounts = accs
            }
        }
        // Top-level `accounts` fallback for tests / fixtures pre-projections.
        if resolvedAccounts.isEmpty,
           let topAccounts = try? container.decodeIfPresent(
               [AccountWire].self,
               forKey: .accounts
           )
        {
            resolvedAccounts = topAccounts
        }

        self.profiles = assembled
        self.accounts = resolvedAccounts
    }
}

/// Build a `ProfileWire` from the kernel's `ProfileCard` (which carries
/// `npub` already-formatted by Rust per aim.md §2). `npubShort` is the only
/// Swift-side derivation; aim.md §2 stipulates shells own abbreviation.
private func profileWire(fromAuthorProfile card: AuthorProfileWire, pubkey: String) -> ProfileWire {
    ProfileWire(
        pubkey: pubkey,
        displayName: card.displayName?.nonEmpty,
        about: card.about?.nonEmpty,
        pictureUrl: card.pictureUrl?.nonEmpty,
        nip05: card.nip05?.nonEmpty,
        npub: card.npub,
        npubShort: shortenNpub(card.npub)
    )
}

/// Build a `ProfileWire` from a `mention_profiles` payload. The mention
/// surface carries no `npub` / `nip05` / `about`, so the bech32 is empty
/// (the npubShort still derives from the hex via `shortenNpub`'s pubkey
/// suffix fallback when the npub is missing).
private func profileWire(fromMention payload: MentionProfileWire, pubkey: String) -> ProfileWire {
    ProfileWire(
        pubkey: pubkey,
        displayName: payload.displayName?.nonEmpty,
        about: nil,
        pictureUrl: payload.pictureUrl?.nonEmpty,
        nip05: nil,
        npub: "",
        npubShort: shortHexPubkey(pubkey)
    )
}

/// Truncate a bech32 npub for display (e.g. `npub1abcd…wxyz`). Mirrors the
/// Rust-side helper the kernel deleted (aim.md §2 — shells own abbreviation).
private func shortenNpub(_ npub: String) -> String {
    guard npub.count > 12 else { return npub }
    let prefix = npub.prefix(9) // "npub1XXXX"
    let suffix = npub.suffix(4)
    return "\(prefix)…\(suffix)"
}

/// Fallback display string when no npub is available (mention_profiles
/// payload). Shows the first 8 and last 4 hex chars.
private func shortHexPubkey(_ hex: String) -> String {
    guard hex.count > 12 else { return hex }
    let prefix = hex.prefix(8)
    let suffix = hex.suffix(4)
    return "\(prefix)…\(suffix)"
}

private extension String {
    /// Return `nil` for an empty string, otherwise `self`. Lets the gallery
    /// treat `displayName: ""` (kernel default) the same as a missing field.
    var nonEmpty: String? { isEmpty ? nil : self }
}

/// Minimal `accounts` row decoder. Phase 1 doesn't render accounts but
/// keeping a typed slot here means phase 2 (sign-in showcase) can wire UI
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
    /// The decode is split into two reads of the same JSON blob:
    ///   1. Typed `GallerySnapshot` decode — claimed_profiles / author_view /
    ///      mention_profiles / accounts. Lean: stays decoupled from any
    ///      embed-projection drift.
    ///   2. Raw JSONSerialization read passed through to `embedHost` so the
    ///      kind-dispatched embed projection (`projections.claimed_events`)
    ///      flows into the SwiftUI environment without expanding the typed
    ///      `GallerySnapshot` shape.
    func decode(frame: Data) {
        guard let data = GalleryFlatBufferSnapshotDecoder.snapshotJSONData(from: frame) else {
            return
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase

        do {
            let next = try decoder.decode(GallerySnapshot.self, from: data)
            self.snapshot = next
            self.lastDecodeError = nil
        } catch {
            let msg = "GallerySnapshot direct decode failed: \(error.localizedDescription)"
            gmLog.error("\(msg, privacy: .public)")
            self.lastDecodeError = msg
        }

        // Embed-projection ingest. Separate from the typed decode so a
        // claimed_events shape change cannot break user/relay/content pages.
        if let raw = try? JSONSerialization.jsonObject(with: data),
           let dict = raw as? [String: Any]
        {
            embedHost.update(fromSnapshotJSON: dict)
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
