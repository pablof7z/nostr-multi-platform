//! DTO types for the identity command runtime — NIP-46 handshake + signer
//! health projections.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

/// NIP-46 bunker handshake progress — the app noun projected onto the snapshot
/// under `projections["bunker_handshake"]`.
///
/// D0: NIP-46 remote signing is an app noun, not a kernel primitive. This type
/// lives in the identity command runtime (the actor layer), NOT in
/// `KernelSnapshot`. The actor writes it to a [`BunkerHandshakeSlot`]; a
/// built-in snapshot projection serializes it into the snapshot's
/// `projections` map every tick (D0 — the kernel emits, never names an app
/// noun).
///
/// Doctrine §6 anti-pattern #1 (duplicated formatting logic across platforms) +
/// RMP bible commandment #4 (no native business logic): the DTO carries
/// pre-computed boolean flags (`is_idle`, `is_in_flight`, `is_failed`,
/// `is_terminal_success`, `can_cancel`) so shells branch on a single flag
/// instead of string-matching on `stage`. The raw `stage` token stays on the
/// wire as the stable key; shells render it (and derive the display label) but
/// no shell switches on it to drive control flow. Per #1493 P9 (labels-to-shells,
/// mirrors #1568) the English `stage_label` was removed from the wire — shells
/// derive the label from the raw `stage` token themselves.
///
/// `Deserialize` is retained so Swift codegen / round-trip tests can decode it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[doc(hidden)]
pub struct BunkerHandshakeDto {
    /// `"connecting"` | `"awaiting_pubkey"` | `"ready"` | `"failed"` | `"idle"`
    /// (the wire never carries `"idle"` from the actor — `bunker_handshake_progress`
    /// maps it to `None` — but a broker that emits `"idle"` directly through
    /// the slot would still be classified correctly through `is_idle`).
    pub(crate) stage: String,
    /// Stable machine code for a user-facing progress label (#1711); `None` for
    /// diagnostic / `"failed"`. Shells localize it (fallback `message`).
    pub(crate) progress_code: Option<String>,
    /// Human-readable status (the English fallback prose / error reason).
    pub(crate) message: Option<String>,
    /// `stage == "idle"`. Defensive: the actor's `bunker_handshake_progress`
    /// collapses an `"idle"` stage to `None` (clearing the slot), so this flag
    /// is effectively always `false` on the wire today. Shells branch on it
    /// instead of `stage.lowercased() == "idle"` so a future broker path that
    /// emits `"idle"` straight into the slot stays correctly suppressed.
    pub(crate) is_idle: bool,
    /// `stage` is one of `"connecting"` or `"awaiting_pubkey"`. Shells use this
    /// to disable inputs and show a spinner without switching on `stage`.
    pub(crate) is_in_flight: bool,
    /// `stage == "failed"`. Shells flip the "Connect" button to "Retry" and
    /// swap the spinner for an error icon on this signal.
    pub(crate) is_failed: bool,
    /// `stage == "ready"` — the handshake has terminated successfully. Shells
    /// pair this with the green-check icon (vs. the red triangle for `is_failed`).
    pub(crate) is_terminal_success: bool,
    /// True when a cancel action would do something — i.e. the handshake is
    /// neither idle nor failed. Shells gate the visibility of a cancel button
    /// on this without reconstructing the rule from `stage` checks.
    pub(crate) can_cancel: bool,
}

impl BunkerHandshakeDto {
    /// Construct a [`BunkerHandshakeDto`] from a stage wire token + optional
    /// message, pre-computing every derived field. Centralizing the derivation
    /// here is doctrine §6 anti-pattern #1: a shell must never reconstruct
    /// these flags / labels from `stage`.
    pub(crate) fn new(stage: String, code: Option<String>, message: Option<String>) -> Self {
        let kind = BunkerStageKind::from_wire(&stage);
        let is_idle = matches!(kind, BunkerStageKind::Idle);
        let is_in_flight = matches!(
            kind,
            BunkerStageKind::Connecting | BunkerStageKind::AwaitingPubkey
        );
        let is_failed = matches!(kind, BunkerStageKind::Failed);
        let is_terminal_success = matches!(kind, BunkerStageKind::Ready);
        let can_cancel = is_in_flight;
        Self {
            stage,
            progress_code: code,
            message,
            is_idle,
            is_in_flight,
            is_failed,
            is_terminal_success,
            can_cancel,
        }
    }

