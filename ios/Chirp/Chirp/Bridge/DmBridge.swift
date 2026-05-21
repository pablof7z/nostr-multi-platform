import Foundation
import os.log

// ─────────────────────────────────────────────────────────────────────────
// NIP-17 private direct-message FFI bridge.
//
// The receive + send halves of NIP-17 private DMs. Mirrors
// `GroupChatBridge.swift`: a thin `KernelHandle` extension owning the C-FFI
// surface, plus an `@MainActor ObservableObject` store (`DmInboxStore`) fed
// by `KernelModel.apply`.
//
// Thin-shell rule (Chirp): ZERO protocol logic in Swift. The Rust
// `DmInboxProjection` owns NIP-44 decryption, kind:14 filtering, per-peer
// grouping, and newest-first ordering; the `nmp.nip17.send` action owns the
// kind:14 rumor, the NIP-59 gift-wrap, and signing. Swift only marshals JSON
// across the FFI and mirrors the snapshot.
//
// ── Read side ─────────────────────────────────────────────────────────────
//
//   • `nmp_app_chirp_register` wires the Rust DM runtime eagerly. Decrypted
//     conversations surface on every kernel snapshot under the `projections`
//     key `"nip17.dm_inbox"` (decoded by `SnapshotProjections.dmInbox`).
//   • Rust owns the active account's kind:1059 `#p` gift-wrap interest and
//     kind:10050 DM-relay-list publish policy. The Swift store only mirrors
//     snapshots.
//
// ── Write side ────────────────────────────────────────────────────────────
//
//   • `sendDm(recipientPubkey:content:replyTo:)` dispatches the `nmp.nip17.send`
//     action through the generic `nmp_app_dispatch_action` path.
//     Fire-and-forget — the sent message reappears through the next snapshot
//     tick (the actor gift-wraps a self-copy to the sender).
// ─────────────────────────────────────────────────────────────────────────

private let dmLog = Logger(subsystem: "io.f7z.chirp", category: "DmBridge")

// ── KernelHandle NIP-17 DM extension (C-FFI surface) ─────────────────────

extension KernelHandle {
    /// Dispatch a `nmp.nip17.send` action — send a NIP-17 private direct message
    /// to `recipientPubkey`. Routes through the generic
    /// `nmp_app_dispatch_action` path; the kind:14 rumor, the NIP-59
    /// gift-wrap, and signing are all owned by Rust (thin-shell rule).
    /// Fire-and-forget: the returned correlation JSON is freed and ignored —
    /// the sent message surfaces through the next `nip17.dm_inbox` snapshot
    /// tick (the actor gift-wraps a self-copy).
    ///
    /// `replyTo`, when supplied, is the event id this message replies to; the
    /// Rust action adds the NIP-10 reply marker.
    func sendDm(recipientPubkey: String, content: String, replyTo: String? = nil) {
        var payload: [String: Any] = [
            "recipient_pubkey": recipientPubkey,
            "content": content,
        ]
        if let replyTo {
            payload["reply_to"] = replyTo
        }
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            dmLog.error("sendDm: failed to encode action payload")
            return
        }
        json.withCString { jsonPtr in
            "nmp.nip17.send".withCString { nsPtr in
                if let ptr = nmp_app_dispatch_action(raw, nsPtr, jsonPtr) {
                    nmp_app_free_string(ptr)
                }
            }
        }
    }

}

// ── DmInboxStore — projection mirror pushed by KernelModel.apply ─────────

/// `@MainActor` store backing `DmListView` / `DmConversationView`. A pure
/// mirror of the kernel's `nip17.dm_inbox` projection plus a thin send
/// wrapper — no Swift owns any DM state, ordering, decryption, or protocol
/// decision (thin-shell rule).
@MainActor
final class DmInboxStore: ObservableObject {
    /// Conversations, newest-thread-first, mirrored verbatim from the kernel
    /// projection. Ordering and grouping are owned by the Rust
    /// `DmInboxProjection`. Within each conversation, `messages` is in
    /// chronological order — oldest first, newest last.
    @Published private(set) var conversations: [DmConversation] = []

    private unowned let kernel: KernelHandle

    /// Construct a store and wire its read projection into the kernel.
    /// Mirrors `GroupChatStore(groupId:kernel:)` — `KernelModel` owns the
    /// single `KernelHandle` and constructs this lazily.
    init(kernel: KernelHandle) {
        self.kernel = kernel
    }

    /// Mirror the latest kernel snapshot. `snapshot` `nil` leaves
    /// `conversations` untouched; an empty array clears it.
    func apply(snapshot: DmInboxSnapshot?) {
        guard let snapshot else { return }
        if snapshot.conversations != conversations {
            conversations = snapshot.conversations
        }
    }

    /// Send a NIP-17 direct message to `recipientPubkey`. Fire-and-forget —
    /// the sent message reappears through the next snapshot tick. Empty /
    /// whitespace content is dropped here (the Rust action also rejects it,
    /// but skipping the FFI round-trip is free).
    func sendDm(to recipientPubkey: String, content: String, replyTo: String? = nil) {
        let trimmed = content.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, !recipientPubkey.isEmpty else { return }
        kernel.sendDm(recipientPubkey: recipientPubkey, content: trimmed, replyTo: replyTo)
    }
}
