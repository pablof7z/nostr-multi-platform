import Foundation
import os.log

private let kbLog = Logger(subsystem: "com.example.Chirp", category: "KernelBridge")

/// Thin C-FFI wrapper around the `nmp_core` static library. Mirrors
/// `ios/NmpStress/NmpStress/KernelBridge.swift` (which is the established
/// pattern). Pulse currently consumes only the timeline-reading surface; the
/// publish / sign-in / multi-account FFI is filed as T66a (see
/// `ios/NmpPulse/README.md`).
final class KernelHandle {
    private let raw: UnsafeMutableRawPointer
    private var updateSink: KernelUpdateSink?

    init() {
        raw = nmp_app_new()
    }

    deinit {
        nmp_app_set_update_callback(raw, nil, nil)
        nmp_app_free(raw)
    }

    func listen(_ handler: @escaping (KernelUpdate) -> Void) {
        let sink = KernelUpdateSink(handler: handler)
        updateSink = sink
        nmp_app_set_update_callback(raw, Unmanaged.passUnretained(sink).toOpaque(), nmpUpdateCallback)
    }

    func start(visibleLimit: UInt32 = 80, emitHz: UInt32 = 4) {
        nmp_app_start(raw, 0, visibleLimit, emitHz)
    }

    func stop() {
        nmp_app_stop(raw)
    }

    func openAuthor(pubkey: String) {
        pubkey.withCString { nmp_app_open_author(raw, $0) }
    }

    func openThread(eventID: String) {
        eventID.withCString { nmp_app_open_thread(raw, $0) }
    }

    func claimProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_claim_profile(raw, pkPtr, cidPtr)
            }
        }
    }

    func releaseProfile(pubkey: String, consumerID: String) {
        pubkey.withCString { pkPtr in
            consumerID.withCString { cidPtr in
                nmp_app_release_profile(raw, pkPtr, cidPtr)
            }
        }
    }

    // ── T66a identity / publish / multi-account / relay-edit ──────────────
    //
    // All fire-and-forget: outcomes arrive on the next snapshot via the
    // KernelUpdate `accounts` / `publishQueue` / `lastErrorToast` fields
    // (D6 — the FFI never returns a value or throws).

    func signInNsec(_ secret: String) {
        secret.withCString { nmp_app_signin_nsec(raw, $0) }
    }

    func signInBunker(_ uri: String) {
        uri.withCString { nmp_app_signin_bunker(raw, $0) }
    }

    func createAccount() {
        nmp_app_create_new_account(raw)
    }

    func switchActive(identityID: String) {
        identityID.withCString { nmp_app_switch_active(raw, $0) }
    }

    func removeAccount(identityID: String) {
        identityID.withCString { nmp_app_remove_account(raw, $0) }
    }

    func publishNote(content: String, replyToID: String?) {
        content.withCString { cPtr in
            if let replyToID {
                replyToID.withCString { rPtr in
                    nmp_app_publish_note(raw, cPtr, rPtr)
                }
            } else {
                nmp_app_publish_note(raw, cPtr, nil)
            }
        }
    }

    func react(targetEventID: String, reaction: String) {
        targetEventID.withCString { tPtr in
            reaction.withCString { rPtr in
                nmp_app_react(raw, tPtr, rPtr)
            }
        }
    }

    func follow(pubkey: String) {
        pubkey.withCString { nmp_app_follow(raw, $0) }
    }

    func unfollow(pubkey: String) {
        pubkey.withCString { nmp_app_unfollow(raw, $0) }
    }

    func addRelay(url: String, role: String) {
        url.withCString { uPtr in
            role.withCString { rPtr in
                nmp_app_add_relay(raw, uPtr, rPtr)
            }
        }
    }

    func removeRelay(url: String) {
        url.withCString { nmp_app_remove_relay(raw, $0) }
    }

    func openTimeline() {
        nmp_app_open_timeline(raw)
    }

    // ── NIP-47 Wallet Connect ────────────────────────────────────────────────

    func walletConnect(uri: String) {
        uri.withCString { nmp_app_wallet_connect(raw, $0) }
    }

    func walletDisconnect() {
        nmp_app_wallet_disconnect(raw)
    }

    func walletPayInvoice(bolt11: String, amountMsats: UInt64?) {
        bolt11.withCString { bPtr in
            if let amount = amountMsats {
                let amountStr = String(amount)
                amountStr.withCString { aPtr in
                    nmp_app_wallet_pay_invoice(raw, bPtr, aPtr)
                }
            } else {
                nmp_app_wallet_pay_invoice(raw, bPtr, nil)
            }
        }
    }

    fileprivate static func decode(pointer: UnsafePointer<CChar>) -> KernelUpdate? {
        let payload = String(cString: pointer)
        let data = Data(payload.utf8)
        guard let outer = try? JSONSerialization.jsonObject(with: data) as? [String: Any] else {
            kbLog.error("outer JSON parse failed")
            return nil
        }
        guard (outer["t"] as? String) == "snapshot" else { return nil }
        guard let inner = outer["v"] else {
            kbLog.error("snapshot missing 'v' field")
            return nil
        }
        guard let innerData = try? JSONSerialization.data(withJSONObject: inner) else {
            kbLog.error("failed to re-serialize inner JSON")
            return nil
        }
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            let result = try decoder.decode(KernelUpdate.self, from: innerData)
            kbLog.info("decoded ok rev=\(result.rev) activeAccount=\(result.activeAccount ?? "nil")")
            return result
        } catch {
            kbLog.error("decode error: \(error.localizedDescription)")
            if let preview = String(data: innerData.prefix(500), encoding: .utf8) {
                kbLog.error("JSON preview: \(preview)")
            }
            return nil
        }
    }
}

