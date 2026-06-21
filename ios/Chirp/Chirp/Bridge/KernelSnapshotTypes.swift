import Foundation

// Snapshot read-model DTOs (group-chat, group-discovery, zaps, DM, follow-list,
// relay-diagnostics, remote-signer/bunker/NIP-46 onboarding, settings-hub) plus
// the resolved-profile bridge. Extracted from `KernelBridge.swift` so the bridge
// file holds only `KernelHandle` (file-size hard-cap separation). Pure DTOs;
// same-module Swift files see each other without import.

// ─── resolved_profiles projection adapter ─────────────────────────────────
//
// `MentionProfile` is the rich, component-facing struct `NoteRenderContext`
// consumes. It is now built from a `ProfileCard` carried by the pre-merged
// `projections["resolved_profiles"]` map (PR #812) rather than from the older
// `mention_profiles` wire DTO. The component API (`[String: MentionProfile]`)
// is unchanged — only the source projection is broader and merged once in Rust.
// No Swift derives a `MentionProfile` from a `TimelineItem` anymore.

extension MentionProfile {
    /// Bridge from a resolved `ProfileCard`. `display` falls back to the
    /// abbreviated hex pubkey when no kind:0 has arrived (`ProfileCard
    /// .displayLabel`); avatar initials and tint colour are derived locally
    /// from the same inputs (`PubkeyFormatting.swift`). ADR-0032 — backend
    /// ships raw data, presentation layer formats.
    init(card: ProfileCard) {
        let display = card.displayLabel
        self.init(
            display: display,
            pictureUrl: card.pictureUrl,
            initials: display.displayInitials,
            colorHex: card.pubkey.pubkeyColorHex
        )
    }
}

/// Settings-hub view projection — `projections["settings_hub"]`. The kernel
/// now emits `relay_count` as an integer; the iOS shell computes the
/// pluralized subtitle locally. Decoded under `.convertFromSnakeCase`, so the
/// Rust `relay_count` JSON key matches the synthesized `relayCount` property
/// name directly.
struct SettingsHubSummary: Decodable, Equatable {
    let relayCount: Int

    var relaysSubtitle: String {
        switch relayCount {
        case 0: return "No relays configured"
        case 1: return "1 relay"
        default: return "\(relayCount) relays"
        }
    }

    static let empty = SettingsHubSummary(relayCount: 0)
}

// ─── NIP-29 group-chat read model ─────────────────────────────────────────
//
// Mirror of `nmp-nip29`'s `GroupChatSnapshot` / `GroupChatMessage` — the
// shape the `GroupChatProjection` serialises under the snapshot key
// `"nmp.nip29.group_chat"`. Thin-shell rule: these are pure DTOs; no Swift
// owns the ordering (the projection emits newest-first) or the membership
// filter (the projection matches kind + `h`-tag).

/// One rendered NIP-29 group-chat message. `pubkey` carries the event
/// author (hex); `kind` is one of 9 (chat) / 11 (discussion) / 1111
/// (comment). `id` is the event id (hex) and the stable list identity.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// (inherited by every nested type) maps the kernel's `"created_at"` /
/// `"created_at_display"` to `createdAt` / `createdAtDisplay` automatically.
/// An explicit enum would have to spell the post-transform name and is pure
/// surface area — omitted deliberately.
struct GroupChatMessage: Decodable, Identifiable, Equatable {
    let id: String
    /// Author Nostr pubkey, hex (64 chars). Presentation layer formats for
    /// display (ADR-0032).
    let pubkey: String
    let content: String
    /// Event `created_at` (Unix seconds). Presentation layer formats for
    /// display via `relativeTimeFromUnixSeconds` (ADR-0032).
    let createdAt: UInt64
    let kind: UInt32
}

/// The serialised read-model a group-chat screen consumes. `messages` is
/// ordered newest-first (`created_at` descending, ties broken by id) by the
/// Rust projection — Swift does not re-sort. Avatar / initials for the
/// group tile are derived by the presentation layer (ADR-0032).
struct GroupChatSnapshot: Decodable, Equatable {
    let messages: [GroupChatMessage]

    static let empty = GroupChatSnapshot(messages: [])
}

