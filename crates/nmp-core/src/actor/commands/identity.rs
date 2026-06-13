//! Actor-local identity runtime + sign-in / switch / remove handlers.
//!
//! D4: the actor thread is the single writer of identity facts. The
//! authoritative store is the `HashMap<IdentityId, Keys>` here; the kernel's
//! `accounts` projection is pushed via `Kernel::set_accounts` after every
//! mutation, then emitted.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use nmp_signer_iface::SignerOp;
use nostr::nips::nip19::{FromBech32, ToBech32};
use nostr::{EventBuilder, Keys, Kind, PublicKey, SecretKey, Tag, Timestamp};
use serde::{Deserialize, Serialize};

use crate::actor::{canonical_relay_role, has_role};
use crate::kernel::{AccountSummary, AppRelay, Kernel};
use crate::relay::{canonical_relay_url, OutboundMessage};
use crate::remote_signer::RemoteSignerHandle;
use crate::substrate::{SignedEvent, UnsignedEvent};
use crate::util::sort_dedup;

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
/// `is_terminal_success`, `can_cancel`) and a pre-formatted English
/// `stage_label` so shells render fields directly instead of string-matching
/// on `stage`. The raw `stage` token stays on the wire as a stable diagnostic
/// key but no shell switches on it.
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
    /// Optional human-readable status (e.g. relay URL, error reason).
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
    /// Pre-formatted English label for `stage` (e.g. `"Connecting to bunker
    /// relays…"`, `"Awaiting bunker approval…"`, `"Connected"`,
    /// `"Bunker handshake failed"`). Always non-empty (D1); shells render this
    /// directly instead of mapping `stage` tokens to display strings.
    pub(crate) stage_label: String,
}

impl BunkerHandshakeDto {
    /// Construct a [`BunkerHandshakeDto`] from a stage wire token + optional
    /// message, pre-computing every derived field. Centralizing the derivation
    /// here is doctrine §6 anti-pattern #1: a shell must never reconstruct
    /// these flags / labels from `stage`.
    pub(crate) fn new(stage: String, message: Option<String>) -> Self {
        let kind = BunkerStageKind::from_wire(&stage);
        let is_idle = matches!(kind, BunkerStageKind::Idle);
        let is_in_flight = matches!(
            kind,
            BunkerStageKind::Connecting | BunkerStageKind::AwaitingPubkey
        );
        let is_failed = matches!(kind, BunkerStageKind::Failed);
        let is_terminal_success = matches!(kind, BunkerStageKind::Ready);
        let can_cancel = is_in_flight;
        let stage_label = stage_label_for(kind, &stage);
        Self {
            stage,
            message,
            is_idle,
            is_in_flight,
            is_failed,
            is_terminal_success,
            can_cancel,
            stage_label,
        }
    }
}

