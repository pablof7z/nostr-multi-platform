import Foundation
import os.log

// ─────────────────────────────────────────────────────────────────────────
// NIP-29 group-chat FFI bridge.
//
// First real consumer of the NIP-29 seam. Mirrors `MarmotBridge.swift` /
// `ModularTimelineBridge.swift`: a thin `KernelHandle` extension that owns
// the C-FFI surface, plus an `@MainActor ObservableObject` store
// (`GroupChatStore`) fed by `KernelModel.apply`.
//
// Thin-shell rule (Chirp): ZERO protocol logic in Swift. The Rust
// `GroupChatProjection` owns ingest filtering and newest-first ordering;
// the `nip29.post_chat_message` action owns the kind:9 event, its tags,
// and signing. Swift only marshals JSON across the FFI and mirrors the
// snapshot.
//
// ── Read side ─────────────────────────────────────────────────────────────
//
//   • `registerGroupChat(groupId:)` wires a `GroupChatProjection` for one
//     group into the kernel. It registers no handle and exports no
//     `unregister` — the group's messages surface on every kernel snapshot
//     under the `projections` key `"nip29.group_chat"` (decoded by
//     `SnapshotProjections.groupChat` in `KernelBridge.swift`).
//   • Single-screen scope: per the FFI contract, calling it twice
//     overwrites the snapshot key and leaks the older event observer for
//     the life of the `app`. Chirp registers exactly one group per run, so
//     `GroupChatStore.registerOnce` guards against a re-register.
//
// ── Write side ────────────────────────────────────────────────────────────
//
//   • `postChatMessage(groupId:content:)` dispatches the
//     `nip29.post_chat_message` action through the generic
//     `nmp_app_dispatch_action` path. Fire-and-forget — the outcome
//     surfaces through the next snapshot tick (matches `react` / `follow`).
// ─────────────────────────────────────────────────────────────────────────

private let gcLog = Logger(subsystem: "io.f7z.chirp", category: "GroupChatBridge")

// ── GroupId — the typed NIP-29 group identity ────────────────────────────

/// NIP-29 group identity: the host relay URL plus the in-relay local id.
///
/// Mirrors the Rust `nmp_nip29::GroupId`. The wire JSON is snake_case
/// (`host_relay_url` / `local_id`); Swift call sites use camelCase and the
/// `jsonObject` computed property does the marshalling.
struct GroupId: Hashable, Equatable {
    /// A `wss://` host relay URL.
    let hostRelayUrl: String
    /// The in-relay local id — NIP-29 charset `[a-z0-9-_]+`.
    let localId: String

    /// The exact JSON object shape the Rust `GroupId` deserializes from.
    /// snake_case keys are mandatory — the Rust struct is plain `serde`,
    /// not `.convertFromSnakeCase`-decoded.
    var jsonObject: [String: String] {
        ["host_relay_url": hostRelayUrl, "local_id": localId]
    }
}

// ── KernelHandle NIP-29 group-chat extension (C-FFI surface) ──────────────

extension KernelHandle {
    /// Wire a NIP-29 `GroupChatProjection` for `groupId` into the kernel.
    ///
    /// Pure consumption — registers no handle. The group's chat messages
    /// then surface on every kernel snapshot under the `projections` key
    /// `"nip29.group_chat"`. D6: a JSON-encode failure degrades to a
    /// logged no-op; the Rust side likewise no-ops on a null / malformed
    /// argument.
    ///
    /// Single-screen scope: per the FFI contract, a second call overwrites
    /// the snapshot key and leaks the prior observer. `GroupChatStore`
    /// guards re-registration; this method itself is not idempotent.
    func registerGroupChat(groupId: GroupId) {
        guard
            let data = try? JSONSerialization.data(withJSONObject: groupId.jsonObject),
            let json = String(data: data, encoding: .utf8)
        else {
            gcLog.error("registerGroupChat: failed to encode GroupId JSON")
            return
        }
        json.withCString { nmp_app_chirp_register_group_chat(raw, $0) }
        gcLog.info("registered NIP-29 group chat projection for \(groupId.localId, privacy: .public)")
    }

    /// Dispatch a `nip29.post_chat_message` action — publish a kind:9 group
    /// chat message. Routes through the generic `nmp_app_dispatch_action`
    /// path; the kind:9 event, its `["h", local_id]` tag, and signing are
    /// all owned by Rust (thin-shell rule). Fire-and-forget: the returned
    /// correlation JSON is freed and ignored — the published message
    /// surfaces through the next `nip29.group_chat` snapshot tick (matches
    /// the `react` / `follow` / `publishNote` pattern).
    func postChatMessage(groupId: GroupId, content: String) {
        let payload: [String: Any] = [
            "group": groupId.jsonObject,
            "content": content,
        ]
        dispatchNip29("nip29.post_chat_message", payload: payload, label: "postChatMessage")
    }

    /// Dispatch a `nip29.react_in_group` action — publish a kind:7 in-group
    /// reaction to `eventId`. Routes through the generic
    /// `nmp_app_dispatch_action` path; the kind:7 event, its `["h", local_id]`
    /// / `["e", target]` / `["p", author]` tags, and signing are all owned by
    /// the Rust `ReactInGroupAction` (thin-shell rule). Fire-and-forget — the
    /// reaction surfaces through the next snapshot tick.
    ///
    /// `reaction` is the kind:7 content (defaults to `"❤️"`); `eventAuthorPubkey`,
    /// when supplied, becomes the `["p", _]` tag so the reaction notifies the
    /// reacted-to author (NIP-25 hygiene).
    func reactToMessage(
        groupId: GroupId,
        eventId: String,
        reaction: String = "❤️",
        eventAuthorPubkey: String? = nil
    ) {
        var payload: [String: Any] = [
            "group": groupId.jsonObject,
            "target_event_id": eventId,
            "content": reaction,
        ]
        if let eventAuthorPubkey {
            payload["target_author_pubkey"] = eventAuthorPubkey
        }
        dispatchNip29("nip29.react_in_group", payload: payload, label: "reactToMessage")
    }