// ─── NIP-29 group-discovery read model ────────────────────────────────────
//
// Mirror of `nmp-nip29`'s `DiscoveredGroupsSnapshot` / `DiscoveredGroup` —
// the shape the `DiscoveredGroupsProjection` serialises under the snapshot
// key `"nmp.nip29.discovered_groups"`. Thin-shell rule: pure DTOs; no Swift
// owns the ordering (the projection emits alphabetical by `groupId`) or the
// member-count math (the projection counts `["p", _]` tags).

/// One discovered NIP-29 group, ready for `JoinGroupView` to render.
///
/// Raw protocol data only (ADR-0032). Presentation-layer fields such as
/// display-name fallback, avatar initials, and formatted subtitle are
/// computed by the `DiscoveredGroup` extension below.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps `"group_id"` / `"host_relay_url"` / `"member_count"` / `"admin_count"`
/// automatically.
struct DiscoveredGroup: Decodable, Identifiable, Equatable {
    /// The NIP-29 in-relay group id (the `["d", _]` tag value). Stable
    /// list identity inside `JoinGroupView`.
    let groupId: String
    /// The host relay this group lives on. NIP-29 identity is the pair
    /// `(host_relay_url, group_id)` — surfaced here so Swift can build a
    /// typed `GroupId` for the join action without re-supplying the URL.
    let hostRelayUrl: String
    let name: String?
    let picture: String?
    let about: String?
    let memberCount: UInt32
    let adminCount: UInt32
    let `public`: Bool
    let open: Bool

    var id: String { "\(hostRelayUrl)|\(groupId)" }
}

extension DiscoveredGroup {
    /// Display name: `name` when non-empty, `groupId` as fallback (ADR-0032).
    var displayName: String {
        if let n = name, !n.isEmpty { return n }
        return groupId
    }

    /// Two-character uppercase initials for the avatar tile (ADR-0032).
    var initials: String {
        String(displayName.prefix(2).uppercased())
    }

    /// Formatted subtitle: visibility glyph + access + pluralized member count
    /// (ADR-0032).
    var subtitle: String {
        let vis = `public` ? "# Public" : "🔒 Private"
        let acc = open ? "Open" : "Closed"
        let mem = memberCount == 1 ? "1 member" : "\(memberCount) members"
        return "\(vis) · \(acc) · \(mem)"
    }
}

/// The serialised read-model `JoinGroupView` consumes. `groups` is ordered
/// alphabetically by `groupId` by the Rust projection — Swift does not
/// re-sort.
struct DiscoveredGroupsSnapshot: Decodable, Equatable {
    /// The host relay this snapshot describes — every row's `hostRelayUrl`
    /// equals this value (the projection is single-relay scoped).
    let hostRelayUrl: String
    let groups: [DiscoveredGroup]

    static let empty = DiscoveredGroupsSnapshot(hostRelayUrl: "", groups: [])
}

// ─── NIP-29 group-create defaults read model (#626) ───────────────────────
//
// Mirror of `nmp-nip29`'s `GroupDefaultsSnapshot` — the shape the crate-owned
// `GroupDefaultsProjection` serialises under the snapshot key
// `"nmp.nip29.group_defaults"`. Thin-shell rule: the suggested public-group
// relay URL is a NIP-29 protocol fact OWNED BY RUST (the `nmp-nip29` constant
// `DEFAULT_PUBLIC_GROUP_RELAY_URL`), surfaced here so `NewGroupSheet` pre-fills
// it without hardcoding a protocol URL in the shell (issue #626). Swift only
// reads `suggestedRelayUrl` into the editable `TextField` binding.

/// The serialised read-model `NewGroupSheet` seeds its public-group relay
/// field from. `suggestedRelayUrl` is the crate-owned default; the user may
/// overwrite it before creating the group.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps the kernel's `"suggested_relay_url"` to `suggestedRelayUrl`
/// automatically.
struct GroupDefaultsSnapshot: Decodable, Equatable {
    let suggestedRelayUrl: String

    static let empty = GroupDefaultsSnapshot(suggestedRelayUrl: "")
}