/// Pre-formatted English label for a handshake stage. `Unknown` falls back to
/// the raw wire token so an unrecognized stage still renders something
/// non-empty (D1) instead of an empty string. The known wire tokens use the
/// same prose AccountsView.swift used to derive from a `switch` block — the
/// strings move server-side once.
fn stage_label_for(kind: BunkerStageKind, raw_stage: &str) -> String {
    match kind {
        BunkerStageKind::Idle => "Idle".to_string(),
        BunkerStageKind::Connecting => "Connecting to bunker relays…".to_string(),
        BunkerStageKind::AwaitingPubkey => "Awaiting bunker approval…".to_string(),
        BunkerStageKind::Ready => "Connected".to_string(),
        BunkerStageKind::Failed => "Bunker handshake failed".to_string(),
        BunkerStageKind::Unknown => raw_stage.to_string(),
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
    /// Pre-computed display label (ADR-0032 / #1099) — shells render verbatim,
    /// never switching on `state`. See [`signer_state_label_and_tone`].
    pub(crate) status_label: String,
    /// Pre-computed tone: "active"|"warning"|"error"|"inactive" (ADR-0032 /
    /// #1099). Shells map tone → colour/icon with no `state`-string knowledge.
    pub(crate) status_tone: String,
}

impl SignerStateDto {
    /// Construct from a signer kind + state wire token + optional reason,
    /// pre-computing all derived boolean flags plus the display label/tone, so
    /// shells never reconstruct flags or display strings from `state` (AP1).
    pub(crate) fn new(signer_kind: String, state: String, reason: Option<String>) -> Self {
        use super::signer_state_label::signer_state_label_and_tone;
        let is_ready = state == "ready";
        let is_awaiting_approval = state == "awaiting_approval";
        let is_reconnecting = state == "reconnecting";
        let is_unavailable = state == "unavailable";
        let is_failed = state == "failed";
        let (status_label, status_tone) = signer_state_label_and_tone(&state);
        Self {
            signer_kind,
            state,
            reason,
            is_ready,
            is_awaiting_approval,
            is_reconnecting,
            is_unavailable,
            is_failed,
            status_label,
            status_tone,
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
    fn from_wire(raw: &str) -> Self {
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

/// One row of the static NIP-46 signer-app table — `(URL scheme, label)`
/// the host shows the user. The table is owned by Rust so the protocol layer
/// (not the platform shell) decides which signer apps qualify as "NIP-46
/// compatible" and how each is labelled.
///
/// `signer_kind` is the stable label that matches `AccountSummary.signer_kind`
/// once the user signs in through this app — exposed so hosts that want to
/// branch on installed-signer kind can read one value, not parse `scheme`.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct SignerAppDescriptor {
    /// Platform URL scheme to probe (`"nostrsigner://"`, `"primal://"`,
    /// `"nostrconnect://"`, …).
    pub(crate) scheme: String,
    /// Human-readable name hosts use in detected-signer CTAs.
    pub(crate) display_label: String,
    /// Stable signer-kind token. All entries here are NIP-46 brokered
    /// signers, so this is always `"nip46"` today; carried as a field so a
    /// future NIP-55 / hardware-signer entry can populate a different kind.
    pub(crate) signer_kind: String,
}

/// Static signer-app probe table. Rust owns this list; the platform shell
/// iterates it and uses its platform capability (e.g.
/// `UIApplication.canOpenURL`) to detect which entry is installed, then
/// renders the matching `display_label`.
///
/// D0: protocol-layer knowledge of which app schemes qualify as NIP-46
/// signers must not live in the platform shell — schemes change as the
/// ecosystem evolves (Nostr Signer, Primal, …) and that table is a
/// protocol-substrate concern.
fn signer_apps_table() -> Vec<SignerAppDescriptor> {
    vec![
        SignerAppDescriptor {
            scheme: "nostrsigner://".to_string(),
            display_label: "Nostr Signer".to_string(),
            signer_kind: "nip46".to_string(),
        },
        SignerAppDescriptor {
            scheme: "primal://".to_string(),
            display_label: "Primal".to_string(),
            signer_kind: "nip46".to_string(),
        },
        SignerAppDescriptor {
            scheme: "nostrconnect://".to_string(),
            display_label: "Signer App".to_string(),
            signer_kind: "nip46".to_string(),
        },
    ]
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
    /// Static table of `(scheme, display_label, signer_kind)` the host probes
    /// for installed signer apps. Always present — never empty.
    pub(crate) signer_apps: Vec<SignerAppDescriptor>,
    /// Typed handshake stage; `None` when no handshake is in flight (mirrors
    /// the bunker slot's `None` semantic).
    pub(crate) stage_kind: Option<BunkerStageKind>,
    /// Human-readable progress / error message; verbatim copy of the bunker
    /// slot's `message`. Hosts display this verbatim — they never format
    /// progress strings themselves.
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
    let (stage_kind, progress_message) = match raw {
        Some(dto) => (Some(BunkerStageKind::from_wire(&dto.stage)), dto.message),
        None => (None, None),
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
        progress_message,
        is_in_flight,
        is_failed,
        is_terminal_success,
        can_cancel: is_in_flight,
    }
}

/// `IdentityId` is the hex pubkey (matches NDK / applesauce / `AccountManager`).
pub(crate) type IdentityId = String;

/// Actor-local multi-account state. Insertion-ordered for deterministic UI.
///
/// Local-key accounts (nsec / generated) live in `keys`; remote-signer
/// accounts (NIP-46 bunker today, NIP-07 / hardware later) live in
/// `remote_signers`. Both share the same `order` list so the UI projection
/// stays deterministic. If the same pubkey lands in BOTH maps, the remote
/// signer wins (`active_signer_kind` + `sign_active_nonblocking` consult it
/// first) — the user explicitly added a remote handle, so route through it.
pub(crate) struct IdentityRuntime {
    keys: HashMap<IdentityId, Keys>,
    // Stored as `Arc<dyn>` (not `Box<dyn>`) so a handle can be cloned for
    // shared ownership where a `'static` owned reference is needed (broker
    // wiring). The ADR-0026 `SignerForSeal` adapter that originally motivated
    // the `Arc` is deleted (ADR-0050 §D5 — gift-wrap now signs through the port).
    remote_signers: HashMap<IdentityId, std::sync::Arc<dyn RemoteSignerHandle>>,
    order: Vec<IdentityId>,
    active: Option<IdentityId>,
    /// Shared output slot for the bunker-handshake projection. The actor (this
    /// runtime) is the sole writer (D4); the built-in `"bunker_handshake"`
    /// snapshot projection reads it. D0: NIP-46 remote signing is an app noun,
    /// so handshake state is NOT a typed `KernelSnapshot` field.
    bunker_handshake: BunkerHandshakeSlot,
    /// Shared output slot for the unified remote-signer health projection
    /// (ADR-0048 D6 — generalises the former `bunker_connection_state`).
    /// Parallels `bunker_handshake`. The actor is the sole writer (D4); the
    /// built-in `"signer_state"` snapshot projection reads it. Gives the host
    /// visibility into NIP-46 relay flaps and NIP-55 signer availability so it
    /// can show a degraded indicator or prompt re-auth rather than silently
    /// bricking the session.
    signer_state: SignerStateSlot,
    /// Stashed `make_active` flag for an in-flight `bunker://` handshake.
    ///
    /// `AddSigner { source: BunkerUri, make_active }` starts an async broker
    /// handshake; the resolved signer arrives later as a separate
    /// `AddSigner { source: RemoteHandle, .. }`. The originating `make_active`
    /// must survive that round-trip, so it is parked here rather than on the
    /// serialized [`BunkerHandshakeDto`] (which would leak the transient flag
    /// onto the snapshot wire). Set when the `BunkerUri` handshake is started;
    /// read + cleared when the matching `RemoteHandle` completes.
    pending_bunker_make_active: bool,
    /// ADR-0052 §D3 — per-app bunker-URI hook slot (replaces the deleted
    /// `bunker_hook::HOOK` global). Installed by `nmp_signer_broker_init`; read
    /// by `start_bunker_handshake` / `restore_bunker_session`. Empty until a
    /// broker installs — an invocation then degrades to a toast (D6).
    bunker_hook: crate::bunker_hook::BunkerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-55 restore hook slot (twin of `bunker_hook`;
    /// replaces `external_signer_hook::HOOK`). Installed by
    /// `nmp_external_signer_init`; read by `restore_nip55_session`.
    external_signer_hook: crate::external_signer_hook::ExternalSignerHookSlot,
}

impl IdentityRuntime {
    /// Construct an identity runtime bound to shared projection slots.
    ///
    /// `bunker_handshake` and `signer_state` are the `Arc<Mutex<…>>` slots the
    /// actor writes into and the built-in snapshot projections read from. The
    /// two `Arc` clones share one inner `Mutex` each, so an actor write is
    /// visible to the projection closure on the next tick without crossing the
    /// FFI boundary.
    pub(crate) fn new(bunker_handshake: BunkerHandshakeSlot, signer_state: SignerStateSlot) -> Self {
        Self {
            keys: HashMap::new(),
            remote_signers: HashMap::new(),
            order: Vec::new(),
            active: None,
            bunker_handshake,
            signer_state,
            pending_bunker_make_active: false,
            // ADR-0052 §D3 — empty per-app hook slots; production replaces them
            // with the `NmpApp`'s `Arc` clones via `set_signer_hook_slots`.
            bunker_hook: crate::bunker_hook::new_bunker_hook_slot(),
            external_signer_hook: crate::external_signer_hook::new_external_signer_hook_slot(),
        }
    }

    // ADR-0052 §D3 — per-app signer-hook bind/install/invoke methods live in
    // the sibling `signer_hooks` module; these accessors keep the slot fields
    // private to this owner.
    pub(super) fn bunker_hook_slot(&self) -> &crate::bunker_hook::BunkerHookSlot {
        &self.bunker_hook
    }
    pub(super) fn external_signer_hook_slot(
        &self,
    ) -> &crate::external_signer_hook::ExternalSignerHookSlot {
        &self.external_signer_hook
    }
    pub(super) fn set_bunker_hook_slot(&mut self, slot: crate::bunker_hook::BunkerHookSlot) {
        self.bunker_hook = slot;
    }
    pub(super) fn set_external_signer_hook_slot(
        &mut self,
        slot: crate::external_signer_hook::ExternalSignerHookSlot,
    ) {
        self.external_signer_hook = slot;
    }

    /// Write the latest bunker-handshake state into the shared projection slot
    /// (D4: actor is sole writer). A poisoned mutex recovers via
    /// `into_inner` rather than panicking the actor thread (D6).
    fn set_bunker_handshake(&self, value: Option<BunkerHandshakeDto>) {
        let mut slot = self
            .bunker_handshake
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = value;
    }

    /// Test-only read of the current bunker-handshake projection state.
    ///
    /// Production code never reads this slot through the runtime — the
    /// `"bunker_handshake"` snapshot projection holds the other `Arc` clone and
    /// reads it directly. This accessor exists purely so the command-path unit
    /// tests can assert on the handshake state the actor wrote.
    #[cfg(test)]
    pub(crate) fn bunker_handshake_for_test(&self) -> Option<BunkerHandshakeDto> {
        self.bunker_handshake
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    /// Write the latest remote-signer health into the shared `signer_state`
    /// projection slot (D4: actor is sole writer). A poisoned mutex recovers via
    /// `into_inner` rather than panicking the actor thread (D6).
    pub(crate) fn set_signer_state(&self, value: Option<SignerStateDto>) {
        let mut slot = self
            .signer_state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *slot = value;
    }

    /// Test-only read of the current signer-state projection value.
    ///
    /// Production code never reads this slot through the runtime.
    #[cfg(test)]
    pub(crate) fn signer_state_for_test(&self) -> Option<SignerStateDto> {
        self.signer_state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn add(&mut self, keys: Keys) -> IdentityId {
        let id = keys.public_key().to_hex();
        if !self.keys.contains_key(&id) && !self.remote_signers.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.keys.insert(id.clone(), keys);
        id
    }

    /// Register a remote-signer handle keyed by its user pubkey hex. Mirrors
    /// `add` for local keys: if the pubkey is new, append to `order`. Unlike the
    /// pre-`AddSigner` `add_remote`, this NEVER auto-activates — activation is
    /// the `add_signer` reducer's job (it owns the `make_active` decision,
    /// including the stashed-bunker-flag round-trip). Returns the account id.
    pub(crate) fn add_remote_inactive(
        &mut self,
        handle: Box<dyn RemoteSignerHandle>,
    ) -> IdentityId {
        let id = handle.pubkey_hex();
        if !self.keys.contains_key(&id) && !self.remote_signers.contains_key(&id) {
            self.order.push(id.clone());
        }
        // `Box<dyn T>` → `Arc<dyn T>` via `Arc::from(box)`. The actor's
        // boundary (`ActorCommand::AddSigner` / `SignerSource::RemoteHandle`)
        // still takes `Box<dyn>` so the broker / nmp-signers contract is
        // unchanged; the actor converts on insertion (ADR-0026 Phase 2 — see
        // the `remote_signers` field doc on [`IdentityRuntime`]).
        self.remote_signers
            .insert(id.clone(), std::sync::Arc::from(handle));
        id
    }

    fn active_keys(&self) -> Option<&Keys> {
        self.active.as_ref().and_then(|id| self.keys.get(id))
    }

    /// Borrow the active account's local `nostr::Keys`, or `None`.
    ///
    /// Returns `None` both when no account is active AND when the active
    /// account is a remote (NIP-46) signer — a remote signer holds no local
    /// secret key. Backend-transparent signing (incl. the NIP-17 gift-wrap DM
    /// chain after ADR-0050 §D5) goes through the actor's signer port
    /// (`SignEventForAccount` / `Nip44EncryptForAccount`), which routes both
    /// backends; this accessor is for the residual local-only consumers
    /// (e.g. Marmot's MLS identity) that genuinely hold `&Keys`.
    pub(crate) fn active_local_keys(&self) -> Option<&Keys> {
        self.active_keys()
    }

    fn active_remote(&self) -> Option<&dyn RemoteSignerHandle> {
        self.active
            .as_ref()
            .and_then(|id| self.remote_signers.get(id))
            .map(std::convert::AsRef::as_ref)
    }

    pub(crate) fn active_pubkey(&self) -> Option<String> {
        self.active.clone()
    }

    /// Returns `true` when `account_id` is registered in either the local-key
    /// or remote-signer map. Used by the `CapabilityResultReady` dispatch arm
    /// to confirm a since-queued write result still targets a live account —
    /// a result for a removed account is dropped (D6 trace) rather than
    /// cross-applied to whatever account is now active.
    pub(crate) fn contains_account(&self, account_id: &str) -> bool {
        self.keys.contains_key(account_id) || self.remote_signers.contains_key(account_id)
    }

    /// Fan an inbound remote-signer response out to every remote handle for
    /// correlation-keyed dispatch (ADR-0050 §D3b — the `DeliverSignerResponse`
    /// command). Each handle's `deliver_response` drops a non-matching id (the
    /// trait contract), so a stray frame degrades into the op's normal timeout
    /// (D6). Runs on the actor thread — single writer (D4).
    pub(crate) fn deliver_to_remote_signers(&self, response_json: &str) {
        for handle in self.remote_signers.values() {
            handle.deliver_response(response_json);
        }
    }

    /// Resolve a `signer_pubkey: Option<&str>` to its (remote handle, local
    /// keys) pair, matching the active-vs-named lookup the sign helpers use
    /// (remote shadows local). `pub(super)` so the sibling `cipher` module's
    /// NIP-44 helpers route without exposing the private key maps (§D1).
    pub(super) fn resolve_cipher_account(
        &self,
        signer_pubkey: Option<&str>,
    ) -> (Option<&dyn RemoteSignerHandle>, Option<&Keys>) {
        match signer_pubkey {
            Some(pk) => (
                self.remote_signers.get(pk).map(|h| h.as_ref()),
                self.keys.get(pk),
            ),
            None => (self.active_remote(), self.active_keys()),
        }
    }

    /// Bech32-encode the active account's secret key (`nsec1…`). Returns
    /// `None` for remote signers (no local key) and when no account is active.
    pub(crate) fn active_nsec_bech32(&self) -> Option<String> {
        self.active_keys()?.secret_key().to_bech32().ok()
    }

    /// Stable signer-kind label for the active account, or `None` if no
    /// account is active. `"local"` for nsec / generated keys; whatever the
    /// remote signer returns (`"nip46"`, …) for remote handles. Exposed for
    /// the broker (Stage 4) and diagnostic-snapshot consumers; today
    /// `sync_kernel` resolves the per-row kind inline so this helper has no
    /// in-tree caller yet.
    pub(crate) fn active_signer_kind(&self) -> Option<&'static str> {
        if let Some(handle) = self.active_remote() {
            return Some(handle.signer_kind());
        }
        self.active_keys().map(|_| "local")
    }

    /// Wall-clock deadline for the active account's next parked op. Reads
    /// `RemoteSignerHandle::op_timeout()` for remote signers (NIP-46 = 5s,
    /// NIP-55 = 90s); `PENDING_SIGN_TIMEOUT` otherwise (local ops are `Ready`
    /// and never park, so the default is safe). ADR-0048 D3 per-op deadline.
    pub(crate) fn active_sign_deadline(&self) -> crate::time::Instant {
        let duration = self
            .active_remote()
            .map(|h| h.op_timeout())
            .unwrap_or(nmp_signer_iface::PENDING_SIGN_TIMEOUT);
        crate::time::Instant::now() + duration
    }

    /// Wall-clock deadline for a parked op on a SPECIFIC account — the
    /// account-addressed sibling of [`Self::active_sign_deadline`]. Reads THAT
    /// account's signer budget (the active account may be a different backend);
    /// `None` falls back to the active account. ADR-0050 §D4.
    pub(crate) fn sign_deadline_for(&self, pubkey: Option<&str>) -> crate::time::Instant {
        let handle = match pubkey {
            Some(pk) => self.remote_signers.get(pk).map(|h| h.as_ref()),
            None => self.active_remote(),
        };
        let duration = handle
            .map(|h| h.op_timeout())
            .unwrap_or(nmp_signer_iface::PENDING_SIGN_TIMEOUT);
        crate::time::Instant::now() + duration
    }

}

/// Build a signed event over a fixed `Keys`. Mirrors the
/// `nmp-signers::LocalKeySigner::sign_now` recipe (same `nostr` primitives) —
/// kept here because D0 forbids importing `nmp-signers`. Two D6 correctness gates
/// (errors-as-state, never silent truncation), detailed at the call sites below:
/// out-of-`u16`-range kind, and any malformed tag — both hard-fail with a toast.
pub(super) fn sign_with(keys: &Keys, unsigned: &UnsignedEvent) -> Result<SignedEvent, String> {
    // Finding 1: validate kind is within the Nostr-defined u16 range before
    // casting. kind:65559 → kind:23 would be a silent correctness violation.
    if unsigned.kind > u32::from(u16::MAX) {
        return Err(format!(
            "invalid kind {}: must be in range [0, 65535]",
            unsigned.kind
        ));
    }
    let kind = Kind::from_u16(unsigned.kind as u16);

    // Finding 2: hard-fail on any malformed tag rather than silently dropping
    // it. The caller is responsible for building well-formed tags; silent
    // drops would produce a signed event that differs from the caller's intent
    // (D6 — correctness hazard for kind-agnostic publish pass-through).
    let mut tags = Vec::with_capacity(unsigned.tags.len());
    let mut malformed = 0usize;
    for t in &unsigned.tags {
        match Tag::parse(t) {
            Ok(tag) => tags.push(tag),
            Err(_) => malformed += 1,
        }
    }
    if malformed > 0 {
        return Err(format!("Dropped {malformed} malformed tag(s)"));
    }

    let event = EventBuilder::new(kind, &unsigned.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(unsigned.created_at))
        .sign_with_keys(keys)
        .map_err(|e| format!("sign failed: {e}"))?;
    Ok(SignedEvent {
        id: event.id.to_hex(),
        sig: event.sig.to_string(),
        unsigned: UnsignedEvent {
            pubkey: event.pubkey.to_hex(),
            kind: u32::from(event.kind.as_u16()),
            tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
            content: event.content.clone(),
            created_at: event.created_at.as_secs(),
        },
    })
}

/// Non-blocking sign with the active account (D8 — never blocks the actor
/// thread).
///
/// For a remote (NIP-46) signer it returns the `SignerOp` verbatim — typically
/// `SignerOp::Pending`, which the caller must park (`ParkedOp`) and
/// `poll()` on future loop ticks. For a local nsec/generated key the sign is
/// CPU-bound and resolves immediately into `SignerOp::Ready`.
///
/// `Err` (a `String`, surfaced as a toast per D6) covers the no-active-account
/// case; a local-signing failure is folded into a `SignerOp::Ready(Err(..))`
/// so the caller's single `poll()` match handles both signer kinds uniformly.
pub(crate) fn sign_active_nonblocking(
    identity: &IdentityRuntime,
    unsigned: &UnsignedEvent,
) -> Result<SignerOp<SignedEvent>, String> {
    if let Some(handle) = identity.active_remote() {
        return Ok(handle.sign(unsigned));
    }
    let keys = identity
        .active_keys()
        .ok_or_else(|| "no active account — sign in first".to_string())?;
    match sign_with(keys, unsigned) {
        Ok(signed) => Ok(SignerOp::ok(signed)),
        Err(e) => Ok(SignerOp::err(nmp_signer_iface::SignerError::Backend(
            format!("local sign failed: {e}"),
        ))),
    }
}

/// Non-blocking sign with a SPECIFIC account, looked up by pubkey hex across
/// BOTH the local-key and remote-signer maps — independent of which account is
/// currently active.
///
/// This is the `signer_pubkey: Some(_)` path for `PublishUnsignedEvent` /
/// `PublishUnsignedEventToRelays`: it lets a non-active account publish without
/// first switching active. Remote signers shadow local keys for the same
/// pubkey (consistent with `sign_active_nonblocking`'s ordering). Returns an
/// `Err(String)` (surfaced as a toast per D6) when no account matches the
/// pubkey; a local-signing failure is folded into a `SignerOp::Ready(Err(..))`
/// so the caller's single `poll()` match handles both signer kinds uniformly —
/// exactly like `sign_active_nonblocking`.
pub(crate) fn sign_with_account_nonblocking(
    identity: &IdentityRuntime,
    pubkey: &str,
    unsigned: &UnsignedEvent,
) -> Result<SignerOp<SignedEvent>, String> {
    if let Some(handle) = identity.remote_signers.get(pubkey) {
        return Ok(handle.sign(unsigned));
    }
    let keys = identity
        .keys
        .get(pubkey)
        .ok_or_else(|| format!("no signer for account {pubkey} — add it first"))?;
    match sign_with(keys, unsigned) {
        Ok(signed) => Ok(SignerOp::ok(signed)),
        Err(e) => Ok(SignerOp::err(nmp_signer_iface::SignerError::Backend(
            format!("local sign failed: {e}"),
        ))),
    }
}

/// Bech32-encode a hex pubkey as `npub1…`. Falls back to the raw hex if the
/// pubkey doesn't parse (defensive — never panics across FFI, D6).
fn npub_from_hex(hex: &str) -> String {
    PublicKey::from_hex(hex)
        .ok()
        .and_then(|pk| pk.to_bech32().ok())
        .unwrap_or_else(|| hex.to_string())
}

/// Pre-classified human-readable label for the row's signer. Swift binds
/// this verbatim — the previous Swift-side `switch kind.lowercased() { … }`
/// (aim.md §4.4 violation) is now this Rust-side classification.
///
/// Wire tokens recognised today:
/// - `"local"` — nsec / generated key kept inside the kernel.
/// - `"nip46"` — NIP-46 bunker (remote signer).
///
/// An unknown / future token returns the token unchanged so a forward-compat
/// signer adapter can ship a custom label simply by returning a new
/// `signer_kind()` string.
fn signer_label_for_kind(kind: &str) -> String {
    match kind {
        "local" => "Local key".to_string(),
        "nip46" => "NIP-46".to_string(),
        other => other.to_string(),
    }
}

/// Push the account projection + rebind the kernel's NIP-42 signer to the
/// active key (D4 single-writer: this is the only path that mutates either).
///
/// Order matters: remote signers shadow local keys for the same pubkey, so
/// the `signer_kind` projection reflects what `sign_active_nonblocking` will
/// actually use.
pub(super) fn sync_kernel(identity: &IdentityRuntime, kernel: &mut Kernel) {
    let active = identity.active.clone();
    let summaries = identity
        .order
        .iter()
        .filter_map(|id| {
            let (signer_kind, npub, signer_is_remote) =
                if let Some(handle) = identity.remote_signers.get(id) {
                    (handle.signer_kind().to_string(), npub_from_hex(id), true)
                } else if let Some(keys) = identity.keys.get(id) {
                    let npub = keys.public_key().to_bech32().unwrap_or_else(|_| id.clone());
                    ("local".to_string(), npub, false)
                } else {
                    return None;
                };
            let is_active = active.as_deref() == Some(id);
            Some(AccountSummary {
                id: id.clone(),
                npub,
                // aim.md §2 — no `short_pubkey` placeholder; `None` until
                // kind:0 lands, presentation layer renders its own
                // fallback. `Kernel::accounts_enriched` populates this
                // once kind:0 arrives.
                display_name: None,
                signer_label: signer_label_for_kind(&signer_kind),
                signer_kind,
                signer_is_remote,
                status: if is_active { "active" } else { "idle" }.to_string(),
                is_active,
                picture_url: None,
            })
        })
        .collect::<Vec<_>>();
    kernel.set_accounts(summaries, active.clone());

    // NIP-42 auth signer binding (V-06 / #960 — ONE uniform async sign seam).
    //
    // A REMOTE signer (NIP-46 / NIP-55) cannot sign synchronously — only the
    // broker holds the key — so we bind the AUTH *pubkey* (the active id is the
    // signer pubkey hex) and let `handle_auth_challenge` PARK the kind:22242 for
    // the async signer port. A LOCAL key binds the synchronous `AuthSignerFn`
    // and resolves inline. The kernel keeps these two bindings disjoint. No more
    // remote bail / "bunker AUTH unsupported" toast — bunker accounts now pass
    // NIP-42 AUTH gates as themselves.
    if let Some(active_id) = active.as_ref() {
        if identity.remote_signers.contains_key(active_id) {
            kernel.bind_auth_remote(active_id.clone());
            return;
        }
    }
    match active.as_ref().and_then(|id| identity.keys.get(id)) {
        Some(keys) => {
            let signer_keys = keys.clone();
            kernel.bind_auth_signer(
                keys.public_key().to_hex(),
                Arc::new(move |unsigned: &UnsignedEvent| sign_with(&signer_keys, unsigned)),
            );
        }
        None => kernel.clear_auth_signer(),
    }
}

/// Retarget the timeline to the active account.
///
/// V-112 (ADR-0042): `open_author()` deleted from kernel. Profile subscription
/// is now owned by the host (nmp_app_chirp_open_author_feed). This function
/// now only wires the follow-feed retarget (set_follow_feed_kinds is called by
/// the contact-list subscription path). Returning empty vec is correct: the
/// host triggers author-feed open via the FFI layer on navigation.
pub(super) fn retarget_timeline(
    identity: &IdentityRuntime,
    _kernel: &mut Kernel,
    _relays_ready: bool,
) -> Vec<OutboundMessage> {
    let _ = identity; // keep for callers that pass it; no retarget needed here
    Vec::new()
}

/// Unified sign-in reducer. Adds a signer from `source` and, when
/// `make_active`, binds it as the active account + retargets the timeline.
///
/// Replaces the old `sign_in_nsec` / `sign_in_bunker` / `add_remote_signer`
/// trio: the per-source logic now branches inside this single function.
///
/// * [`SignerSource::LocalNsec`] — parse the secret, register the local key,
///   and (when `make_active`) activate immediately. Returns the bootstrap +
///   retarget outbound frames so a fresh local account starts syncing at once.
/// * [`SignerSource::BunkerUri`] — shape-validate the URI, seed the
///   `bunker_handshake` projection, stash `make_active` for the async
///   round-trip, and delegate the handshake to the registered broker. No
///   outbound frames yet — the signer arrives later as a `RemoteHandle`.
/// * [`SignerSource::RemoteHandle`] — register the completed remote signer and
///   (when `make_active`) activate it, returning the retarget frames.
pub(crate) fn add_signer(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    source: crate::actor::SignerSource,
    make_active: bool,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    match source {
        crate::actor::SignerSource::LocalNsec(secret) => {
            let Some(keys) = parse_secret(secret.as_str()) else {
                kernel.set_last_error_toast(Some(
                    "invalid secret key — expected nsec1… or 64-hex".to_string(),
                ));
                return Vec::new();
            };
            let id = identity.add(keys);
            if make_active {
                identity.active = Some(id);
            }
            sync_kernel(identity, kernel);
            kernel.reconcile_follow_feed_after_identity_change();
            let mut outbound = kernel.active_account_bootstrap_requests();
            outbound.extend(retarget_timeline(identity, kernel, relays_ready));
            outbound
        }
        crate::actor::SignerSource::BunkerUri(uri) => {
            // Stash `make_active` so the async broker round-trip can apply it
            // when the resolved signer arrives as a `RemoteHandle` (the
            // serialized `BunkerHandshakeDto` must NOT carry this transient
            // flag — it would leak onto the snapshot wire).
            identity.pending_bunker_make_active = make_active;
            start_bunker_handshake(identity, kernel, &uri);
            Vec::new()
        }
        crate::actor::SignerSource::RemoteHandle(handle) => {
            let id = identity.add_remote_inactive(handle);
            // The broker round-trip may complete long after the originating
            // `BunkerUri` command; the `make_active` the user requested then was
            // stashed in `pending_bunker_make_active` (the broker adapter cannot
            // see the stash, so it sends its own `make_active` value). Honour
            // EITHER signal: the command's flag OR the stashed flag. Always
            // take + clear the stash so a later non-bunker `RemoteHandle` does
            // not inherit a stale value.
            let stashed = std::mem::take(&mut identity.pending_bunker_make_active);
            // A remote signer with no other active account always becomes
            // active so the user is signed in (`add_remote_inactive` never
            // auto-activates; this is the sole activation site).
            if make_active || stashed || identity.active.is_none() {
                identity.active = Some(id);
            }
            sync_kernel(identity, kernel);
            retarget_timeline(identity, kernel, relays_ready)
        }
    }
}

/// Pubkeys every fresh account follows out-of-the-box (hex, kind:3).
pub(super) const DEFAULT_FOLLOWS: &[&str] = &[
    // npub1l2vyh47mk2p0qlsku7hg0vn29faehy9hy34ygaclpn66ukqp3afqutajft
    "fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52",
    // fiatjaf
    "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d",
];
const DEFAULT_ONBOARDING_OVERRIDE_ROLE: &str = "both,indexer";

pub(crate) fn create_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    relays_ready: bool,
    profile: &HashMap<String, String>,
    relays: &[(String, String)],
    _mls: bool,
    make_active: bool,
) -> Vec<OutboundMessage> {
    let id = identity.add(Keys::generate());
    if make_active {
        identity.active = Some(id.clone());
    }
    sync_kernel(identity, kernel);
    // Only overwrite `configured_relays` when the caller declared relays during
    // onboarding. When `relays` is empty we keep whatever was seeded at Start
    // (`ActorCommand::Start { initial_relays }`) or via pre-start
    // `nmp_app_add_relay` — clobbering it with an empty vec would strip the
    // app's declared relay set.
    let relay_rows = relay_rows_from_create_account(relays);
    if !relays.is_empty() {
        kernel.set_configured_relays(relay_rows.clone());
    }

    // Pre-populate seed_contacts so the follow-feed can be set up immediately
    // without waiting for the published kind:3 to round-trip from relays.
    let follows = DEFAULT_FOLLOWS
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>();
    kernel.prepopulate_seed_contacts(id.clone(), follows);

    let mut publish_outbound = Vec::new();

    // ── Publish kind:0 metadata ──────────────────────────────────
    let kind0_content = match serde_json::to_string(profile) {
        Ok(json) => json,
        Err(e) => {
            kernel.set_last_error_toast(Some(format!("profile serialisation: {e}")));
            String::new()
        }
    };
    if let (false, Some(author)) = (kind0_content.is_empty(), identity.active_pubkey()) {
        let unsigned_meta = UnsignedEvent {
            pubkey: author,
            kind: 0,
            tags: Vec::new(),
            content: kind0_content,
            created_at: kernel.now_secs(),
        };
        // V-54 (closed, non-bug) / ADR-0040 site-3 correction: `create_account`
        // activates a fresh LOCAL key before this sign, so `sign_active_nonblocking`
        // takes the synchronous Ready branch — no remote round-trip, no actor
        // stall (D8). Enforce that invariant so a future edit can't silently
        // reintroduce an onboarding freeze (V-111 / #972 removed the blocking
        // primitive entirely).
        debug_assert!(
            identity.active_remote().is_none(),
            "cold-start kind:0 sign must run with a local key active (else blocks the actor)"
        );
        match sign_active_nonblocking(identity, &unsigned_meta).and_then(|mut op| {
            op.poll()
                .ok_or_else(|| "sign op pending — remote signer on cold-start path".to_string())
                .and_then(|r| r.map_err(|e| format!("sign failed: {e}")))
        }) {
            Ok(signed) => {
                // Cold-start routing (same chicken-and-egg as kind:10002 below).
                // A brand-new account has no kind:10002 on file, so the NIP-65
                // outbox resolver (`PublishTarget::Auto`) would resolve
                // `NoTargets` and the publish engine would silently drop this
                // profile metadata — nobody would ever see the new account's
                // display name. Route the initial kind:0 to the explicit
                // cold-start target instead.
                let target_relays = cold_start_publish_targets(kernel, &relay_rows);
                if target_relays.is_empty() {
                    // D6: no usable cold-start relay — surface a toast, never
                    // panic. The account still exists locally; the user can add
                    // relays and re-publish their profile from Settings.
                    kernel.set_last_error_toast(Some(
                        "could not publish profile — no cold-start relays available".to_string(),
                    ));
                } else {
                    publish_outbound.extend(kernel.publish_signed_to(
                        &signed,
                        &[],
                        crate::publish::PublishTarget::Explicit {
                            relays: target_relays,
                        },
                    ));
                }
            }
            Err(reason) => {
                // D6: sign failed — surface toast, skip publish. The
                // debug_assert above ensures this arm is unreachable on the
                // guaranteed local-key path (V-111 / #972).
                kernel.set_last_error_toast(Some(reason));
            }
        }
    }

    // ── Publish kind:10002 relay list ─────────────────────────────
    let relay_tags = nip65_tags_from_relay_rows(&relay_rows);
    if let (false, Some(author)) = (relay_tags.is_empty(), identity.active_pubkey()) {
        let unsigned_relay = UnsignedEvent {
            pubkey: author,
            kind: crate::kinds::KIND_RELAY_LIST,
            tags: relay_tags,
            content: String::new(),
            created_at: kernel.now_secs(),
        };
        // Local-key invariant (see kind:0 site above) — synchronous Ready
        // branch via sign_active_nonblocking, no actor stall (D8). V-111 / #972.
        debug_assert!(
            identity.active_remote().is_none(),
            "cold-start kind:10002 sign must run with a local key active (else blocks the actor)"
        );
        match sign_active_nonblocking(identity, &unsigned_relay).and_then(|mut op| {
            op.poll()
                .ok_or_else(|| "sign op pending — remote signer on cold-start path".to_string())
                .and_then(|r| r.map_err(|e| format!("sign failed: {e}")))
        }) {
            Ok(signed) => {
                kernel.prepopulate_author_relay_list(
                    signed.unsigned.pubkey.clone(),
                    signed.id.clone(),
                    signed.unsigned.created_at,
                    signed.unsigned.tags.clone(),
                );
                // Cold-start routing. A brand-new account has no kind:10002 on
                // file yet, so the NIP-65 outbox resolver (`PublishTarget::Auto`)
                // would resolve `NoTargets` and the publish engine would silently
                // drop this very event — the chicken-and-egg the account can never
                // escape (it can't announce its relays because it has no relays on
                // record). Route the initial relay list explicitly instead: to the
                // relays the user just declared (the canonical NIP-65 home of a
                // relay list — publish it to the relays it names) unioned with the
                // well-known discovery seed so others can find the new account.
                let target_relays = cold_start_publish_targets(kernel, &relay_rows);
                if target_relays.is_empty() {
                    // D6: no usable cold-start relay — surface a toast, never
                    // panic. The account still exists locally; the user can add
                    // relays and re-publish from Settings.
                    kernel.set_last_error_toast(Some(
                        "could not publish relay list — no cold-start relays available"
                            .to_string(),
                    ));
                } else {
                    publish_outbound.extend(kernel.publish_signed_to(
                        &signed,
                        &[],
                        crate::publish::PublishTarget::Explicit {
                            relays: target_relays,
                        },
                    ));
                }
            }
            Err(reason) => {
                // D6: sign failed — surface toast, skip publish. The
                // debug_assert above ensures this arm is unreachable on the
                // guaranteed local-key path (V-111 / #972).
                kernel.set_last_error_toast(Some(reason));
            }
        }
    }

    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound.extend(publish_outbound);
    outbound.extend(publish_initial_follows(identity, kernel, &relay_rows));
    outbound
}

/// Resolve the explicit relay set every *initial* event a brand-new account
/// emits — kind:0 (profile metadata), kind:3 (contacts) and kind:10002 (relay
/// list) — is published to on account creation (cold-start).
///
/// A freshly-created account has no kind:10002 in the store, so the NIP-65
/// outbox resolver cannot route any of its first events — it would resolve
/// `NoTargets` and the publish engine would drop them. This helper builds the
/// explicit cold-start target instead:
///
/// 1. The canonical relay rows the user just declared during onboarding; and
/// 2. The kernel's well-known discovery seed (`bootstrap_discovery_relays`) so
///    other clients performing relay-list / profile discovery can find the new
///    account.
///
/// The result is sorted + deduped. It is empty only when the user supplied no
/// relays AND no discovery relays are configured — the caller treats an empty
/// result as a D6 graceful failure (toast, never panic).
///
/// This applies ONLY to cold-start: `create_account` is the sole caller, and a
/// brand-new account by construction has no prior kind:10002. A user updating
/// their profile / contacts / relay list later publishes through
/// `publish_signed` (`Auto`), which routes to their already-declared write
/// relays — that path is unaffected.
fn cold_start_publish_targets(kernel: &Kernel, relay_rows: &[AppRelay]) -> Vec<String> {
    let mut targets: Vec<String> = relay_rows
        .iter()
        .map(|row| row.url.clone())
        .chain(kernel.bootstrap_discovery_relays())
        .collect();
    sort_dedup(&mut targets);
    targets
}

/// Canonicalize the onboarding-declared `(url, role)` pairs into `AppRelay`
/// rows. Returns an empty vec for empty input — there is NO hardcoded default
/// fallback anymore: when the caller declares no relays, the kernel keeps the
/// relay set seeded at `ActorCommand::Start` (or via pre-start
/// `nmp_app_add_relay`). The app, not `nmp-core`, owns the default relay list.
fn relay_rows_from_create_account(relays: &[(String, String)]) -> Vec<AppRelay> {
    relays
        .iter()
        .filter_map(|(url, role)| {
            let url = canonical_relay_url(url)?;
            let raw_role = if role.trim().is_empty() {
                DEFAULT_ONBOARDING_OVERRIDE_ROLE
            } else {
                role
            };
            let role = canonical_relay_role(raw_role).unwrap_or_else(|| "both".to_string());
            Some(AppRelay::new(url, role))
        })
        .collect()
}

fn nip65_tags_from_relay_rows(rows: &[AppRelay]) -> Vec<Vec<String>> {
    rows.iter()
        .filter_map(|row| {
            let read = has_role(&row.role, "read");
            let write = has_role(&row.role, "write");
            match (read, write) {
                (true, true) => Some(vec!["r".to_string(), row.url.clone()]),
                (true, false) => Some(vec!["r".to_string(), row.url.clone(), "read".to_string()]),
                (false, true) => Some(vec!["r".to_string(), row.url.clone(), "write".to_string()]),
                (false, false) => None,
            }
        })
        .collect()
}

/// Publish the cold-start kind:3 contacts list (`DEFAULT_FOLLOWS`) for a
/// brand-new account.
///
/// Like kind:0 and kind:10002, this is a cold-start publish: the account has
/// no kind:10002 on file, so the NIP-65 outbox resolver (`PublishTarget::Auto`)
/// would resolve `NoTargets` and the publish engine would silently drop the
/// contacts list — the new account's follows would never propagate. The
/// initial kind:3 is therefore routed to the explicit cold-start target
/// (`cold_start_publish_targets`), the same union of declared + discovery
/// relays the initial kind:0 / kind:10002 use.
///
/// `relay_rows` are the canonical relay rows declared during onboarding,
/// threaded through from `create_account` so the cold-start target can be
/// resolved without rebuilding or re-normalizing them.
fn publish_initial_follows(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    relay_rows: &[AppRelay],
) -> Vec<OutboundMessage> {
    let Some(author) = identity.active_pubkey() else {
        return Vec::new();
    };
    let tags = DEFAULT_FOLLOWS
        .iter()
        .map(|p| vec!["p".to_string(), p.to_string()])
        .collect::<Vec<_>>();
    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags,
        content: String::new(),
        created_at: kernel.now_secs(),
    };
    // Local-key invariant: `publish_initial_follows` is only called from
    // `create_account` (after a fresh local key is activated), so
    // `sign_active_nonblocking` takes the synchronous Ready branch — no remote
    // round-trip, no actor stall (D8). V-111 / #972 removed the blocking
    // primitive entirely.
    debug_assert!(
        identity.active_remote().is_none(),
        "cold-start kind:3 sign must run with a local key active (else blocks the actor)"
    );
    match sign_active_nonblocking(identity, &unsigned).and_then(|mut op| {
        op.poll()
            .ok_or_else(|| "sign op pending — remote signer on cold-start path".to_string())
            .and_then(|r| r.map_err(|e| format!("sign failed: {e}")))
    }) {
        Ok(signed) => {
            let target_relays = cold_start_publish_targets(kernel, relay_rows);
            if target_relays.is_empty() {
                // D6: no usable cold-start relay — surface a toast, never
                // panic. The follow set is already pre-populated locally
                // (`prepopulate_seed_contacts`); the user can re-publish
                // their contacts once relays are configured.
                kernel.set_last_error_toast(Some(
                    "could not publish contacts — no cold-start relays available".to_string(),
                ));
                Vec::new()
            } else {
                kernel.publish_signed_to(
                    &signed,
                    &[],
                    crate::publish::PublishTarget::Explicit {
                        relays: target_relays,
                    },
                )
            }
        }
        Err(reason) => {
            kernel.set_last_error_toast(Some(reason));
            Vec::new()
        }
    }
}

pub(crate) fn switch_active(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
    relays_ready: bool,
) -> Vec<OutboundMessage> {
    if !identity.keys.contains_key(identity_id)
        && !identity.remote_signers.contains_key(identity_id)
    {
        kernel.set_last_error_toast(Some(format!("account not found: {identity_id}")));
        return Vec::new();
    }
    if identity.active.as_deref() == Some(identity_id) {
        return Vec::new();
    }
    identity.active = Some(identity_id.to_string());
    sync_kernel(identity, kernel);
    // #168: reconcile the M2 follow-feed to the NEW active account — withdraw
    // the prior account's follow interests + emit the CLOSE diff (stale-feed /
    // privacy leak fix). Runs AFTER sync_kernel set kernel.active_account.
    kernel.reconcile_follow_feed_after_identity_change();
    let mut outbound = kernel.active_account_bootstrap_requests();
    outbound.extend(retarget_timeline(identity, kernel, relays_ready));
    outbound
}

pub(crate) fn remove_account(
    identity: &mut IdentityRuntime,
    kernel: &mut Kernel,
    identity_id: &str,
) -> Vec<OutboundMessage> {
    let had_local = identity.keys.remove(identity_id).is_some();
    let had_remote = match identity.remote_signers.remove(identity_id) {
        Some(handle) => {
            // Drain in-flight requests before dropping so blocked callers
            // fail fast rather than waiting for the remote-sign timeout.
            handle.disconnect();
            drop(handle);
            true
        }
        None => false,
    };
    if !had_local && !had_remote {
        return Vec::new();
    }
    identity.order.retain(|x| x != identity_id);
    if identity.active.as_deref() == Some(identity_id) {
        identity.active = identity.order.first().cloned();
    }
    sync_kernel(identity, kernel);
    // #168: removing an account (esp. the last → active=None) must withdraw
    // the prior account's M2 follow interests + emit the CLOSE diff so the
    // follow-feed subs do not leak past logout. Runs AFTER sync_kernel.
    kernel.reconcile_follow_feed_after_identity_change();
    Vec::new()
}

/// Update the `"signer_state"` projection when the NIP-46 relay-layer
/// connection state changes. V-14 step b, generalised by ADR-0048 D6.
///
/// `state` is one of `"connected"` | `"reconnecting"` | `"failed"`.
/// `"connected"` is mapped to `"ready"` in the unified `SignerStateDto` surface
/// so NIP-46 and NIP-55 share the same state vocabulary.
/// `reason` carries the error message for `"reconnecting"` and `"failed"`.
///
/// D0: the connection state is an app noun — written to the shared
/// [`SignerStateSlot`] (read by the `"signer_state"` snapshot projection)
/// instead of a typed `KernelSnapshot` field. The slot write does NOT flip
/// `changed_since_emit`, so the kernel is marked dirty explicitly — otherwise
/// the refreshed projection could sit unemitted until an unrelated kernel
/// mutation triggered a tick.
pub(crate) fn bunker_connection_state_changed(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    state: String,
    reason: Option<String>,
) {
    identity.set_signer_state(Some(SignerStateDto::from_nip46_connection_state(
        &state, reason,
    )));
    kernel.mark_changed_since_emit();
}

/// Update the `"signer_state"` projection for a NIP-55 signer event.
///
/// ADR-0048 D6: called from the capability-bridge result path when the host
/// reports a NIP-55 operation outcome that affects the long-lived signer
/// health (e.g. signer unavailable, rejected, awaiting approval).
pub(crate) fn nip55_signer_state_changed(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    state: String,
    reason: Option<String>,
) {
    identity.set_signer_state(Some(SignerStateDto::new("nip55".to_string(), state, reason)));
    kernel.mark_changed_since_emit();
}

/// Broker adapter → actor: latest NIP-46 handshake progress. Stage `"idle"`
/// clears the projection; everything else replaces it.
///
/// D0: the handshake state is an app noun, so it is written to the shared
/// [`BunkerHandshakeSlot`] (read by the `"bunker_handshake"` snapshot
/// projection) instead of a typed `KernelSnapshot` field. The slot write does
/// NOT flip `changed_since_emit`, so the kernel is marked dirty explicitly —
/// otherwise the refreshed projection could sit unemitted until an unrelated
/// kernel mutation triggered a tick.
pub(crate) fn bunker_handshake_progress(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    stage: String,
    message: Option<String>,
) {
    let value = if stage == "idle" {
        None
    } else {
        Some(BunkerHandshakeDto::new(stage, message))
    };
    identity.set_bunker_handshake(value);
    kernel.mark_changed_since_emit();
}

/// Shape-validate a `bunker://` URI, seed the `bunker_handshake` projection
/// with `"connecting"`, and delegate the handshake to the registered broker.
///
/// Called by [`add_signer`]'s [`crate::actor::SignerSource::BunkerUri`] arm
/// (which has already stashed `make_active` in `pending_bunker_make_active`).
fn start_bunker_handshake(identity: &IdentityRuntime, kernel: &mut Kernel, uri: &str) {
    // Stage 3 of NIP-46 wiring: actor exposes handshake-progress snapshot.
    // Stage 4 of NIP-46 wiring: actor delegates the handshake to the broker
    // hook installed in this app's per-app `bunker_hook` slot (ADR-0052 §D3 —
    // installed by `nmp_signer_broker_init`; no process-global).
    //
    // Here we shape-validate the URI, seed the snapshot with `"connecting"`
    // so the host sign-in flow renders progress immediately, then hand
    // the URI to the registered broker. The broker drives the connect /
    // get_public_key dance on its own thread and reports progress via
    // `BunkerHandshakeProgress` + `AddSigner { RemoteHandle, .. }`. D0 stays
    // clean: `nmp-core` imports neither the broker crate nor `nmp-signers`.
    if parse_bunker_remote(uri).is_none() {
        kernel.set_last_error_toast(Some(
            "invalid bunker:// URI — expected bunker://<64-hex-pubkey>?relay=…".to_string(),
        ));
        return;
    }
    identity.set_bunker_handshake(Some(BunkerHandshakeDto::new(
        "connecting".to_string(),
        Some("Waiting for broker...".to_string()),
    )));
    kernel.mark_changed_since_emit();
    if !identity.invoke_bunker_connect_hook(uri) {
        // Defence against init-order bugs: the broker should be registered
        // before any URI can reach the actor. If it isn't, surface a clear
        // toast and clear the progress projection (D6 — error becomes state,
        // never panic across FFI).
        identity.set_bunker_handshake(None);
        kernel.set_last_error_toast(Some(
            "NIP-46 broker not initialised — call nmp_signer_broker_init".to_string(),
        ));
    }
}

pub(crate) fn restore_bunker_session(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    payload_json: &str,
) {
    identity.set_bunker_handshake(Some(BunkerHandshakeDto::new(
        "connecting".to_string(),
        Some("Restoring broker session...".to_string()),
    )));
    kernel.mark_changed_since_emit();
    if !identity.invoke_bunker_restore_hook(payload_json) {
        identity.set_bunker_handshake(None);
        kernel.set_last_error_toast(Some(
            "NIP-46 broker not initialised — call nmp_signer_broker_init".to_string(),
        ));
    }
}

/// ADR-0048 D4 — restore a persisted NIP-55 account on cold start.
///
/// Unlike the bunker restore there is no handshake: the payload is
/// pubkey-only, so the registered driver hook synchronously reconstructs
/// the `Nip55Signer` and enqueues `AddSigner { RemoteHandle, .. }` back to
/// the actor. A missing hook degrades to a toast (D6) — defence against
/// init-order bugs, exactly like the bunker path.
pub(crate) fn restore_nip55_session(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    payload_json: &str,
) {
    if !identity.invoke_external_signer_restore_hook(payload_json) {
        identity.set_signer_state(Some(SignerStateDto::new(
            "nip55".to_string(),
            "unavailable".to_string(),
            Some("NIP-55 driver not initialised".to_string()),
        )));
        kernel.set_last_error_toast(Some(
            "NIP-55 driver not initialised — call nmp_external_signer_init".to_string(),
        ));
        kernel.mark_changed_since_emit();
    }
}

/// Parse an nsec/bech32 or 64-hex secret into `Keys`. `None` on bad input.
fn parse_secret(secret: &str) -> Option<Keys> {
    let s = secret.trim();
    if let Ok(sk) = SecretKey::from_bech32(s) {
        return Some(Keys::new(sk));
    }
    if s.len() == 64 {
        if let Ok(sk) = SecretKey::from_hex(s) {
            return Some(Keys::new(sk));
        }
    }
    None
}

/// Minimal `bunker://<remote-pubkey-hex>?relay=…` shape check. Returns the
/// remote pubkey hex if the URI is well-formed.
fn parse_bunker_remote(uri: &str) -> Option<String> {
    let rest = uri.trim().strip_prefix("bunker://")?;
    let pubkey = rest.split(['?', '/']).next()?;
    if pubkey.len() == 64 && pubkey.chars().all(|c| c.is_ascii_hexdigit()) {
        Some(pubkey.to_string())
    } else {
        None
    }
}

#[cfg(test)]
#[path = "identity/nip46_onboarding_tests.rs"]
mod nip46_onboarding_tests;
