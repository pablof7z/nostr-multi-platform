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
//   • `registerDmInbox(viewerPubkey:)` wires a `DmInboxProjection` into the
//     kernel. It registers no handle and exports no `unregister` — decrypted
//     conversations surface on every kernel snapshot under the `projections`
//     key `"nip17.dm_inbox"` (decoded by `SnapshotProjections.dmInbox`).
//   • The `viewerPubkey` is what makes the inbox LIVE rather than inert: the
//     FFI uses it to push a kind:1059 `#p` gift-wrap interest so the kernel
//     opens a REQ for incoming envelopes. `DmInboxStore` re-invokes after the
//     active account is known / changes (the interest id is per-pubkey
//     deterministic, so a re-invoke for the same account is a no-op).
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
    /// Wire the NIP-17 `DmInboxProjection` into the kernel.
    ///
    /// Pure consumption — registers no handle. Decrypted conversations then
    /// surface on every kernel snapshot under the `projections` key
    /// `"nip17.dm_inbox"`.
    ///
    /// `viewerPubkey` (the active account's hex pubkey) is forwarded so the
    /// FFI can push the kind:1059 `#p` gift-wrap inbox interest — WITHOUT it
    /// the projection is registered but inert (no REQ is opened, no envelopes
    /// arrive). Pass `nil` only for the startup-before-sign-in call;
    /// `DmInboxStore` re-invokes with a concrete pubkey once the account is
    /// known. The interest id is deterministic per-pubkey, so a re-invoke for
    /// the same account is an idempotent no-op.
    func registerDmInbox(viewerPubkey: String?) {
        if let viewerPubkey {
            viewerPubkey.withCString { nmp_app_chirp_register_dm_inbox(raw, $0) }
        } else {
            nmp_app_chirp_register_dm_inbox(raw, nil)
        }
        dmLog.info(
            "registered NIP-17 DM inbox (pubkey known: \(viewerPubkey != nil, privacy: .public))"
        )
    }

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

    /// Dispatch a `nmp.nip17.publish_relay_list` action — publish the active
    /// account's kind:10050 NIP-17 DM-relay list so other clients can
    /// discover where to send the user gift-wrapped DMs.
    ///
    /// `relays` is the user's DM-inbox relay set (per NIP-17 § 2: the relays
    /// where the user wants to *receive* DMs — i.e. read-eligible relays).
    /// Routes through the generic `nmp_app_dispatch_action` path; the
    /// kind:10050 event build, URL canonicalization, signing, and NIP-65
    /// outbox routing are all owned by Rust (thin-shell rule).
    ///
    /// Fire-and-forget: the returned correlation JSON is freed and ignored —
    /// the Rust action rejects an empty relay set (publishing zero `relay`
    /// tags would CLEAR the cache on every ingesting peer), so callers that
    /// might race here should keep their own "have not yet computed a non-
    /// empty set" guard. `KernelModel.maybePublishDmRelayList` does exactly
    /// that.
    func publishDmRelayList(relays: [String]) {
        let payload: [String: Any] = ["relays": relays]
        guard
            let data = try? JSONSerialization.data(withJSONObject: payload),
            let json = String(data: data, encoding: .utf8)
        else {
            dmLog.error("publishDmRelayList: failed to encode action payload")
            return
        }
        json.withCString { jsonPtr in
            "nmp.nip17.publish_relay_list".withCString { nsPtr in
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
    /// The viewer pubkey the kind:1059 interest was last pushed for. `nil`
    /// until the first account is known. Re-pushing only when this changes
    /// keeps `apply` (called every tick) from spamming the FFI.
    private var registeredPubkey: String?

    /// Construct a store and wire its read projection into the kernel.
    /// Mirrors `GroupChatStore(groupId:kernel:)` — `KernelModel` owns the
    /// single `KernelHandle` and constructs this lazily.
    ///
    /// The initial registration passes `nil` for the viewer pubkey (the
    /// account is typically not yet known at construction). `apply` pushes
    /// the kind:1059 interest once the active account surfaces.
    init(kernel: KernelHandle) {
        self.kernel = kernel
        kernel.registerDmInbox(viewerPubkey: nil)
    }

    /// Mirror the latest kernel snapshot and, when the active account becomes
    /// known or changes, re-invoke the FFI so the kind:1059 gift-wrap
    /// interest is pushed for that account. Called from `KernelModel.apply`
    /// on every tick.
    ///
    /// `snapshot` `nil` (projection not yet wired) leaves `conversations`
    /// untouched; an empty array clears it. `activePubkey` `nil` means no
    /// account is signed in — no interest to push.
    ///
    /// `activePubkey` is used ONLY to drive the kind:1059 interest push; it
    /// is NOT mirrored as state for the views. Per-message outgoing vs
    /// incoming classification arrives pre-computed on `DmMessage.isOutgoing`
    /// (thin-shell rule — the shell never compares pubkeys to decide).
    func apply(snapshot: DmInboxSnapshot?, activePubkey: String?) {
        let normalizedPubkey = (activePubkey?.isEmpty == true) ? nil : activePubkey
        if let activePubkey = normalizedPubkey,
            activePubkey != registeredPubkey
        {
            registeredPubkey = activePubkey
            // Re-invoke so the FFI pushes the kind:1059 `#p` interest. The
            // interest id is deterministic per-pubkey; the projection's
            // snapshot key is simply overwritten with an equivalent
            // projection — accepted single-screen-scope cost.
            kernel.registerDmInbox(viewerPubkey: activePubkey)
        }

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
