import SwiftUI

/// Display helpers for the remote-signer `status_tone` field (ADR-0032 / #1099).
///
/// Display decisions live in Rust: the kernel ships `status_label`
/// ("Connected", "Reconnecting…", "Waiting for approval…", "Signer
/// unavailable", "Connection failed", "Unknown") and `status_tone`
/// ("active" | "warning" | "error" | "inactive") on the typed `signer_state`
/// wire (tail-appended additive fields). `SignerStateRow` renders the label
/// verbatim and maps the tone → `Color` via [`color(forTone:)`]; no Swift-side
/// string-switch on `state` remains.
///
/// [`derivedLabel`] / [`derivedTone`] are the forward-compat fallback used ONLY
/// when decoding an older buffer that predates those fields — they re-derive
/// label/tone from the `state` token, mirroring the Rust
/// `signer_state_label_and_tone()` function byte-for-byte (D1). New buffers
/// carry the precomputed values and never hit those paths.
enum SignerStateTone {
    /// Map a pre-computed `statusTone` string to a `Color`.
    /// Vocabulary: `"active"` | `"warning"` | `"error"` | `"inactive"`.
    static func color(forTone tone: String) -> Color {
        switch tone {
        case "active":  return ChirpColor.success
        case "warning": return ChirpColor.warning
        case "error":   return ChirpColor.danger
        default:        return ChirpColor.textSecondary
        }
    }

    /// Mirror of Rust `signer_state_label_and_tone()` — label half.
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

    /// Mirror of Rust `signer_state_label_and_tone()` — tone half.
    static func derivedTone(_ state: String) -> String {
        switch state {
        case "ready", "connected":              return "active"
        case "reconnecting", "awaiting_approval": return "warning"
        case "unavailable", "failed":           return "error"
        default:                                return "inactive"
        }
    }
}
