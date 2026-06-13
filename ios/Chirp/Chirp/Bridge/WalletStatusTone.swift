import Foundation

/// Forward-compat derivation of the wallet `status_label` / `status_tone`
/// display fields from the raw NIP-47 wire status token.
///
/// ADR-0032 / #623: display decisions live in Rust. The kernel ships
/// `status_label` ("Connecting", "Ready", "Error", "Disconnected") and
/// `status_tone` ("active" | "warning" | "error" | "inactive") on the typed
/// `WalletStatus` wire (tail-appended additive fields). These helpers are the
/// fallback used ONLY when decoding an older buffer that predates those fields
/// — they re-derive label/tone from the `status` token, mirroring the Rust
/// `nmp_nip47::status::status_label()` / `status_tone()` functions byte-for-byte
/// (D1 best-effort, fail-closed). New buffers carry the precomputed values and
/// never hit this path.
enum WalletStatusTone {
    /// Mirror of Rust `status_label()`.
    static func derivedLabel(_ wire: String) -> String {
        switch wire {
        case "connecting":   return "Connecting"
        case "ready":        return "Ready"
        case "error":        return "Error"
        case "disconnected": return "Disconnected"
        default:             return "Unknown"
        }
    }

    /// Mirror of Rust `status_tone()`.
    static func derivedTone(_ wire: String) -> String {
        switch wire {
        case "ready":      return "active"
        case "connecting": return "warning"
        case "error":      return "error"
        default:           return "inactive"
        }
    }
}
