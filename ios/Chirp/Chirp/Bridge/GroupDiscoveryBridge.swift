import Foundation
import os.log

// ─────────────────────────────────────────────────────────────────────────
// NIP-29 group-discovery + join FFI bridge.
//
// Sibling of `GroupChatBridge.swift` — the read + write sides of the NIP-29
// discover / join screen, mirroring the same `KernelHandle` extension +
// `@MainActor ObservableObject` store pattern.
//
// Thin-shell rule (Chirp): ZERO protocol logic in Swift. The Rust
// `DiscoveredGroupsProjection` owns kind:39000/39001/39002 filtering,
// replaceable-event merging, and alphabetical ordering; the
// `nmp.nip29.discover` action owns the relay-pinned `LogicalInterest`; the
// `nmp.nip29.join` action owns the kind:9021 event + tags + signing. Swift
// only marshals JSON across the FFI and mirrors the snapshot.
//
// ── Read side ─────────────────────────────────────────────────────────────
//
//   • `registerGroupDiscovery(hostRelayUrl:)` wires a
//     `DiscoveredGroupsProjection` for one host relay into the kernel. It
//     registers no handle and exports no `unregister` — the relay's group
//     catalog surfaces on every kernel snapshot under the `projections`
//     key `"nip29.discovered_groups"` (decoded by
//     `SnapshotProjections.discoveredGroups` in `KernelBridge.swift`).
//   • Single-screen scope: per the FFI contract, calling it twice
//     overwrites the snapshot key and leaks the older event observer for
//     the life of the `app`. `DiscoveredGroupsStore.registerOnce`
//     guards against a re-register on the SAME relay; switching to a new
//     relay deliberately overwrites (UX expectation: pick one at a time).
//
// ── Write side ────────────────────────────────────────────────────────────
//
//   • `discoverGroups(relayUrl:)` dispatches the `nmp.nip29.discover`
//     action — the executor pushes a host-pinned LogicalInterest for the
//     three metadata kinds (39000/39001/39002). Fire-and-forget; events
//     arrive through the next snapshot tick.
//   • `joinGroup(group:inviteCode:reason:)` dispatches the
//     `nmp.nip29.join` action — publishes a kind:9021 join request,
//     host-pinned to the group's own relay. Fire-and-forget; the relay's
//     reaction (a new kind:39002 listing the user, or no change for held
//     requests) surfaces through the next discovery snapshot.
// ─────────────────────────────────────────────────────────────────────────

private let gdLog = Logger(subsystem: "io.f7z.chirp", category: "GroupDiscoveryBridge")

// ── KernelHandle NIP-29 discovery + join extension (C-FFI surface) ────────

extension KernelHandle {
    /// Wire a NIP-29 `DiscoveredGroupsProjection` for `hostRelayUrl` into
    /// the kernel.
    ///
    /// Pure consumption — registers no handle. The relay's group catalog
    /// surfaces on every kernel snapshot under the `projections` key
    /// `"nip29.discovered_groups"`.
    ///
    /// Single-screen scope: per the FFI contract, a second call overwrites
    /// the snapshot key and leaks the prior observer. `DiscoveredGroupsStore`
    /// guards re-registration; this method itself is not idempotent.
    func registerGroupDiscovery(hostRelayUrl: String) {
        hostRelayUrl.withCString { nmp_app_chirp_register_group_discovery(raw, $0) }
        gdLog.info(
            "registered NIP-29 discovery projection for \(hostRelayUrl, privacy: .public)")
    }

    /// Dispatch a `nmp.nip29.discover` action — push the relay-pinned
    /// `LogicalInterest` for kinds 39000/39001/39002 so the kernel opens a
    /// REQ for that relay's group catalog. Fire-and-forget; the catalog
    /// surfaces through the next `nip29.discovered_groups` snapshot tick.
    /// Without a successful prior `registerGroupDiscovery` the projection
    /// is missing and the snapshot key stays nil (the executor still
    /// pushes the interest, but no Swift consumer mirrors it).
    func discoverGroups(relayUrl: String) {
        let payload: [String: Any] = ["relay_url": relayUrl]
        dispatchNip29Discovery(
            "nmp.nip29.discover", payload: payload, label: "discoverGroups")
    }

    /// Dispatch a `nmp.nip29.join` action — publish a kind:9021 join
    /// request to `group`'s host relay. Fire-and-forget; the relay's
    /// response (a new kind:39002 listing the user) surfaces through the
    /// next discovery snapshot tick.
    ///
    /// `inviteCode`, when supplied, becomes the `["code", _]` tag on the
    /// request — closed groups consume it on first use. `reason` becomes
    /// the event content; empty/missing → no content.
    func joinGroup(
        group: GroupId,
        inviteCode: String? = nil,
        reason: String? = nil
    ) {
        var payload: [String: Any] = ["group": group.jsonObject]
        if let inviteCode, !inviteCode.isEmpty {
            payload["invite_code"] = inviteCode
        }
        if let reason, !reason.isEmpty {
            payload["reason"] = reason
        }
        dispatchNip29Discovery(
            "nmp.nip29.join", payload: payload, label: "joinGroup")
    }

