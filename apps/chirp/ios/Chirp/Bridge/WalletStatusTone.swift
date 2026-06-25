import Foundation

/// Shell-side rendering of the wallet status label, semantic tone, and
/// thousands-separated balance from the RAW NIP-47 tokens the kernel ships.
///
/// RAW-DATA DOCTRINE (aim.md §2 / ADR-0032 /
/// docs/wiki/guides/shell-formatting-boundary.md): presentation strings —
/// human-readable labels, tone vocabularies, and display-formatted numbers —
/// live in the shell, never in the Rust projection / FlatBuffers wire. The
/// typed `WalletStatus` buffer carries only the raw `status` token
/// ("connecting" | "ready" | "error" | "disconnected") and the raw
/// `balance_sats:u64`; this enum maps them to the UI. The earlier precomputed
/// `status_label` / `status_tone` / `balance_sats_display` wire fields were a
/// regression (#623) removed in the wallet_status sweep (analogous to the #1580
/// signer-state sweep).
enum WalletStatusTone {
    /// Human-readable label for the raw wire status token.
    static func label(_ wire: String) -> String {
        switch wire {
        case "connecting":   return "Connecting"
        case "ready":        return "Ready"
        case "error":        return "Error"
        case "disconnected": return "Disconnected"
        default:             return "Unknown"
        }
    }

    /// Semantic tone for the raw wire status token —
    /// "active" | "warning" | "error" | "inactive".
    static func tone(_ wire: String) -> String {
        switch wire {
        case "ready":      return "active"
        case "connecting": return "warning"
        case "error":      return "error"
        default:           return "inactive"
        }
    }

    /// Format a satoshi count with thousands separators (`12345` → `"12,345"`).
    /// Replaces the former Rust-side `format_sats_display` precompute — the
    /// shell owns balance display formatting (raw-data doctrine).
    static func formattedSats(_ sats: UInt64) -> String {
        let formatter = NumberFormatter()
        formatter.numberStyle = .decimal
        formatter.groupingSeparator = ","
        formatter.usesGroupingSeparator = true
        return formatter.string(from: NSNumber(value: sats)) ?? String(sats)
    }
}