    /// Build a handshake DTO for a `stage` from a [`UiToken`] progress label
    /// (#1711): `code` → `progress_code`, prose → `message`. For kernel-set labels.
    pub(crate) fn progress(stage: &str, token: &crate::ui_token::UiToken) -> Self {
        Self::new(
            stage.to_string(),
            Some(token.code().to_string()),
            Some(token.fallback_prose().to_string()),
        )
    }
}

/// Shared bunker-handshake slot — the output side of the bunker projection.
///
/// One `Arc` clone lives on the actor's [`IdentityRuntime`] (the sole writer,
/// D4); another is captured by the built-in `"bunker_handshake"`
/// snapshot-projection closure registered on `NmpApp`. The projection reads
/// this slot on every snapshot tick and serializes its contents into
/// `KernelSnapshot::projections`.
///
/// `None` (the default) means no handshake is in flight — the projection then
/// contributes JSON `null` under the `"bunker_handshake"` key, preserving the
/// "key present, value null when idle" semantic host sign-in flows
/// decode (an explicit `"idle"` stage from the broker maps to `None`).
#[doc(hidden)]
pub type BunkerHandshakeSlot = Arc<Mutex<Option<BunkerHandshakeDto>>>;

/// Construct a fresh, empty [`BunkerHandshakeSlot`].
///
/// `pub` so `nmp-ffi`'s `nmp_app_new` can build the slot before handing it
/// to the actor; the slot type is `pub(crate)` because only the identity
/// runtime owns the writer side.
pub fn new_bunker_handshake_slot() -> BunkerHandshakeSlot {
    Arc::new(Mutex::new(None))
}

/// Generalised remote-signer health projection — the app noun projected onto
/// the snapshot under `projections["signer_state"]`.
///
/// **ADR-0048 D6**: replaces the NIP-46-only `bunker_connection_state` with a
/// single canonical "remote signer health" surface keyed by `signer_kind`.
/// Hosts render one status row regardless of whether the active signer is NIP-46
/// or NIP-55 (Amber). `signer_kind` drives the label; `state` drives the badge
/// colour; `is_*` flags gate affordances without string-matching `state`.
///
/// **NIP-46 states:** `"ready"`, `"reconnecting"`, `"failed"` (relay transport
/// health — identical semantics as the former `bunker_connection_state`).
///
/// **NIP-55 states:** `"ready"`, `"awaiting_approval"` (Intent round-trip in
/// flight; drives "Waiting for Amber…" inline), `"unavailable"` (signer app not
/// installed / uninstalled mid-session), `"failed"` (rejected / mismatch /
/// timeout — permanent; host prompts re-auth).
///
/// `Deserialize` is retained so Swift codegen / round-trip tests can decode it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[doc(hidden)]
pub struct SignerStateDto {
    /// `"nip46"` | `"nip55"` | `"local"`. Stable label the host uses to pick
    /// the right icon/copy without string-matching on `state`.
    pub(crate) signer_kind: String,
    /// `"ready"` | `"awaiting_approval"` | `"reconnecting"` | `"unavailable"` | `"failed"`.
    pub(crate) state: String,
    /// Optional human-readable reason (error message on degraded/failed states).
    pub(crate) reason: Option<String>,
    /// `true` when `state == "ready"`.
    pub(crate) is_ready: bool,
    /// `true` when `state == "awaiting_approval"` (NIP-55 Intent round-trip in
    /// flight — drives "Waiting for Amber…" inline affordance).
    pub(crate) is_awaiting_approval: bool,
    /// `true` when `state == "reconnecting"` (NIP-46 transient relay flap).
    pub(crate) is_reconnecting: bool,
    /// `true` when `state == "unavailable"` (NIP-55 signer app not installed /
    /// uninstalled mid-session). Host prompts the user to install or pick a
    /// different signer.
    pub(crate) is_unavailable: bool,
    /// `true` when `state == "failed"` (permanent error — rejected / mismatch /
    /// relay handshake failed). Host prompts re-auth.
    pub(crate) is_failed: bool,
}

impl SignerStateDto {
    /// Construct from a signer kind + state wire token + optional reason,
    /// pre-computing all derived boolean flags so shells never reconstruct flags
    /// from `state` (AP1). Display strings (label/tone) are NOT pre-computed:
    /// per #1493 P9 (labels-to-shells, mirrors #1568) the shells derive the
    /// English label and semantic tone from the raw `state` token themselves.
    pub(crate) fn new(signer_kind: String, state: String, reason: Option<String>) -> Self {
        let is_ready = state == "ready";
        let is_awaiting_approval = state == "awaiting_approval";
        let is_reconnecting = state == "reconnecting";
        let is_unavailable = state == "unavailable";
        let is_failed = state == "failed";
        Self {
            signer_kind,
            state,
            reason,
            is_ready,
            is_awaiting_approval,
            is_reconnecting,
            is_unavailable,
            is_failed,
        }
    }

