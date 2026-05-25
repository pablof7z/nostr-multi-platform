import Foundation
import os.log

// ── Snapshot decode ──────────────────────────────────────────────────────

/// Mirror of `KERNEL_SCHEMA_VERSION` (Rust: M0.A snapshot.rs).
/// Must match the `schema_version` field in the stub payload.
/// A mismatch causes the snapshot to be dropped rather than misparsed.
private let PODCAST_SCHEMA_VERSION: UInt32 = 1

/// Typed envelope for the outer `{"t":…,"v":{…}}` wire frame.
private struct PodcastSnapshotEnvelope: Decodable {
    let t: String
    let v: PodcastUpdate
}

// ─── Swift-side timing wrapper ────────────────────────────────────────────

struct KernelUpdateResult {
    let update: PodcastUpdate
    let payloadBytes: Int
    let callbackReceivedAt: ContinuousClock.Instant
    let decodeMicros: Int
}

// ─── Decode ───────────────────────────────────────────────────────────────

extension KernelHandle {
    /// Decode a nul-terminated C string into a `KernelUpdateResult`.
    /// Returns `nil` on parse failure; the update channel discards that frame.
    static func decode(pointer: UnsafePointer<CChar>) -> KernelUpdateResult? {
        let start = ContinuousClock.now
        let payload = String(cString: pointer)
        let data = Data(payload.utf8)
        let decoder = JSONDecoder()
        decoder.keyDecodingStrategy = .convertFromSnakeCase
        do {
            let envelope = try decoder.decode(PodcastSnapshotEnvelope.self, from: data)
            guard envelope.t == "snapshot" else {
                kbLog.error("unknown envelope tag=\(envelope.t) bytes=\(data.count)")
                return nil
            }
            let update = envelope.v
            guard update.schemaVersion == PODCAST_SCHEMA_VERSION else {
                kbLog.error("schema version mismatch: kernel=\(update.schemaVersion) host=\(PODCAST_SCHEMA_VERSION) — snapshot rejected")
                return nil
            }
            let duration = start.duration(to: .now)
            kbLog.info("decoded ok rev=\(update.rev) running=\(update.running)")
            return KernelUpdateResult(
                update: update,
                payloadBytes: data.count,
                callbackReceivedAt: start,
                decodeMicros: duration.microseconds
            )
        } catch {
            kbLog.error("envelope decode error: \(error.localizedDescription) bytes=\(data.count)")
            return nil
        }
    }
}

// ─── Duration.microseconds helper ────────────────────────────────────────

extension Duration {
    var microseconds: Int {
        let (seconds, attoseconds) = components
        return Int(seconds) * 1_000_000 + Int(attoseconds / 1_000_000_000_000)
    }
}
