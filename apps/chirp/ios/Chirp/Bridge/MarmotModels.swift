import Foundation

// Marmot (MLS-over-Nostr) decoded snapshot DTOs — verbatim FFI schema.
// Extracted from MarmotBridge.swift so neither file crosses the 500-LOC
// hard cap (AGENTS.md). The MarmotStore + dispatch wrappers stay in
// MarmotBridge.swift; these are the pure value types it mirrors.

// ── Decoded snapshot DTOs (verbatim FFI schema) ──────────────────────────

/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// (inherited by every nested type through the FlatBuffer decoder) maps
/// `"id_hex"` → `idHex`, `"member_count"` → `memberCount`, `"last_msg_at"` →
/// `lastMsgAt`, `"unread_count"` → `unreadCount` automatically.
/// An explicit enum with snake_case rawValues would CONFLICT with the
/// FlatBuffer decoder, which has already applied `convertFromSnakeCase`
/// before any `CodingKey` lookup (identical pattern to `GroupEvent`
/// and `DiscoveredGroup` — see their comments in `KernelBridge.swift`).
struct MarmotGroup: Decodable, Identifiable, Equatable {
    let idHex: String
    /// Group name verbatim from MLS metadata. May be empty — use `displayName`
    /// for rendering (applies the "Untitled group" fallback). aim.md §2: raw
    /// data on the wire; shells own the fallback copy.
    let name: String
    /// Member Nostr pubkeys, hex (64 chars). Presentation layer formats
    /// each entry for display (ADR-0032).
    let members: [String]
    /// Member count (length of `members`). Pluralisation lives in the
    /// presentation layer (ADR-0032).
    let memberCount: UInt32
    /// Total decrypted application-message count for the group, or `nil`
    /// when zero. Read-cursor seam — the host shell owns the per-device
    /// read watermark.
    let unreadCount: UInt32?
    let lastMsgAt: UInt64?

    var id: String { idHex }

    // ── Shell-owned presentation (aim.md §2) ────────────────────────────

    /// Empty-name fallback applied in the shell (D7 — presentation belongs
    /// to the native layer, not the wire projection).
    var displayName: String { name.isEmpty ? "Untitled group" : name }

    /// 2-char uppercase initials for the avatar tile, derived from `name`.
    /// Returns `"?"` when name is blank. Shell-computed per aim.md §2.
    var initials: String {
        let chars = name.unicodeScalars.filter { !CharacterSet.whitespaces.contains($0) }
        guard let first = chars.first else { return "?" }
        let firstChar = Character(first)
        let rest = chars.dropFirst()
        if let second = rest.first {
            return "\(firstChar)\(Character(second))".uppercased()
        }
        return String(firstChar).uppercased()
    }
}

/// No explicit `CodingKeys`: `.convertFromSnakeCase` maps `"id_hex"` →
/// `idHex`, `"group_name"` → `groupName`, `"inviter_npub"` → `inviterNpub`
/// automatically (same pattern as `MarmotGroup` above).
struct MarmotPendingWelcome: Decodable, Identifiable, Equatable {
    let idHex: String
    /// Group name verbatim from the Welcome envelope. May be empty — use
    /// `displayName` for rendering (applies the "Group invite" fallback).
    let groupName: String
    /// The inviter's Nostr pubkey, hex (64 chars — the field name is
    /// historical; the value is hex, not bech32). Presentation layer
    /// formats for display (ADR-0032).
    let inviterNpub: String

    var id: String { idHex }

    // ── Shell-owned presentation (aim.md §2) ────────────────────────────

    /// Empty-name fallback applied in the shell.
    var displayName: String { groupName.isEmpty ? "Group invite" : groupName }
}

/// No explicit `CodingKeys`: `.convertFromSnakeCase` maps `"d_tag"` → `dTag`,
/// `"age_secs"` → `ageSecs`, `"is_registered"` → `isRegistered` automatically
/// (same pattern as `MarmotGroup` above).
struct MarmotKeyPackage: Decodable, Equatable {
    let published: Bool
    let dTag: String?
    let ageSecs: UInt64?
    let stale: Bool
    /// `true` when this status was built against a registered Marmot signing
    /// identity. `false` only when no handle exists. Shells gate the publish
    /// button and derive subtitle copy from this + published/ageSecs/stale.
    let isRegistered: Bool

