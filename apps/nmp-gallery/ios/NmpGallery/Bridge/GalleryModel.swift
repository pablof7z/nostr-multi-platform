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

/// One entry of the kernel's `projections.relay_role_options` array — the
/// canonical role token paired with the kernel-emitted human-readable `label`
/// and semantic `tint`. The relay-list component consumes `label`/`tint` from
/// here directly; no role→label/tint derivation lives in Swift (ADR-0041,
/// issue #996). Mirrors Chirp's `RelayRoleOption`.
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

/// Wire-shape of one entry in `projections."refs.profile"` — the kernel's
/// `ProfileCard`. Field names use snake_case in JSON; the decoder uses the
/// global `.convertFromSnakeCase` strategy so Swift sees camelCase.
///
/// ADR-0063 (#1671): the map is sourced from the kernel's `refs.profile`
/// row-delta projection (the resolve_ref output), merged host-side into the
/// `GalleryRefStores` and materialised under the `refs.profile` JSON key by
/// `nmp_app_gallery_snapshot_json_from_update_frame`. The app JSON adapter
/// derives a bech32 `npub` from the raw pubkey for this gallery-only view. The
/// extra `lnurl` field the card carries is ignored here.
private struct RefProfileWire: Decodable, Sendable {
    let pubkey: String
    let npub: String
    let displayName: String?
    let pictureUrl: String?
    let nip05: String?
    let about: String?
}

/// Snapshot wire-shape pushed through `nmp_app_set_update_callback`. The
/// kernel's `KernelSnapshot` ships a host-extensible `projections` map; the
/// gallery reads the resolved-profile key from it:
///
///   * `projections."refs.profile"[pubkey]` — the kernel's single resolved
///     `ProfileCard` per pubkey (ADR-0063 #1671 — the resolve_ref output,
///     materialised from the `refs.profile` row-delta store host-side). The
///     gallery owns no precedence merge. Always present (`{}` when empty).
///
///   * `projections."refs.event.envelopes"[primaryId]` — the render-facing
///     embed envelope derived from the authoritative `refs.event` row store.
///
/// `snapshot.profiles[pubkey] -> ProfileWire?` is decoded directly from that
/// surface so the per-component pages stay decoupled from the wire
/// shape. Decoding is fault-tolerant — a missing/null projection key
/// degrades to an empty map instead of failing the whole tick.
struct GallerySnapshot: Decodable, Equatable, Sendable {
    let running: Bool
    let profiles: [String: ProfileWire]
    let accounts: [AccountWire]
    /// Pre-resolved event-ref embed envelope map derived from `refs.event`
    /// after `resolve_ref` (ADR-0063 / ADR-0034). Key = `primary_id`; value =
    /// fully resolved `EmbeddedEventEnvelope` with `projection` already
    /// kind-dispatched in Rust. Nil when the projection is absent.
    let resolvedEventEmbeds: [String: EmbeddedEventEnvelope]?
    /// Kernel-emitted relay-role presentation tokens from
    /// `projections.relay_role_options` (issue #996). The relay-list page
    /// looks `configured_relays.role` up here for `label`/`tint`, exactly as
    /// Chirp's `RelayConfigRow` does — no Swift-side role derivation.
    let relayRoleOptions: [GalleryRelayRoleOption]

    static let empty = GallerySnapshot(running: false, profiles: [:], accounts: [], resolvedEventEmbeds: nil)

    init(running: Bool, profiles: [String: ProfileWire], accounts: [AccountWire],
         resolvedEventEmbeds: [String: EmbeddedEventEnvelope]? = nil,
         relayRoleOptions: [GalleryRelayRoleOption] = []) {
        self.running = running
        self.profiles = profiles
        self.accounts = accounts
        self.resolvedEventEmbeds = resolvedEventEmbeds
        self.relayRoleOptions = relayRoleOptions
    }

    private enum CodingKeys: String, CodingKey {
        case running, projections, accounts
    }

    private enum ProjectionsKeys: String, CodingKey {
        // ADR-0063 (#1671): the resolved-profile map is keyed by the dotted
        // `refs.profile` projection key. `.convertFromSnakeCase` does NOT touch
        // a dotted key, so the raw value is spelled out explicitly here.
        case refsProfile = "refs.profile"
        case accounts
        // ADR-0063 (#1671): event embed envelopes are derived from the
        // `refs.event` row-delta store under their own dotted projection key.
        case refEventEnvelopes = "refs.event.envelopes"
        // `relay_role_options` → camelCase after `.convertFromSnakeCase`.
        case relayRoleOptions
    }

    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)
        self.running = try container.decodeIfPresent(Bool.self, forKey: .running) ?? false

        // `accounts` may live either at the top level (legacy / test fixtures)
        // or under `projections.accounts` (current kernel snapshot shape).
        var resolvedAccounts: [AccountWire] = []

        var assembled: [String: ProfileWire] = [:]
        var resolvedEventEmbeds: [String: EmbeddedEventEnvelope]? = nil
        var roleOptions: [GalleryRelayRoleOption] = []
        if let projections = try? container.nestedContainer(
            keyedBy: ProjectionsKeys.self,
            forKey: .projections
        ) {
            // ADR-0063 (#1671): the kernel ships one resolved card per pubkey
            // under `refs.profile` (the resolve_ref output, materialised from
            // the row-delta store in Rust). The gallery just decodes the result.
            if let resolved = try? projections.decodeIfPresent(
                [String: RefProfileWire].self,
                forKey: .refsProfile
            ) {
                for (pubkey, card) in resolved {
                    let key = card.pubkey.isEmpty ? pubkey : card.pubkey
                    assembled[key] = profileWire(fromRefProfile: card, pubkey: key)
                }
            }
            if let accs = try? projections.decodeIfPresent(
                [AccountWire].self,
                forKey: .accounts
            ) {
                resolvedAccounts = accs
            }
            // Issue #1283 / ADR-0034: decode the pre-resolved embed map derived
            // from `refs.event`. Fault-tolerant — nil when absent.
            resolvedEventEmbeds = try? projections.decodeIfPresent(
                [String: EmbeddedEventEnvelope].self,
                forKey: .refEventEnvelopes
            )
            // Issue #996: decode the kernel's relay-role presentation tokens so
            // the relay-list page resolves label/tint from the kernel source of
            // truth instead of deriving them in Swift.
            if let opts = try? projections.decodeIfPresent(
                [GalleryRelayRoleOption].self,
                forKey: .relayRoleOptions
            ) {
                roleOptions = opts
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
        self.resolvedEventEmbeds = resolvedEventEmbeds
        self.relayRoleOptions = roleOptions
    }
}

