import SwiftUI

/// Authoritative shell renderer for the remote-signer display label + tone
/// (#1493 P9 — labels-to-shells, mirrors #1568; aim.md:62).
///
/// The typed `signer_state` wire carries only the raw `state` token and the
/// `is_*` flags; Rust no longer ships any pre-formatted English. This enum owns
/// the presentation: [`derivedLabel`] maps `state` → the English label
/// ("Connected", "Reconnecting…", "Waiting for approval…", "Signer unavailable",
/// "Connection failed", "Unknown") and [`derivedTone`] maps `state` → a semantic
/// tone ("active" | "warning" | "error" | "inactive"), which [`color(forTone:)`]
/// turns into a `Color`. The Android peer (`TypedSignerStateDecoder.deriveStatus*`)
/// mirrors this table.
enum SignerStateTone {
    /// Map a derived `tone` string to a `Color`.
    /// Vocabulary: `"active"` | `"warning"` | `"error"` | `"inactive"`.
    static func color(forTone tone: String) -> Color {
        switch tone {
        case "active":  return ChirpColor.success
        case "warning": return ChirpColor.warning
        case "error":   return ChirpColor.danger
        default:        return ChirpColor.textSecondary
        }
    }

    /// Shell renderer: the English label for a raw `state` token (#1493 P9).
    static func derivedLabel(_ state: String) -> String {
        switch state {
        case "ready", "connected": return "Connected"
        case "reconnecting":       return "Reconnecting…"
        case "awaiting_approval":  return "Waiting for approval…"
        case "unavailable":        return "Signer unavailable"
        case "failed":             return "Connection failed"
        default:                   return "Unknown"
        }
    }

    /// Shell renderer: the semantic tone for a raw `state` token (#1493 P9).
    static func derivedTone(_ state: String) -> String {
        switch state {
        case "ready", "connected":              return "active"
        case "reconnecting", "awaiting_approval": return "warning"
        case "unavailable", "failed":           return "error"
        default:                                return "inactive"
        }
    }
}
