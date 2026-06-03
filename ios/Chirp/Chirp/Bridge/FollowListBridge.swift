import Foundation
import os.log

// ─────────────────────────────────────────────────────────────────────────
// NIP-02 follow list FFI bridge.
//
// The active account's kind:3 contact list, projected in Rust and mirrored
// here for the DM compose contact picker.
//
// Thin-shell rule (Chirp): ZERO protocol logic in Swift. The Rust
// `FollowListProjection` owns kind:3 parsing, p-tag extraction, and all
// display-string computation. Swift only marshals JSON across the FFI and
// mirrors the snapshot.
//
// ── Read side ─────────────────────────────────────────────────────────────
//
//   • `registerFollowList(activePubkey:)` wires a `FollowListProjection`
//     into the kernel. The kernel's standing `account_profile_interest`
//     already fetches kind:3 for the active account — no separate interest
//     push is needed.
//   • Decrypted follows surface on every kernel snapshot under the
//     `projections` key `"nmp.follow_list"` (decoded by
//     `SnapshotProjections.followList`).
//   • `FollowListStore` re-invokes after the active account changes so the
//     active_pubkey slot in the projection is updated.
// ─────────────────────────────────────────────────────────────────────────

private let flLog = Logger(subsystem: "io.f7z.chirp", category: "FollowListBridge")

// ── KernelHandle NIP-02 follow list extension (C-FFI surface) ────────────

extension KernelHandle {
    /// Wire the `FollowListProjection` for `activePubkey` into the kernel.
    ///
    /// Pure consumption — registers no handle. The active account's follow
    /// list then surfaces on every kernel snapshot under the `projections`
    /// key `"nmp.follow_list"`.
    ///
    /// `activePubkey` sets the projection's active-account slot so the
    /// snapshot returns the correct account's follows. Pass `nil` for the
    /// startup-before-sign-in call; `FollowListStore` re-invokes with a
    /// concrete pubkey once the account is known.
    func registerFollowList(activePubkey: String?) {
        if let activePubkey {
            activePubkey.withCString { nmp_app_chirp_register_follow_list(raw, $0) }
        } else {
            nmp_app_chirp_register_follow_list(raw, nil)
        }
        flLog.info(
            "registered follow list projection (pubkey known: \(activePubkey != nil, privacy: .public))"
        )
    }
}

// ── FollowListStore — projection mirror pushed by KernelModel.apply ───────

/// `@MainActor` store backing the DM compose contact picker. A pure mirror
/// of the kernel's `nmp.follow_list` projection — no Swift owns any
/// follow-list state, ordering, or protocol decision (thin-shell rule).
@MainActor
final class FollowListStore: ObservableObject {
    /// The active account's follow list, mirrored verbatim from the kernel
    /// projection. Entries carry raw pubkeys; Swift formats labels locally.
    @Published private(set) var follows: [FollowEntry] = []

    private unowned let kernel: KernelHandle
    /// The active pubkey the projection was last registered for. `nil` until
    /// the first account is known. Re-registering only when this changes
    /// keeps `apply` (called every tick) from spamming the FFI.
    var registeredPubkey: String?

    /// Construct a store and wire its read projection into the kernel.
    /// Mirrors `DmInboxStore(kernel:)`.
    ///
    /// The initial registration passes `nil` for the active pubkey (the
    /// account is typically not yet known at construction). `apply` updates
    /// the registration once the active account surfaces.
    init(kernel: KernelHandle) {
        self.kernel = kernel
        kernel.registerFollowList(activePubkey: nil)
    }

    /// Mirror the latest kernel snapshot. When the active account becomes
    /// known or changes, re-invoke the FFI so the projection's active-pubkey
    /// slot is updated. Called from `KernelModel.apply` on every tick.
    ///
    /// `snapshot` `nil` (projection not yet wired) leaves `follows`
    /// untouched; a snapshot with an empty array clears it.
    /// `activePubkey` `nil` means no account is signed in — no re-register.
    func apply(snapshot: FollowListSnapshot?, activePubkey: String?) {
        let normalizedPubkey = (activePubkey?.isEmpty == true) ? nil : activePubkey
        if let activePubkey = normalizedPubkey,
            activePubkey != registeredPubkey
        {
            registeredPubkey = activePubkey
            // Re-invoke so the projection's active-pubkey slot points at the
            // current account. The new registration overwrites the snapshot
            // key — accepted single-screen-scope cost (same as DmInboxStore).
            kernel.registerFollowList(activePubkey: activePubkey)
        }

        guard let snapshot else { return }
        if snapshot.follows != follows {
            follows = snapshot.follows
        }
    }
}
