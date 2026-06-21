import Foundation

// Remote-signer health DTOs (NIP-46 bunker handshake, unified signer state,
// NIP-46 onboarding) for the KernelBridge FFI seam. Extracted from
// `KernelBridge.swift` so the bridge file holds only `KernelHandle` (file-size
// hard-cap separation). Pure DTOs; same-module Swift files see each other
// without import.

/// NIP-46 (`bunker://`) handshake progress, projected from the kernel snapshot
/// under `projections["bunker_handshake"]`. Stage values: `"connecting"`,
/// `"awaiting_pubkey"`, `"ready"`, `"failed"`, `"idle"`. `message` is a
/// human-readable progress / error hint.
///
/// **Prefer `Nip46Onboarding` for the onboarding UI**: that projection carries
/// the typed `stageKind` enum + pre-computed `isInFlight` / `isFailed` /
/// `isTerminalSuccess` / `canCancel` flags. For the `AccountsView` "Add
/// account" sheet (and any other site that already reads
/// `model.bunkerHandshake`), the same flags are now mirrored on this struct
/// too: doctrine §6 anti-pattern #1 + RMP bible commandment #4 — shells
/// render fields directly instead of switching on the raw `stage` string.
///
/// The flag fields are optional so an older kernel build that predates the
/// doctrine fix still decodes (D1); call sites that fall back to `stage` are
/// correct (but should migrate once the kernel rebuild lands).
struct BunkerHandshake: Decodable, Equatable {
    let stage: String
    let message: String?
    /// `stage == "idle"` (computed Rust-side; absent on legacy kernels).
    let isIdle: Bool?
    /// `stage` is one of `"connecting"` / `"awaiting_pubkey"`. Drives the
    /// spinner vs. terminal-icon swap and input-disabled gates.
    let isInFlight: Bool?
    /// `stage == "failed"`. Drives the red triangle + "Retry" button label.
    let isFailed: Bool?
    /// `stage == "ready"`. Drives the green check on the progress row.
    let isTerminalSuccess: Bool?
    /// True when the handshake can be cancelled (i.e. mid-flight). Drives
    /// the visibility of the "Cancel handshake" button.
    let canCancel: Bool?
}

extension BunkerHandshake {
    /// Shell-derived English label for `stage` (#1493 P9 — labels-to-shells,
    /// mirrors #1568; aim.md:62). The wire carries only the raw `stage` token;
    /// this presentation table replaces the deleted Rust `stage_label_for`.
    /// The bunker-handshake row is iOS-only today, so there is no Android peer.
    var stageLabel: String {
        switch stage {
        case "connecting":      return "Connecting to bunker relays…"
        case "awaiting_pubkey": return "Awaiting bunker approval…"
        case "ready":           return "Connected"
        case "failed":          return "Bunker handshake failed"
        case "idle":            return "Idle"
        default:                return stage
        }
    }
}

/// Unified remote-signer health — `projections["signer_state"]`.
///
/// ADR-0048 D6: the unified remote-signer health surface (generalises the
/// former `BunkerConnectionState`, V-14 / #963 / #1098). Covers BOTH NIP-46
/// bunker sessions (relay-socket health) and NIP-55 external-signer (Amber)
/// sessions (app availability / Intent approval state). Distinct from
/// `BunkerHandshake` (which tracks the NIP-46 connect / get_public_key
/// handshake progress). Nil when no remote-signer session is active (the
/// projection contributes JSON `null` — local-key accounts).
///
/// Rust pre-computes every flag so shells never string-compare `state`
/// (aim.md §6 / AP1). `isReady` drives the green indicator;
/// `isAwaitingApproval` drives a "Waiting for Amber…" inline affordance;
/// `isReconnecting` drives an amber reconnecting badge; `isUnavailable` and
/// `isFailed` drive a red re-auth prompt.
///
/// #1493 P9 (labels-to-shells): the display label + tone are NOT on the wire —
/// they are derived here from the raw `state` token via `SignerStateTone`, the
/// authoritative shell renderer (Android peer: `TypedSignerStateDecoder`).
///
/// `Decodable` for the JSON fallback path; `Equatable` for `@Published` diffing
/// so SwiftUI re-renders only on real state changes.
struct SignerState: Decodable, Equatable {
    /// `"nip46"` | `"nip55"` | `"local"`. Picks the row icon/label copy.
    let signerKind: String
    /// `"ready"`|`"awaiting_approval"`|`"reconnecting"`|`"unavailable"`|`"failed"`.
    /// Verbatim from `SignerStateDto::state`. Prefer the typed flags below.
    let state: String
    /// Optional human-readable reason (error message on degraded states).
    let reason: String?
    /// `state == "ready"`. Green indicator.
    let isReady: Bool
    /// `state == "awaiting_approval"` — NIP-55 Intent round-trip in flight.
    let isAwaitingApproval: Bool
    /// `state == "reconnecting"` — transient NIP-46 flap, auto-reconnect.
    let isReconnecting: Bool
    /// `state == "unavailable"` — NIP-55 signer app missing. Prompt re-auth.
    let isUnavailable: Bool
    /// `state == "failed"` — permanent error, session bricked. Prompt re-auth.
    let isFailed: Bool
}