    /// Shared fire-and-forget marshal for the discover / join action
    /// dispatches. Encodes `payload` to JSON and routes it through
    /// `nmp_app_dispatch_action`; the returned correlation JSON is freed
    /// and ignored (outcomes surface through the next snapshot tick). D6:
    /// a JSON-encode failure degrades to a logged no-op.
    private func dispatchNip29Discovery(
        _ namespace: String, payload: [String: Any], label: String
    ) {
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            gdLog.error("\(label, privacy: .public): failed to encode action payload")
            return
        }
        json.withCString { jsonPtr in
            namespace.withCString { nsPtr in
                if let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) {
                    nmp_app_free_string(ptr)
                }
            }
        }
    }
}

// ── DiscoveredGroupsStore — projection mirror pushed by KernelModel.apply ─

/// `@MainActor` store backing `JoinGroupView`. A pure mirror of the
/// kernel's `nip29.discovered_groups` projection plus the discover / join
/// dispatchers — no Swift owns any group state, ordering, or protocol
/// decision (thin-shell rule).
///
/// Lifecycle is lazy + relay-keyed: the store starts un-registered; the
/// view's "Search" action sets the relay URL, which registers the read
/// projection (idempotent on the same URL) and immediately dispatches
/// `nmp.nip29.discover`. Switching relays overwrites the snapshot key
/// (FFI contract: single-screen scope).
@MainActor
final class DiscoveredGroupsStore: ObservableObject {
    /// The relay this store is currently scoped to. Empty / nil until the
    /// user enters one and taps Search. `groups` is `[]` while empty.
    @Published private(set) var hostRelayUrl: String = ""

    /// Alphabetically-ordered discovered groups, mirrored verbatim from the
    /// kernel projection. Ordering is owned by the Rust
    /// `DiscoveredGroupsProjection`.
    @Published private(set) var groups: [DiscoveredGroup] = []

    /// `true` between a discover dispatch and the first non-empty
    /// snapshot tick. Drives a "Searching…" indicator on the view. Cleared
    /// once any snapshot arrives (empty or not) — the relay may genuinely
    /// host zero groups.
    @Published private(set) var isSearching: Bool = false

    /// `nil` in steady state. Set to the group_id Swift just dispatched a
    /// `nmp.nip29.join` for, so `JoinGroupView` can flip the row to
    /// "Requested" until the user dismisses the screen. The relay's
    /// kind:39002 response surfaces in `groups` on the next tick; reading
    /// "joined" from that requires the active account's pubkey and is a
    /// follow-up — see the PR description.
    @Published private(set) var lastJoinedGroupId: String?

    private unowned let kernel: KernelHandle

    /// The relay URL the read projection is currently registered for.
    /// Empty until first search. Comparing against `hostRelayUrl` on the
    /// next search keeps the per-URL `registerOnce` guard correct across
    /// relay switches (the FFI overwrites the snapshot key intentionally;
    /// the guard prevents redundant calls for the SAME URL).
    private var registeredRelayUrl: String = ""

    init(kernel: KernelHandle) {
        self.kernel = kernel
    }

    /// Begin a discover session against `relayUrl`: ensure the read
    /// projection is registered for this relay (re-registering on a
    /// change is intentional — single-screen scope) and dispatch
    /// `nmp.nip29.discover`. Whitespace / empty input is dropped here
    /// (the Rust validator also rejects empty/non-wss input, but skipping
    /// the FFI round-trip is free).
    func searchGroups(relayUrl: String) {
        let trimmed = relayUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        // Switching relays: clear the prior snapshot so the view shows the
        // empty/loading state until the new relay's catalog arrives.
        if trimmed != hostRelayUrl {
            groups = []
            lastJoinedGroupId = nil
        }
        hostRelayUrl = trimmed

        if trimmed != registeredRelayUrl {
            kernel.registerGroupDiscovery(hostRelayUrl: trimmed)
            registeredRelayUrl = trimmed
        }
        isSearching = true
        kernel.discoverGroups(relayUrl: trimmed)
    }

    /// Mirror the latest kernel snapshot. Called from `KernelModel.apply`
    /// on every tick. A snapshot whose `hostRelayUrl` does not match the
    /// store's current target is ignored (we may receive one stale tick
    /// while the user is mid-switch). Empty `groups` is honoured — the
    /// relay may genuinely host none.
    func apply(snapshot: DiscoveredGroupsSnapshot?) {
        guard let snapshot else { return }
        // Ignore stale snapshots from a previous relay registration.
        guard snapshot.hostRelayUrl == hostRelayUrl else { return }
        // Mirror the rows.
        if snapshot.groups != groups {
            groups = snapshot.groups
        }
        // Clear the searching indicator on the first tick after a
        // dispatch — even when the catalog is empty (the relay returned
        // EOSE with nothing).
        if isSearching {
            isSearching = false
        }
    }

    /// Dispatch `nmp.nip29.join` for `group`. Fire-and-forget; the relay's
    /// reaction (a new kind:39002 with the user added) surfaces through a
    /// future discovery snapshot. `inviteCode` is the optional preauth code
    /// for closed groups.
    func joinGroup(_ group: DiscoveredGroup, inviteCode: String? = nil) {
        let typedGroup = GroupId(
            hostRelayUrl: group.hostRelayUrl,
            localId: group.groupId)
        kernel.joinGroup(group: typedGroup, inviteCode: inviteCode)
        lastJoinedGroupId = group.groupId
    }

    /// Clear `lastJoinedGroupId`. The view calls this when the user
    /// dismisses the join confirmation — the row reverts to its default
    /// state until the user re-taps.
    func clearLastJoinedGroupId() {
        lastJoinedGroupId = nil
    }
}