// ─── NIP-57 zap aggregate read model ──────────────────────────────────────
//
// Mirror of `nmp-nip57`'s `ZapsAggregateSnapshot` / `ZapCount` — the shape
// the `ZapsAggregateProjection` serialises under the snapshot key
// `"nmp.nip57.zaps"`. Thin-shell rule: these are pure DTOs. The Rust
// projection owns ALL protocol logic — kind:9735 receipt decoding, bolt11
// amount parsing, per-target grouping, and per-receipt dedupe. Swift never
// re-derives `count` or `totalMsats` from raw events.

/// Aggregate zap totals for a single target event. `totalMsats` sums the
/// authoritative bolt11 amount of every distinct receipt indexed under the
/// target; `count` is the number of distinct receipts. A receipt whose
/// amount could not be parsed contributes `0` msats but still increments
/// `count` — the zap *happened*, the amount is just unknown.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// (inherited by every nested type) maps the kernel's `"total_msats"` to
/// `totalMsats` automatically.
struct ZapCount: Decodable, Equatable {
    let totalMsats: UInt64
    let count: UInt32
}

/// The serialised read-model a timeline-zap-count surface consumes.
/// `totals` maps a zapped event id (hex) to its running `ZapCount`. The
/// wrapper struct (rather than a bare map at the top level) mirrors the
/// Rust shape and leaves room for sibling fields without a breaking
/// re-shape.
struct ZapsAggregateSnapshot: Decodable, Equatable {
    /// `target_event_id (hex) → ZapCount`. Empty when the projection has
    /// been registered but no kind:9735 receipts have arrived yet.
    let totals: [String: ZapCount]

    static let empty = ZapsAggregateSnapshot(totals: [:])
}

// ─── NIP-17 DM relay-list read model ─────────────────────────────────────
//
// Mirror of the `DmRelayListSnapshot` the `DmRuntimeController` serialises
// under the snapshot key `"nmp.nip17.dm_relay_list"`. Thin-shell rule: pure
// DTO — the Rust side owns all kind:10050 reconciliation logic.

/// The active account's DM relay list state. `activePubkey` is the active
/// account's hex pubkey (nil when no account is loaded). `readRelayUrls`
/// is the subset of configured relay URLs eligible for DM reads.
///
/// No explicit `CodingKeys`: `.convertFromSnakeCase` maps `"active_pubkey"` →
/// `activePubkey` and `"read_relay_urls"` → `readRelayUrls` automatically.
struct DmRelayListSnapshot: Decodable, Equatable {
    let activePubkey: String?
    let readRelayUrls: [String]
}

// ─── NIP-17 DM inbox read model ───────────────────────────────────────────
//
// Mirror of `nmp-nip17`'s `DmInboxSnapshot` / `DmConversation` / `DmMessage`
// — the shape the `DmInboxProjection` serialises under the snapshot key
// `"nmp.nip17.dm_inbox"`. Thin-shell rule: these are pure DTOs. The Rust
// projection owns ALL protocol logic — NIP-44 decryption, kind:14 filtering,
// per-peer grouping, and newest-first ordering. Swift never re-sorts or
// re-groups.

/// One decrypted NIP-17 direct message. `senderPubkey` is taken from the
/// verified kind:13 seal (not a forgeable tag); `id` is the inner rumor
/// event id (hex) and the stable list identity. `isOutgoing` is pre-
/// classified by the Rust projection against the active local pubkey —
/// the shell never compares pubkeys to align a bubble (thin-shell rule).
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps `"sender_pubkey"` / `"created_at"` / `"reply_to"` / `"is_outgoing"` /
/// `"source_relays"` automatically.
struct DmMessage: Decodable, Identifiable, Equatable {
    let id: String
    let senderPubkey: String
    let content: String
    /// Event `created_at` (Unix seconds). Presentation layer formats via
    /// `relativeTimeFromUnixSeconds` (ADR-0032).
    let createdAt: UInt64
    let replyTo: String?
    let isOutgoing: Bool
    let sourceRelays: [String]?
}