/// Build a `ProfileWire` from one `refs.profile` entry. The gallery JSON
/// adapter derives the full `npub` from the raw pubkey; `npubShort` is the only
/// Swift-side derivation. aim.md §2 stipulates shells own abbreviation.
private func profileWire(fromRefProfile card: RefProfileWire, pubkey: String) -> ProfileWire {
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

/// Truncate a bech32 npub for display (e.g. `npub1abcd…wxyz`). Mirrors the
/// Rust-side helper the kernel deleted (aim.md §2 — shells own abbreviation).
private func shortenNpub(_ npub: String) -> String {
    guard npub.count > 12 else { return npub }
    let prefix = npub.prefix(9) // "npub1XXXX"
    let suffix = npub.suffix(4)
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
struct AccountWire: Decodable, Equatable, Sendable {
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

    /// Embed-projection host. Reads resolved event-ref embed envelopes from every
    /// snapshot push (M16 / ADR-0034) so kind-dispatched embed renderers see
    /// resolved envelopes without re-parsing the kernel wire.
    let embedHost = EmbedHost()

    /// Concrete `EventRefResolverProtocol` impl forwarded into the SwiftUI
    /// environment so `EmbeddedEvent` views can fire resolve/release against
    /// the gallery's live kernel. Stored (not computed / lazy) so the
    /// `@Observable` macro can synthesize storage.
    let embedEventRefResolver: EventRefResolverProtocol

    init() {
        let kernel = GalleryKernelHandle()
        self.kernel = kernel
        self.embedEventRefResolver = KernelEventRefResolver(kernel: kernel)
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
        // before any component-owned profile resolve means the first resolve
        // already has candidates instead of waiting for an external mailbox
        // to arrive.
        for relay in GALLERY_SHOWCASE.relays {
            kernel.addRelay(url: relay.url, role: relay.role)
        }
        // Do not open the showcase author here. The user-avatar registry component
        // resolves `SHOWCASE_PUBKEY_HEX` when it mounts, and the kernel surfaces the
        // result through `projections."refs.profile"`.
    }

    /// Decode a FlatBuffers update frame received from the push callback. A
    /// decode failure logs and keeps the previous snapshot intact (soft-fail).
    ///
    /// `GallerySnapshot` includes `resolvedEventEmbeds` — the pre-resolved
    /// event-ref embed envelope map derived from `refs.event` after
    /// `resolve_ref` and materialised under `refs.event.envelopes`.
    /// A single `JSONDecoder` pass fills both the profile/account fields and the
    /// embed map; the separate `JSONSerialization` + `EmbedHost.update(fromSnapshotJSON:)`
    /// path is deleted (the kind-dispatch now runs in Rust, not in Swift).
    func decode(frame: Data) {
        guard let data = GalleryFlatBufferSnapshotDecoder.snapshotJSONData(
            from: frame, stores: kernel.refStores) else {
            return
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase

        do {
            let next = try decoder.decode(GallerySnapshot.self, from: data)
            self.snapshot = next
            self.lastDecodeError = nil
            // Embed-projection: feed the pre-resolved map directly from the
            // typed `GallerySnapshot` field (no separate JSONSerialization pass).
            embedHost.update(resolvedEventEmbeds: next.resolvedEventEmbeds)
        } catch {
            let msg = "GallerySnapshot direct decode failed: \(error.localizedDescription)"
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
    func resolveProfileRef(pubkey: String, consumerID: String) {
        kernel.resolveProfileRef(pubkey: pubkey, consumerID: consumerID)
    }

    /// NostrProfileHost: release a component's profile interest on unmount.
    func releaseProfileRef(pubkey: String, consumerID: String) {
        kernel.releaseProfileRef(pubkey: pubkey, consumerID: consumerID)
    }

    /// Showcase write surface (phase 2). Dispatches a sign-in action without
    /// holding the secret on the Swift side beyond this call.
    func signInShowcase(nsec: String) {
        kernel.signInNsec(nsec)
    }
}