private final class KernelUpdateSink {
    let handler: (KernelUpdate) -> Void
    init(handler: @escaping (KernelUpdate) -> Void) {
        self.handler = handler
    }
}

private let nmpUpdateCallback: NmpUpdateCallback = { context, pointer in
    guard let context, let pointer else { return }
    guard let update = KernelHandle.decode(pointer: pointer) else { return }
    let sink = Unmanaged<KernelUpdateSink>.fromOpaque(context).takeUnretainedValue()
    sink.handler(update)
}

// ─── Decoded snapshot shape ────────────────────────────────────────────────
//
// Pulse uses a tighter subset of the NmpStress KernelUpdate. The kernel
// emits ALL these fields; Pulse just doesn't reference the metrics-heavy
// ones. If the kernel ever stops emitting one, JSON decode fails closed
// (the @Published model stays at its prior value) — no field is required.

struct KernelUpdate: Decodable {
    let rev: UInt64
    let running: Bool
    let relayUrl: String
    let testNpub: String
    let profile: ProfileCard
    let items: [TimelineItem]
    let metrics: KernelMetricsLite
    let relayStatuses: [RelayStatus]
    // T66a projections. Optional so a kernel that elides one (or an older
    // build) still decodes — the model keeps its prior value (D1).
    let threadView: ThreadView?
    let accounts: [AccountSummary]?
    let activeAccount: String?
    let publishQueue: [PublishQueueEntry]?
    let lastErrorToast: String?
    let relayEditRows: [RelayEditRow]?
    // NIP-47 wallet projection. Optional so older kernels still decode (D1).
    let walletStatus: WalletStatusData?
}

struct ThreadView: Decodable, Equatable {
    let focusedEventId: String
    let rootEventId: String
    let state: String
    let items: [TimelineItem]
    let previousCount: Int
    let nextCount: Int
}

struct AccountSummary: Decodable, Identifiable, Equatable {
    let id: String
    let npub: String
    let displayName: String
    let signerKind: String
    let status: String
    var isActive: Bool { status == "active" }
}

struct PublishQueueEntry: Decodable, Identifiable, Equatable {
    let eventId: String
    let kind: UInt32
    let targetRelays: Int
    let status: String
    var id: String { eventId }
}

struct RelayEditRow: Decodable, Identifiable, Equatable {
    let url: String
    let role: String
    var id: String { url }
}

/// NIP-47 wallet connection status, projected from the kernel snapshot.
struct WalletStatusData: Decodable, Equatable {
    /// `"connecting"` | `"ready"` | `"error"` | `"disconnected"`
    let status: String
    let relayUrl: String
    let walletNpub: String
    let balanceMsats: UInt64?

    var isReady: Bool { status == "ready" }
    var isConnected: Bool { status == "connecting" || status == "ready" }

    /// Balance in sats (rounded down from msats).
    var balanceSats: UInt64? {
        balanceMsats.map { $0 / 1000 }
    }
}

struct ProfileCard: Decodable, Equatable {
    let pubkey: String
    let npub: String
    let display: String
    let pictureUrl: String?
    let nip05: String
    let about: String
    let avatarInitials: String
    let avatarColor: String
    let source: String
}

struct TimelineItem: Decodable, Identifiable, Equatable, Hashable {
    let id: String
    let authorPubkey: String
    let authorDisplay: String
    let authorPictureUrl: String?
    let authorAvatarInitials: String
    let authorAvatarColor: String
    let content: String
    let contentPreview: String
    let createdAtDisplay: String
    let relayCount: UInt32
}

struct KernelMetricsLite: Decodable {
    let storedEvents: Int
    let visibleItems: Int
    let eventsRx: UInt64
    let updateSequence: UInt64
}

struct RelayStatus: Decodable, Equatable, Identifiable {
    var id: String { relayUrl }
    let role: String
    let relayUrl: String
    let connection: String
    let auth: String
    let activeWireSubscriptions: Int
    let reconnectCount: UInt32
}