/// One DM thread — every message exchanged with a single peer. `messages`
/// is ordered chronologically by the Rust projection — oldest first,
/// newest last — so the host renders a chat log in that order and never
/// reverses (thin-shell rule). The thread's most-recent message is
/// `messages.last`.
///
/// ADR-0032: only the raw peer hex pubkey crosses the FFI boundary. The
/// presentation layer formats it for display (`shortHex`,
/// `pubkeyColorHex`, `displayInitials`).
struct DmConversation: Decodable, Identifiable, Equatable {
    /// The OTHER party in the thread (hex pubkey). Also the list identity.
    let peerPubkey: String
    let messages: [DmMessage]

    var id: String { peerPubkey }
}

// ─── NIP-02 follow list read model ───────────────────────────────────────────
//
// Mirror of `nmp-app-chirp`'s `FollowListProjection` — the shape it serialises
// under the snapshot key `"nmp.follow_list"`. Follow entries carry raw pubkeys;
// Swift formats compact labels/avatars from those raw fields (ADR-0032).

/// One entry in the active account's follow list. Only the raw hex
/// `pubkey` crosses the FFI boundary; the presentation layer formats
/// the abbreviated label / avatar tint / initials locally (ADR-0032).
struct FollowEntry: Decodable, Identifiable, Equatable {
    let pubkey: String
    var id: String { pubkey }
}

/// The serialised follow-list snapshot. `follows` is the active account's
/// NIP-02 kind:3 contact list; each entry carries the raw followee pubkey.
struct FollowListSnapshot: Decodable, Equatable {
    let follows: [FollowEntry]
    static let empty = FollowListSnapshot(follows: [])
}

/// The serialised read-model the DM screens consume. `conversations` is
/// ordered by most-recent message (newest thread first) by the Rust
/// projection — Swift does not re-sort.
struct DmInboxSnapshot: Decodable, Equatable {
    let conversations: [DmConversation]
    /// ADR-0050 §D7 decrypt-pipeline policy state (errors-as-state) — the
    /// tri-state that replaced the old `remoteSignerUnsupported` bool. Stable
    /// wire tokens the host switches on:
    ///
    /// * `"unavailable"` — no active account; the host should hide the DM
    ///   screen entirely.
    /// * `"limited"` — an active account with `undecryptedCount > 0`: a bunker
    ///   backfill is pending or throttled by the bounded per-account decrypt
    ///   queue. NOT a silent drop — the host surfaces the count.
    /// * `"ok"` — an active account with everything decrypted.
    var decryptState: String
    /// ADR-0050 §D7 — count of envelopes admitted-but-not-yet-decrypted plus
    /// those not admitted because the per-account bound was full. Non-zero
    /// exactly when `decryptState == "limited"`.
    var undecryptedCount: UInt32

    static let empty = DmInboxSnapshot(conversations: [], decryptState: "unavailable", undecryptedCount: 0)

    // Custom init so the §D7 fields degrade safely when absent (an older Rust
    // build that predates Stage 5). The decoder uses `.convertFromSnakeCase`,
    // so `decrypt_state` / `undecrypted_count` map to the property names; an
    // absent `decrypt_state` decodes to "unavailable" (the safe "hide the
    // screen" default, never a misleading "ok").
    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        conversations = try c.decode([DmConversation].self, forKey: .conversations)
        decryptState = try c.decodeIfPresent(String.self, forKey: .decryptState) ?? "unavailable"
        undecryptedCount = try c.decodeIfPresent(UInt32.self, forKey: .undecryptedCount) ?? 0
    }

    init(conversations: [DmConversation], decryptState: String = "ok", undecryptedCount: UInt32 = 0) {
        self.conversations = conversations
        self.decryptState = decryptState
        self.undecryptedCount = undecryptedCount
    }

    private enum CodingKeys: String, CodingKey {
        case conversations
        case decryptState
        case undecryptedCount
    }
}

// Relay-diagnostics DTOs (RelayDiagnosticsWireSub, RelayDiagnosticsInfo,
// RelayConnectionReason, RelayDiagnosticsRow, RelayDiagnosticsInterest,
// RelayDiagnosticsSnapshot) live in KernelSnapshotTypes+RelayDiagnostics.swift
// (extracted to satisfy the 500-LOC file-size hard-cap gate, AGENTS.md).

