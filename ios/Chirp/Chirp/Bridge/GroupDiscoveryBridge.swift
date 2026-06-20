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
//   • `openGroupDiscovery(hostRelayUrl:)` opens a scoped group-discovery
//     session, returning an opaque handle. The relay's group catalog surfaces
//     under `"nmp.nip29.discovered_groups"` on every snapshot tick.
//   • `closeGroupDiscovery(_:)` tears down the session: unregisters the
//     observer and removes the snapshot projection.
//   • `DiscoveredGroupsStore` holds the live handle and closes it on relay
//     switch or deinit.
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
    /// Open a NIP-29 group-discovery session for `hostRelayUrl`.
    ///
    /// Returns an opaque handle the caller MUST pass to
    /// `closeGroupDiscovery(_:)` when the session ends (screen dismissed or
    /// relay switched). Returns `nil` when the relay URL is empty or
    /// registration fails (D6).
    func openGroupDiscovery(hostRelayUrl: String) -> OpaquePointer? {
        guard !hostRelayUrl.isEmpty else { return nil }
        let ptr = hostRelayUrl.withCString {
            nmp_app_chirp_open_group_discovery(raw, $0)
        }
        guard let ptr else { return nil }
        gdLog.info("opened NIP-29 discovery session for \(hostRelayUrl, privacy: .public)")
        return OpaquePointer(ptr)
    }

    /// Close a group-discovery session previously opened with
    /// `openGroupDiscovery(hostRelayUrl:)`.
    ///
    /// Unregisters the observer and removes the snapshot projection so no
    /// stale group catalog is emitted after the session ends. The `handle`
    /// MUST NOT be used after this call. A nil handle is a no-op.
    func closeGroupDiscovery(_ handle: OpaquePointer?) {
        guard let handle else { return }
        nmp_app_chirp_close_group_discovery(UnsafeMutableRawPointer(handle))
        gdLog.info("closed NIP-29 discovery session")
    }

    /// Dispatch a `nmp.nip29.discover` action — push the relay-pinned
    /// `LogicalInterest` for kinds 39000/39001/39002 so the kernel opens a
    /// REQ for that relay's group catalog. Fire-and-forget; the catalog
    /// surfaces through the next `nmp.nip29.discovered_groups` snapshot tick.
    /// Without a successful prior `openGroupDiscovery` the projection is
    /// missing and the snapshot key stays nil (the executor still pushes the
    /// interest, but no Swift consumer mirrors it).
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
                    nmp_free_string(ptr)
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
/// Lifecycle is handle-keyed: on the first search against a relay
/// `openGroupDiscovery` is called to register the read projection and a
/// handle is stored here. On relay switch the old handle is closed before
/// opening a new one so there is never a bounded observer leak.
@MainActor
final class DiscoveredGroupsStore: ObservableObject {
    /// The relay this store is currently scoped to. Empty until the user
    /// enters one and taps Search. `groups` is `[]` while empty.
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
    /// "Requested" until the user dismisses the screen.
    @Published private(set) var lastJoinedGroupId: String?

    private unowned let kernel: KernelHandle

    /// The opaque Rust handle for the currently-open discovery session.
    /// `nil` until the user first searches. Closed on relay switch or deinit.
    private var discoveryHandle: OpaquePointer?

    init(kernel: KernelHandle) {
        self.kernel = kernel
    }

    deinit {
        // Close the Rust-side discovery session when the store is torn down
        // so the event observer and snapshot projection are reclaimed.
        if let handle = discoveryHandle {
            kernel.closeGroupDiscovery(handle)
        }
    }

    /// Begin a discover session against `relayUrl`: open the read projection
    /// for this relay (closing any prior session) and dispatch
    /// `nmp.nip29.discover`. Whitespace / empty input is dropped here
    /// (the Rust validator also rejects empty/non-wss input).
    func searchGroups(relayUrl: String) {
        let trimmed = relayUrl.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        // Switching relays: close the prior session and clear the snapshot
        // so the view shows the empty/loading state until the new relay's
        // catalog arrives.
        if trimmed != hostRelayUrl {
            kernel.closeGroupDiscovery(discoveryHandle)
            discoveryHandle = nil
            groups = []
            lastJoinedGroupId = nil
        }
        hostRelayUrl = trimmed

        // Open a session for this relay when none is held yet (first search
        // or after a relay switch above).
        if discoveryHandle == nil {
            discoveryHandle = kernel.openGroupDiscovery(hostRelayUrl: trimmed)
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
        if snapshot.groups != groups {
            groups = snapshot.groups
        }
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