    /// Build a NIP-46 state from the relay-layer connection state token.
    ///
    /// Maps the old `bunker_connection_state` tokens (`"connected"`,
    /// `"reconnecting"`, `"failed"`) into the unified `signer_state` surface.
    /// `"connected"` maps to `"ready"` for consistency with NIP-55 naming.
    pub(crate) fn from_nip46_connection_state(state: &str, reason: Option<String>) -> Self {
        // Map legacy "connected" → "ready" so NIP-46 and NIP-55 share the name.
        let canonical_state = if state == "connected" {
            "ready".to_string()
        } else {
            state.to_string()
        };
        Self::new("nip46".to_string(), canonical_state, reason)
    }
}

/// Shared signer-state slot (ADR-0048 D6 generalisation of the former
/// bunker-connection-state slot).
///
/// `None` (the default) means no remote signer session is active (the
/// projection then contributes JSON `null` under `"signer_state"`).
#[doc(hidden)]
pub type SignerStateSlot = Arc<Mutex<Option<SignerStateDto>>>;

/// Construct a fresh, empty [`SignerStateSlot`].
///
/// `pub` so `nmp-ffi`'s `nmp_app_new` can build the slot; the actor is the
/// sole writer (D4).
pub fn new_signer_state_slot() -> SignerStateSlot {
    Arc::new(Mutex::new(None))
}

/// Typed token for the NIP-46 handshake stage. Mirrors the wire strings the
/// broker writes into [`BunkerHandshakeDto::stage`] one-to-one; hosts read
/// this instead of string-comparing the raw stage value (which is then a Rust
/// implementation detail). `Unknown` covers forward-compat for any new wire
/// value the host hasn't been re-typed against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BunkerStageKind {
    Idle,
    Connecting,
    AwaitingPubkey,
    Ready,
    Failed,
    Unknown,
}

impl BunkerStageKind {
    /// Decode a wire stage string into the typed enum. Unknown values map to
    /// `Unknown` so a host that has not been re-typed still gets a stable read.
    pub(super) fn from_wire(raw: &str) -> Self {
        match raw {
            "idle" => Self::Idle,
            "connecting" => Self::Connecting,
            "awaiting_pubkey" => Self::AwaitingPubkey,
            "ready" => Self::Ready,
            "failed" => Self::Failed,
            _ => Self::Unknown,
        }
    }
}

/// One row of the static NIP-46 signer-app table — the `(URL scheme,
/// signer_kind)` pair the host probes for. The table is owned by Rust so the
/// protocol layer (not the platform shell) decides which signer apps qualify as
/// "NIP-46 compatible".
///
/// The pre-rendered `display_label` vendor name ("Amber"/"Primal"/"Nostr
/// Connect") was removed from the wire (#1712, D7/D27 — presentation artifact);
/// shells resolve the brand name from their own generated signer catalog
/// (`KnownSigners.generated.{swift,kt}`) keyed by `scheme`.
///
/// `signer_kind` is the stable label that matches `AccountSummary.signer_kind`
/// once the user signs in through this app — exposed so hosts that want to
/// branch on installed-signer kind can read one value, not parse `scheme`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SignerAppDescriptor {
    /// Platform URL scheme to probe (`"nostrsigner://"`, `"primal://"`,
    /// `"nostrconnect://"`, …).
    pub(crate) scheme: String,
    /// Stable signer-kind token. All entries here are NIP-46 brokered
    /// signers, so this is always `"nip46"` today; carried as a field so a
    /// future NIP-55 / hardware-signer entry can populate a different kind.
    pub(crate) signer_kind: String,
}