    /// Dispatch a `nip29.comment_in_group` action — publish a kind:1111 in-group
    /// comment that replies to `replyToEventId`. Routes through the generic
    /// `nmp_app_dispatch_action` path; the kind:1111 event, its `["h", local_id]`
    /// / `["e", parent]` tags, and signing are all owned by the Rust
    /// `CommentInGroupAction` (thin-shell rule). Fire-and-forget — the comment
    /// surfaces through the next snapshot tick.
    ///
    /// `replyToEventId` maps to `parent_event_id`; `root_event_id` is left
    /// unset — Chirp tracks no thread root (a flat one-level reply is the
    /// scope of this screen).
    func replyToMessage(groupId: GroupId, replyToEventId: String, content: String) {
        let payload: [String: Any] = [
            "group": groupId.jsonObject,
            "parent_event_id": replyToEventId,
            "content": content,
        ]
        dispatchNip29("nip29.comment_in_group", payload: payload, label: "replyToMessage")
    }

    /// Shared fire-and-forget marshal for a NIP-29 action dispatch. Encodes
    /// `payload` to JSON and routes it through `nmp_app_dispatch_action`; the
    /// returned correlation JSON is freed and ignored (the outcome surfaces
    /// through the next snapshot tick). D6: a JSON-encode failure degrades to
    /// a logged no-op.
    private func dispatchNip29(_ namespace: String, payload: [String: Any], label: String) {
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            gcLog.error("\(label, privacy: .public): failed to encode action payload")
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

// ── GroupChatStore — projection mirror pushed by KernelModel.apply ────────

/// `@MainActor` store backing `GroupChatView`. A pure mirror of the kernel's
/// `nip29.group_chat` projection plus a thin send wrapper — no Swift owns
/// any chat state, ordering, or protocol decision (thin-shell rule).
@MainActor
final class GroupChatStore: ObservableObject {
    /// The group this store reads and posts to.
    let groupId: GroupId

    /// Newest-first chat messages, mirrored verbatim from the kernel
    /// projection. Ordering is owned by the Rust `GroupChatProjection`.
    @Published private(set) var messages: [GroupChatMessage] = []

    private unowned let kernel: KernelHandle
    /// Guards against a second `nmp_app_chirp_register_group_chat` call —
    /// the FFI has single-screen scope and a re-register leaks an observer.
    private var registered = false

    /// Construct a store for `groupId` and wire its read projection into
    /// the kernel. Mirrors `MarmotStore(kernel:)` — `KernelModel` owns the
    /// single `KernelHandle` and constructs this lazily.
    init(groupId: GroupId, kernel: KernelHandle) {
        self.groupId = groupId
        self.kernel = kernel
        registerOnce()
    }

    /// Register the read projection exactly once. Re-entry is a no-op so a
    /// `KernelModel` reset that re-pushes snapshots cannot double-register
    /// (the FFI contract: a second call leaks the prior observer).
    private func registerOnce() {
        guard !registered else { return }
        registered = true
        kernel.registerGroupChat(groupId: groupId)
    }

    /// Mirror the latest kernel snapshot. Called from `KernelModel.apply`
    /// on every tick. `nil` (projection not yet wired / older kernel)
    /// leaves `messages` untouched; an empty array clears it.
    func apply(snapshot: GroupChatSnapshot?) {
        guard let snapshot else { return }
        if snapshot.messages != messages {
            messages = snapshot.messages
        }
    }

    /// Publish a chat message to the group. Fire-and-forget — the sent
    /// message reappears through the next snapshot tick. Empty / whitespace
    /// content is dropped here (the Rust action also rejects empty content,
    /// but skipping the FFI round-trip is free).
    ///
    /// When `replyToEventId` is supplied, this routes to the
    /// `nip29.comment_in_group` action (a kind:1111 reply) instead of a plain
    /// kind:9 chat message — the reply still surfaces in this group's stream.
    /// The verb choice is the only Swift-side branch; the event kind, tags,
    /// and signing remain Rust-owned (thin-shell rule).
    func sendMessage(_ content: String, replyToEventId: String? = nil) {
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        if let replyToEventId {
            kernel.replyToMessage(
                groupId: groupId, replyToEventId: replyToEventId, content: trimmed)
        } else {
            kernel.postChatMessage(groupId: groupId, content: trimmed)
        }
    }

    /// React to a group message — publish a kind:7 in-group reaction to
    /// `eventId`. Fire-and-forget; the reaction surfaces through the next
    /// snapshot tick. The reaction content defaults to `"❤️"`.
    ///
    /// `eventAuthorPubkey`, when supplied, becomes the kind:7 `["p", _]` tag
    /// so the reaction notifies the message author (NIP-25 hygiene). The view
    /// passes the pubkey it already renders; no protocol decision is made in
    /// Swift (thin-shell rule).
    func reactToMessage(
        eventId: String, reaction: String = "❤️", eventAuthorPubkey: String? = nil
    ) {
        kernel.reactToMessage(
            groupId: groupId,
            eventId: eventId,
            reaction: reaction,
            eventAuthorPubkey: eventAuthorPubkey)
    }
}