extension SignerState {
    /// Shell-derived display label for `state` (#1493 P9). Rendered verbatim by
    /// `SignerStateRow`; mirrors the Android `TypedSignerStateDecoder` table.
    var statusLabel: String { SignerStateTone.derivedLabel(state) }
    /// Shell-derived tone for `state` (#1493 P9) — "active"|"warning"|"error"|
    /// "inactive". Mapped to a `Color` by `SignerStateTone.color(forTone:)`.
    var statusTone: String { SignerStateTone.derivedTone(state) }
}

/// NIP-46 onboarding read model — `projections["nip46_onboarding"]`.
///
/// Rust owns the entire onboarding state machine and pre-computes every value
/// a host UI reads: the static signer-app probe table, the typed `stageKind`,
/// and the boolean flags shells use to render spinners / icons / buttons.
/// Views never string-compare stage values; they read `stageKind` directly.
///
/// Always present (the projection contributes a non-null payload on every
/// tick) so `signerApps` is reachable even when no handshake is in flight.
struct Nip46Onboarding: Decodable, Equatable {
    /// One row of the signer-app table. Rust owns the URL schemes that
    /// qualify as NIP-46 compatible; Swift only iterates and calls
    /// `UIApplication.canOpenURL` (a platform capability per aim.md §4.6).
    struct SignerApp: Decodable, Equatable, Identifiable {
        let scheme: String
        let signerKind: String
        var id: String { scheme }

        /// Human-readable brand name derived from the raw `scheme` token.
        ///
        /// `display_label` was removed from the `nip46_onboarding` wire (#1712,
        /// D7/D27 — presentation artifact). The kernel now ships only the raw
        /// `scheme`; the shell maps it to a brand name here (the same set Rust's
        /// signer catalog owns). Unknown schemes fall back to a humanized scheme.
        var displayLabel: String {
            switch scheme {
            case "nostrsigner://":  return "Amber"
            case "primal://":       return "Primal"
            case "nostrconnect://": return "Nostr Connect"
            default:
                return scheme
                    .replacingOccurrences(of: "://", with: "")
                    .capitalized
            }
        }
    }

    /// Typed stage token. `nil` when no handshake is in flight (mirrors the
    /// `bunker_handshake` slot's empty state). `unknown` is the forward-compat
    /// fall-through for any wire stage the host hasn't been re-typed against.
    enum StageKind: String, Decodable, Equatable {
        case idle
        case connecting
        case awaitingPubkey = "awaiting_pubkey"
        case ready
        case failed
        case unknown
    }

    let signerApps: [SignerApp]
    let stageKind: StageKind?
    /// Stable machine code for the progress label (#1711); `nil` for diagnostic
    /// transitions. The shell localizes it via `UiProgressProse`.
    let progressCode: String?
    let progressMessage: String?
    let isInFlight: Bool
    let isFailed: Bool
    let isTerminalSuccess: Bool
    let canCancel: Bool

    /// Localized progress label: the `progressCode` mapped to localized copy,
    /// falling back to the English `progressMessage` the wire still carries for
    /// any code the shell doesn't recognize (#1711, mirrors `localizedErrorToast`).
    var localizedProgress: String? {
        if let code = progressCode, let prose = UiProgressProse.localized(code: code) {
            return prose
        }
        return progressMessage
    }
}