    static let empty = MarmotKeyPackage(
        published: false,
        dTag: nil,
        ageSecs: nil,
        stale: false,
        isRegistered: false
    )
}

/// One op parked in the deferred-completion store waiting for peer key packages
/// (schema_version 2+). Mirrors Rust `PendingOpRow`. Typed-decode only.
struct MarmotPendingOp: Decodable, Identifiable, Equatable {
    let correlationId: String // matches `action_lifecycle` entries
    let opTag: String         // `"create_group"` | `"invite"`
    let missingCount: UInt32  // pubkeys still missing a cached key package
    let ageSecs: UInt64       // wall-clock seconds since the op was parked
    var id: String { correlationId }

    // ── Shell-owned presentation (aim.md §2) ────────────────────────────

    /// Human-readable label derived from `missingCount`. Shell-computed per
    /// aim.md §2 — the projection sends raw counts, not display strings.
    var displayLabel: String {
        "Waiting for key packages (\(missingCount))\u{2026}"
    }
}

/// The most recent terminal op FAILURE (deferred-op expiry / failed retry), or
/// `nil` when none. Mirrors Rust `LastOpError`. Raw data only (aim.md §2):
/// `reason` is a machine code; `bannerText` maps it to a user banner — the one
/// sanctioned place for native failure copy. Unknown codes fall back to a
/// generic op-tagged message (D6: never a bare code or empty banner).
struct MarmotLastOpError: Decodable, Equatable {
    let op: String            // failing op tag ("create_group" | "invite")
    let reason: String        // machine code, e.g. "key_package_unavailable"
    let atSecs: UInt64        // wall-clock second the failure was recorded
    let correlationId: String // correlation_id of the failed action

    var bannerText: String {
        let opLabel = switch op {
        case "create_group": "create the group"
        case "invite": "send the invite"
        default: "complete the last action"
        }
        return switch reason {
        case "key_package_unavailable":
            "Couldn't \(opLabel) — a member has no key package published yet. Try again later."
        default: "Couldn't \(opLabel)."
        }
    }
}

/// `.convertFromSnakeCase` maps snake_case keys automatically. `pendingOps` /
/// `lastOpError` are typed-decode only (`TypedProjectionGlue`); the JSON path
/// omits them, so the custom `init(from:)` defaults them via `decodeIfPresent`.
struct MarmotSnapshot: Decodable, Equatable {
    let groups: [MarmotGroup]
    let pendingWelcomes: [MarmotPendingWelcome]
    let keyPackage: MarmotKeyPackage
    let cachedKpPubkeys: [String]
    /// `true` when built against a registered Marmot signing identity; `false`
    /// for the `.empty` fallback (no `MarmotHandle`). Both branches Rust-owned.
    let isRegistered: Bool
    /// Ops parked waiting for peer KPs (v2+). Empty when none. Typed-decode only.
    var pendingOps: [MarmotPendingOp] = []
    /// Most recent terminal op failure (v2+), or `nil`. Typed-decode only.
    var lastOpError: MarmotLastOpError?
    /// #1651 service-init failure machine token (replaces the V-62
    /// `keyringUnavailable` bool the Chirp domain type formerly dropped):
    /// `""` = none, `"keyring_unavailable"`, `"db_key_lost"`. Typed-decode only.
    var initErrorKind: String = ""
    /// #1651 raw init-error detail (`db_key_lost` only), empty otherwise.
    var initErrorDetail: String = ""

    // ── Shell-owned presentation (aim.md §2) ────────────────────────────

    /// Pluralised label for the top-of-list pending-invites chip, or `nil`
    /// when no welcomes are pending. Computed in the shell from raw count
    /// (aim.md §2 — pluralisation is presentation, not protocol data).
    var invitesChipLabel: String? {
        switch pendingWelcomes.count {
        case 0: return nil
        case 1: return "1 invite"
        case let n: return "\(n) invites"
        }
    }

