import Foundation
import Observation
import os.log

private let gmLog = Logger(subsystem: "org.nmp.gallery", category: "GalleryModel")

/// Pubkey of the demo account whose profile the gallery claims on startup.
/// pablof7z — picked as a known well-populated kind:0 source for live data.
let DEMO_PUBKEY_HEX = "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"

/// Full bech32 `npub1…` form of [`DEMO_PUBKEY_HEX`]. Used as a fallback
/// before kind:0 arrives so user-* component pages can render real-shape
/// data immediately (no spinner).
///
/// Computed once in Rust via `nmp_core::display::to_npub(DEMO_PUBKEY_HEX)`
/// and pinned here as a literal — Swift never reformats npubs (aim.md §6.9).
let DEMO_NPUB =
    "npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft"

/// Rust-truncated `npub1…` form of [`DEMO_PUBKEY_HEX`]: first 10 chars +
/// `"…"` + last 6 chars. Matches `nmp_core::display::short_npub` exactly
/// — pinned by a `nmp-core` unit test so any drift in the canonical
/// abbreviation algorithm fails CI before this constant goes stale.
///
/// Used only as a placeholder in [`GalleryModel.bestEffortProfile`] until
/// the kernel pushes the real `ProfileWire` (which carries its own
/// Rust-computed `npubShort`).
let DEMO_NPUB_SHORT = "npub1l2vyh…utajft"

/// Stable consumer id for the gallery's profile interest. The kernel
/// refcounts profile claims per `(pubkey, consumer_id)` pair; using one stable
/// id means claim/release stays balanced even across re-renders.
let GALLERY_PROFILE_CONSUMER_ID = "gallery"

/// Bootstrap relay set seeded into the kernel on cold start. The gallery has
/// no logged-in user, so there is no kind:10002 to source app relays from;
/// without these seeds the kernel has nowhere to send a kind:0 fetch and
/// every component page hangs on a placeholder.
///
///   • `wss://purplepag.es`  — canonical kind:0 / kind:10002 indexer
///     (`FALLBACK_INDEXER_RELAY` in `crates/nmp-core/src/relay.rs`).
///   • `wss://relay.damus.io`, `wss://nos.lol` — redundancy so pablof7z's
///     write relays (discovered via kind:10002) remain reachable even if one
///     of them is throttling at the moment of the screenshot.
///
/// Role `"both"` lets the same socket carry inbox + outbox legs of the
/// planner's interest set (the diagnostic lane is `RelayRole::Content`; the
/// NIP-65 read/write split lives on the `RelayEditRow`, not on the pool key).
let GALLERY_BOOTSTRAP_RELAYS: [String] = [
    "wss://purplepag.es",
    "wss://relay.damus.io",
    "wss://nos.lol",
]

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
/// gallery reads two keys from it:
///
///   * `projections.author_view.profile` — populated by `open_author`,
///     carries a full `ProfileCard` with `npub`, `nip05`, and `about`.
///     This is the primary source for the gallery's demo author.
///   * `projections.mention_profiles[pubkey]` — populated for every author
///     whose notes appear in a visible timeline / author view / thread
///     view. Carries `display_name` + `picture_url` only (no `npub`).
///     Secondary source; useful once the author_view's items are visible.
///
/// `snapshot.profiles[pubkey] -> ProfileWire?` is synthesised from those
/// two surfaces so the per-component pages stay decoupled from the wire
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
        case authorView, mentionProfiles, accounts
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

/// `@Observable` mirror of the gallery snapshot. The kernel pushes
/// FlatBuffers update frames through `GalleryKernelHandle.listen`; this class
/// decodes them and republishes for SwiftUI consumption.
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
    /// seeds the bootstrap relay set, then opens an author view on the demo
    /// pubkey so user-* component pages have real data to render.
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
        // BEFORE the author-view open means the very first compile pass
        // already has candidates, so the kind:0 fetch ships on tick 1
        // instead of waiting for an external mailbox to arrive.
        for url in GALLERY_BOOTSTRAP_RELAYS {
            kernel.addRelay(url: url, role: "both")
        }
        // Open an author view on the demo pubkey (pablof7z). The kernel
        // fetches kind:10002 + kind:0 from the discovery relays seeded
        // above and surfaces the resolved `ProfileCard` under
        // `projections.author_view.profile` in the push-callback snapshot.
        //
        // We deliberately do NOT use `claim_profile` here even though it
        // is semantically the right primitive for "I want this profile":
        // the kernel populates its internal `self.profiles` cache on
        // kind:0 arrival, but no projection on the snapshot wire surface
        // exposes that map for a claim-only pubkey (only the active
        // account lands in `projections.profile`). `open_author` is the
        // shortest path to the same data with a decoder-visible projection.
        kernel.openAuthor(pubkey: DEMO_PUBKEY_HEX)
    }

    /// Decode a FlatBuffers update frame received from the push callback. A
    /// decode failure logs and keeps the previous snapshot intact (soft-fail).
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
    }

    /// Convenience accessor for the demo profile. Returns nil while kind:0
    /// is still in flight — most call sites should prefer
    /// [`bestEffortProfile`] which never returns nil.
    var demoProfile: ProfileWire? {
        snapshot.profiles[DEMO_PUBKEY_HEX]
    }

    /// Always-renderable `ProfileWire` for the demo account. Returns the
    /// real kernel-supplied profile when kind:0 has arrived; otherwise a
    /// placeholder built from `(DEMO_PUBKEY_HEX, DEMO_NPUB, DEMO_NPUB_SHORT)`
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
    /// placeholder → real profile is automatic.
    var bestEffortProfile: ProfileWire {
        if let real = snapshot.profiles[DEMO_PUBKEY_HEX] {
            return real
        }
        return ProfileWire(
            pubkey: DEMO_PUBKEY_HEX,
            displayName: nil,
            about: nil,
            pictureUrl: nil,
            nip05: nil,
            npub: DEMO_NPUB,
            npubShort: DEMO_NPUB_SHORT
        )
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