// `AppRelay` and `RelayRoleOption` moved to
// `Generated/KernelTypes.generated.swift` (V6 Stage 1, plan §6b). Rust
// source: `nmp-core/src/kernel/identity_state.rs::AppRelay` /
// `nmp-core/src/actor/relay_roles.rs::RelayRoleOption`. The previous
// `AppRelay` carried a custom memberwise `init(url:role:roleLabel:roleTint:)`
// with defaulted last two args; no caller in the iOS shell constructed
// `AppRelay` directly (only decoded from snapshots and read fields),
// so removing the init is safe — the generated type's synthesised
// memberwise init is unused.

/// NIP-47 wallet connection status, projected from the kernel snapshot.
///
/// No explicit `CodingKeys`: the top-level `.convertFromSnakeCase` strategy
/// maps Rust snake_case (`balance_sats`, …) onto these camelCase properties
/// automatically.
///
/// RAW-DATA DOCTRINE (aim.md §2 / ADR-0032 /
/// docs/wiki/guides/shell-formatting-boundary.md): the kernel ships only raw
/// tokens. `statusLabel` / `statusTone` / `balanceSatsDisplay` are NOT on the
/// wire — the shell derives the label, tone, and formatted balance from the raw
/// `status` token + `balanceSats` (see `WalletStatusTone`). The `status_label` /
/// `status_tone` / `balance_sats_display` precompute was a regression (#623)
/// removed in the wallet_status sweep. `wallet_npub_short` was a further
/// presentation regression (#1678, D7) removed similarly — shells abbreviate
/// `walletPubkeyHex` using `.shortHex`. `isReady` / `isConnected` remain
/// pre-computed because they encode protocol semantics (a boolean predicate over
/// the status token), not display formatting.
struct WalletStatusData: Decodable, Equatable {
    /// Raw NIP-47 status token: `"connecting"` | `"ready"` | `"error"` |
    /// `"disconnected"`. The shell maps this to a label/tone itself.
    let status: String
    let relayUrl: String
    /// Wallet service pubkey, hex (64 chars). Presentation layer formats
    /// for display (ADR-0032 — bech32 / abbreviation are shell concerns).
    let walletPubkeyHex: String
    let walletNpub: String
    let balanceMsats: UInt64?
    /// Satoshi balance (= `balance_msats / 1000`). `nil` until the wallet
    /// responds to `get_balance`. Presentation layer formats for display.
    let balanceSats: Int?
    /// `status == "ready"` pre-computed in Rust.
    let isReady: Bool
    /// `status == "connecting" || status == "ready"` pre-computed in Rust.
    let isConnected: Bool

    /// Human-readable label derived locally from the raw `status` token.
    var statusLabel: String { WalletStatusTone.label(status) }
    /// Semantic tone (`"active"|"warning"|"error"|"inactive"`) derived locally
    /// from the raw `status` token; the view maps it → colour.
    var statusTone: String { WalletStatusTone.tone(status) }
}

// ─── RelayRoleOption shell-side label ─────────────────────────────────────
//
// `label` was removed from the `relay_role_options` wire (#1678, D7 —
// presentation artifact; raw-data doctrine aim.md §2 / ADR-0032). The kernel
// now ships only the raw `value` token; the shell maps it to a human-readable
// label here.

extension RelayRoleOption {
    /// Human-readable label derived from the raw `value` token.
    /// Shells own this mapping (#1678); the kernel no longer pre-renders it.
    var label: String {
        switch value {
        case "both,indexer": return "Both + Index"
        case "both":         return "Both"
        case "read":         return "Read"
        case "write":        return "Write"
        case "indexer":      return "Index"
        default:             return value
        }
    }
}

// ─── AccountSummary shell-side signer label ───────────────────────────────
//
// `signer_label` was removed from the `accounts` wire (#1712, D7/D27 —
// presentation artifact; raw-data doctrine aim.md §2 / ADR-0032). The kernel
// now ships only the raw `signerKind` token; the shell maps it to a
// human-readable label here.

extension AccountSummary {
    /// Human-readable signer label derived from the raw `signerKind` token.
    /// Shells own this mapping (#1712); the kernel no longer pre-renders it.
    var signerLabel: String {
        switch signerKind {
        case "local": return "Local key"
        case "nip46": return "NIP-46"
        default:      return signerKind
        }
    }
}