    enum CodingKeys: String, CodingKey {
        case groups, pendingWelcomes, keyPackage, cachedKpPubkeys
        case isRegistered, pendingOps, lastOpError
    }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        groups = try c.decode([MarmotGroup].self, forKey: .groups)
        pendingWelcomes = try c.decode([MarmotPendingWelcome].self, forKey: .pendingWelcomes)
        keyPackage = try c.decode(MarmotKeyPackage.self, forKey: .keyPackage)
        cachedKpPubkeys = try c.decode([String].self, forKey: .cachedKpPubkeys)
        isRegistered = try c.decode(Bool.self, forKey: .isRegistered)
        pendingOps = try c.decodeIfPresent([MarmotPendingOp].self, forKey: .pendingOps) ?? []
        lastOpError = try c.decodeIfPresent(MarmotLastOpError.self, forKey: .lastOpError)
    }

    init(
        groups: [MarmotGroup], pendingWelcomes: [MarmotPendingWelcome],
        keyPackage: MarmotKeyPackage, cachedKpPubkeys: [String],
        isRegistered: Bool,
        pendingOps: [MarmotPendingOp] = [], lastOpError: MarmotLastOpError? = nil,
        initErrorKind: String = "", initErrorDetail: String = ""
    ) {
        self.groups = groups; self.pendingWelcomes = pendingWelcomes
        self.keyPackage = keyPackage; self.cachedKpPubkeys = cachedKpPubkeys
        self.isRegistered = isRegistered
        self.pendingOps = pendingOps; self.lastOpError = lastOpError
        self.initErrorKind = initErrorKind; self.initErrorDetail = initErrorDetail
    }

    static let empty = MarmotSnapshot(
        groups: [],
        pendingWelcomes: [],
        keyPackage: .empty,
        cachedKpPubkeys: [],
        isRegistered: false,
        pendingOps: [],
        lastOpError: nil
    )
}

/// No explicit `CodingKeys`: `.convertFromSnakeCase` maps
/// `"sender_pubkey_hex"` → `senderPubkeyHex` and `"created_at"` → `createdAt`
/// automatically (same pattern as `MarmotGroup` above).
struct MarmotMessage: Decodable, Identifiable, Equatable {
    let id: String
    /// Author Nostr pubkey, hex (64 chars). Presentation layer formats
    /// for display (ADR-0032).
    let senderPubkeyHex: String
    let content: String
    /// Rumor `created_at` (sender clock, Unix seconds). Presentation
    /// layer formats via `relativeTimeFromUnixSeconds` (ADR-0032).
    let createdAt: UInt64
    let epoch: UInt64?
}

/// Result envelope every Marmot dispatch wrapper returns (ADR-0025 PR 3).
///
/// `dispatch_action` is non-blocking: it returns a `correlation_id`
/// synchronously; the real outcome arrives asynchronously via
/// (a) `snapshot.pendingOps` (parked, waiting for KPs) and
/// (b) `action_lifecycle.recentTerminal` (settled — same seam as
/// RelaySettingsView). Callers stash `correlationId` and drive their UI from
/// (a)/(b), not from `ok`; dismiss only on terminal `.accepted`.
struct MarmotOpResult: Equatable {
    /// `true` when `dispatch_action` accepted the submission; `false` on
    /// synchronous rejection (bridge unavailable, malformed JSON).
    let ok: Bool
    /// Synchronous rejection reason, or `nil` on acceptance.
    let error: String?
    /// Kernel-minted correlation id on acceptance; `nil` on rejection. Matches
    /// `snapshot.pendingOps` rows and `action_lifecycle.recentTerminal` entries.
    let correlationId: String?

    static let bridgeUnavailable = MarmotOpResult(
        ok: false, error: "marmot bridge unavailable", correlationId: nil)

    /// PR 3: submission accepted by `dispatch_action`; keeps the kernel-minted
    /// `correlationId` so callers can match the async terminal verdict.
    static func submitted(correlationId: String) -> MarmotOpResult {
        MarmotOpResult(ok: true, error: nil, correlationId: correlationId)
    }

    static func failure(_ message: String) -> MarmotOpResult {
        MarmotOpResult(ok: false, error: message, correlationId: nil)
    }
}