/// The NIP-46 `nostrconnect` onboarding probe table — **derived from the
/// single Rust-owned [`crate::signer_catalog`]** (#1493 P9), no longer a
/// hand-authored list. The platform shell iterates it and uses its platform
/// capability (`UIApplication.canOpenURL`) to detect which entry is installed,
/// then resolves the matching brand name from its own generated signer catalog.
///
/// This surface is the iOS onboarding catalog, so it is exactly the catalog
/// entries that (a) are offered on iOS (`ios.is_some()`) and (b) speak NIP-46.
/// A catalog entry offered only on Android, or one that speaks NIP-55 only,
/// would be excluded here — it is not a `nostrconnect`/NIP-46 onboarding target.
///
/// D0: protocol-layer knowledge of which app schemes qualify as NIP-46 signers
/// must not live in the platform shell — that catalog is a protocol-substrate
/// concern, owned in `crate::signer_catalog`.
fn signer_apps_table() -> Vec<SignerAppDescriptor> {
    use crate::signer_catalog::{known_signer_apps, SignerCapability};
    known_signer_apps()
        .iter()
        .filter_map(|app| {
            let ios = app.ios?;
            if !app.speaks(SignerCapability::Nip46) {
                return None;
            }
            Some(SignerAppDescriptor {
                scheme: format!("{}://", ios.url_scheme),
                signer_kind: SignerCapability::Nip46.as_token().to_string(),
            })
        })
        .collect()
}

/// Pre-computed NIP-46 onboarding read model — `projections["nip46_onboarding"]`.
///
/// Derives every field a host onboarding screen reads from the same
/// [`BunkerHandshakeSlot`] the `"bunker_handshake"` projection serializes,
/// plus the static signer-app table Rust owns. Hosts no longer:
///   * keep a typed enum of stage strings (`stage_kind` carries the typed
///     token)
///   * switch on stage strings to decide which spinner / icon / button state
///     to render (`is_in_flight`, `is_failed`, `is_terminal_success`,
///     `can_cancel` are pre-computed)
///   * hard-code which URL schemes count as NIP-46 signer apps
///     (`signer_apps`)
///
/// D0: NIP-46 remote signing is an app noun, so this projection lives under
/// the kernel's `projections` map exactly like `"bunker_handshake"` — never
/// as a typed `KernelSnapshot` field.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct Nip46OnboardingDto {
    /// Static table of `(scheme, signer_kind)` the host probes for installed
    /// signer apps. Always present — never empty.
    pub(crate) signer_apps: Vec<SignerAppDescriptor>,
    /// Typed handshake stage; `None` when no handshake is in flight (mirrors
    /// the bunker slot's `None` semantic).
    pub(crate) stage_kind: Option<BunkerStageKind>,
    /// Stable machine code for the progress label (#1711). Hosts localize it (fallback `progress_message`).
    pub(crate) progress_code: Option<String>,
    /// Human-readable progress / error message (English fallback prose); copy of
    /// the bunker slot's `message`, rendered when `progress_code` is unrecognized.
    pub(crate) progress_message: Option<String>,
    /// True when a handshake is mid-flight (`connecting` / `awaiting_pubkey`).
    /// Hosts use this to disable inputs and show a spinner without inspecting
    /// `stage_kind`.
    pub(crate) is_in_flight: bool,
    /// True when the last handshake attempt ended in `failed`. Hosts swap
    /// the "Connect" button to "Retry" on this signal.
    pub(crate) is_failed: bool,
    /// True when the handshake reached `ready` (final success). Hosts move
    /// off the onboarding screen on this signal.
    pub(crate) is_terminal_success: bool,
    /// True when a cancel action would do something — i.e. a handshake is in
    /// flight. Hosts gate the visibility of the cancel button on this.
    pub(crate) can_cancel: bool,
}

/// Build the `nip46_onboarding` projection payload by reading the shared
/// bunker-handshake slot and deriving the typed view. Runs on every snapshot
/// tick (D8: lock-and-clone only, no allocation in the steady-state path
/// beyond the static signer-app vec).
pub(crate) fn build_nip46_onboarding_dto(slot: &BunkerHandshakeSlot) -> Nip46OnboardingDto {
    let raw = slot
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    let (stage_kind, progress_code, progress_message) = match raw {
        Some(dto) => {
            let kind = BunkerStageKind::from_wire(&dto.stage);
            (Some(kind), dto.progress_code, dto.message)
        }
        None => (None, None, None),
    };
    let is_in_flight = matches!(
        stage_kind,
        Some(BunkerStageKind::Connecting | BunkerStageKind::AwaitingPubkey)
    );
    let is_failed = matches!(stage_kind, Some(BunkerStageKind::Failed));
    let is_terminal_success = matches!(stage_kind, Some(BunkerStageKind::Ready));
    Nip46OnboardingDto {
        signer_apps: signer_apps_table(),
        stage_kind,
        progress_code,
        progress_message,
        is_in_flight,
        is_failed,
        is_terminal_success,
        can_cancel: is_in_flight,
    }
}
