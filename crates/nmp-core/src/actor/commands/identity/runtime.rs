//! [`IdentityRuntime`] struct definition and its impl methods.
//!
//! D4: the actor thread is the single writer of identity facts. The
//! authoritative store is the `HashMap<IdentityId, Keys>` here; the kernel's
//! `accounts` projection is pushed via `Kernel::set_accounts` after every
//! mutation, then emitted.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nostr::Keys;

use crate::remote_signer::RemoteSignerHandle;

use super::dto::{BunkerHandshakeDto, BunkerHandshakeSlot, SignerStateDto, SignerStateSlot};

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
    pub(super) keys: HashMap<IdentityId, Keys>,
    // Arc lets broker wiring share a remote handle without owning the actor map.
    pub(super) remote_signers: HashMap<IdentityId, Arc<dyn RemoteSignerHandle>>,
    pub(super) order: Vec<IdentityId>,
    pub(super) active: Option<IdentityId>,
    pub(super) app_managed: HashSet<IdentityId>,
    /// Actor-written output slot for the `"bunker_handshake"` projection.
    pub(super) bunker_handshake: BunkerHandshakeSlot,
    /// Actor-written output slot for the unified remote-signer health projection.
    pub(super) signer_state: SignerStateSlot,
    /// Stashed flags for an in-flight `bunker://` handshake.
    pub(super) pending_bunker_make_active: bool,
    /// ADR-0052 §D3 — per-app bunker-URI hook slot.
    pub(super) bunker_hook: crate::bunker_hook::BunkerHookSlot,
    /// ADR-0052 §D3 — per-app NIP-55 restore hook slot.
    pub(super) external_signer_hook: crate::external_signer_hook::ExternalSignerHookSlot,
}

impl IdentityRuntime {
    /// Construct an identity runtime bound to shared projection slots.
    pub(crate) fn new(
        bunker_handshake: BunkerHandshakeSlot,
        signer_state: SignerStateSlot,
    ) -> Self {
        Self {
            keys: HashMap::new(),
            remote_signers: HashMap::new(),
            order: Vec::new(),
            active: None,
            app_managed: HashSet::new(),
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
    pub(crate) fn bunker_hook_slot(&self) -> &crate::bunker_hook::BunkerHookSlot {
        &self.bunker_hook
    }
    pub(crate) fn external_signer_hook_slot(
        &self,
    ) -> &crate::external_signer_hook::ExternalSignerHookSlot {
        &self.external_signer_hook
    }
    pub(crate) fn set_bunker_hook_slot(&mut self, slot: crate::bunker_hook::BunkerHookSlot) {
        self.bunker_hook = slot;
    }
    pub(crate) fn set_external_signer_hook_slot(
        &mut self,
        slot: crate::external_signer_hook::ExternalSignerHookSlot,
    ) {
        self.external_signer_hook = slot;
    }

    /// Write the latest bunker-handshake state into the shared projection slot
    /// (D4: actor is sole writer). A poisoned mutex recovers via
    /// `into_inner` rather than panicking the actor thread (D6).
    pub(super) fn set_bunker_handshake(&self, value: Option<BunkerHandshakeDto>) {
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

    pub(super) fn add(&mut self, keys: Keys) -> IdentityId {
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
        self.remote_signers.insert(id.clone(), Arc::from(handle));
        id
    }

    pub(super) fn active_keys(&self) -> Option<&Keys> {
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

    pub(super) fn active_remote(&self) -> Option<&dyn RemoteSignerHandle> {
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

    pub(crate) fn is_app_managed(&self, account_id: &str) -> bool {
        self.app_managed.contains(account_id)
    }

    pub(super) fn set_app_managed(&mut self, account_id: &str, app_managed: bool) {
        if app_managed {
            self.app_managed.insert(account_id.to_string());
            if self.active.as_deref() == Some(account_id) {
                self.active = None;
            }
        } else {
            self.app_managed.remove(account_id);
        }
    }

    pub(crate) fn app_managed_local_secrets(&self) -> Vec<(IdentityId, String)> {
        use nostr::nips::nip19::ToBech32;
        self.order
            .iter()
            .filter(|id| self.app_managed.contains(*id))
            .filter_map(|id| {
                let secret = self.keys.get(id)?.secret_key().to_bech32().ok()?;
                Some((id.clone(), secret))
            })
            .collect()
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
    /// (remote shadows local). `pub(crate)` so the sibling `cipher` module's
    /// NIP-44 helpers route without exposing the private key maps (§D1).
    pub(crate) fn resolve_cipher_account(
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
        use nostr::nips::nip19::ToBech32;
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
