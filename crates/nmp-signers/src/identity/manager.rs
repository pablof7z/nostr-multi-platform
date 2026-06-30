//! `AccountManager` — multi-account runtime state with synchronous
//! active-switch.

use std::collections::HashMap;
use std::sync::Arc;

use nostr::PublicKey;
use serde::{Deserialize, Serialize};

use crate::signers::Signer;

/// Identity id is the hex-encoded pubkey of the account (matches NDK + applesauce).
pub type IdentityId = String;

/// `AccountManager` error variants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AccountError {
    /// No account exists with this id.
    NotFound(IdentityId),
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "account not found: {id}"),
        }
    }
}

impl std::error::Error for AccountError {}

/// Observer payload for active-account transitions.  Fires on `switch_active`
/// (new active) and on `remove(active_id)` (`current = None` — teardown
/// signal: kind:3 / kind:10002 close-out + `FullState { active_account: None }`).
/// No-op transitions do not fire.
#[derive(Clone, Debug)]
pub struct ActiveChangeEvent {
    /// Previous active account, if any.
    pub previous: Option<IdentityId>,
    /// New active account, or `None` if the active slot was cleared.
    pub current: Option<IdentityId>,
    /// Pubkey of the new active account.  `None` iff `current` is `None`.
    pub current_pubkey: Option<PublicKey>,
}

/// Observer hook for active-account changes.  Runs on the caller's thread
/// (which in the NMP kernel is the actor thread per D4 — single writer per
/// fact).
pub trait ActiveChangeObserver: Send + Sync {
    /// Called after the active slot has been updated synchronously, but
    /// before the originating `switch_active` / `remove` call returns.
    /// Observers must not block — the actor thread is on the hot path.
    fn on_active_change(&self, event: &ActiveChangeEvent);
}

/// Multi-account holder.
pub struct AccountManager {
    accounts: HashMap<IdentityId, Arc<dyn Signer>>,
    /// Insertion-order list of ids for deterministic iteration.
    order: Vec<IdentityId>,
    active: Option<IdentityId>,
    observers: Vec<Arc<dyn ActiveChangeObserver>>,
}

impl std::fmt::Debug for AccountManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountManager")
            .field("account_count", &self.accounts.len())
            .field("active", &self.active)
            .field("observers", &self.observers.len())
            .finish_non_exhaustive()
    }
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountManager {
    /// Empty manager.
    #[must_use]
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            order: Vec::new(),
            active: None,
            observers: Vec::new(),
        }
    }

    /// Add an account.  Inserts the signer into the roster.
    ///
    /// **PD-004 (same nsec = same account):** `IdentityId` is permanently
    /// `pubkey_hex`.  Adding a pubkey that is already known is an idempotent
    /// no-op — it returns the existing id and keeps the originally-installed
    /// signer.  NMP explicitly rejects the applesauce "two accounts for one
    /// pubkey" model: one pubkey is always exactly one account slot (at most a
    /// future relay-policy merge — the `Signer` trait carries no policy today,
    /// so nothing to merge yet).
    #[must_use]
    pub fn add(&mut self, signer: Arc<dyn Signer>) -> Result<IdentityId, AccountError> {
        let pubkey = signer.pubkey();
        let id = pubkey.to_hex();
        if self.accounts.contains_key(&id) {
            return Ok(id);
        }
        self.accounts.insert(id.clone(), signer);
        self.order.push(id.clone());
        Ok(id)
    }

    /// Switch the active account.  Invariants:
    /// 1. New signer is installed (`active` flipped) **synchronously** before
    ///    observers run.
    /// 2. Observers run **after** the flip, in registration order.
    /// 3. Switching to the already-active id is a no-op (no observer fires).
    #[must_use]
    pub fn switch_active(&mut self, id: &IdentityId) -> Result<(), AccountError> {
        if !self.accounts.contains_key(id) {
            return Err(AccountError::NotFound(id.clone()));
        }
        if self.active.as_deref() == Some(id) {
            return Ok(());
        }
        let previous = self.active.take();
        self.active = Some(id.clone());
        let current_pubkey = self
            .accounts
            .get(id)
            .expect("checked above") // doctrine-allow: D6 — `accounts.contains_key(id)` is guarded at the top of this fn (line 160); a missing key here means a logic bug, not a runtime error
            .pubkey();
        let event = ActiveChangeEvent {
            previous,
            current: Some(id.clone()),
            current_pubkey: Some(current_pubkey),
        };
        for obs in &self.observers {
            obs.on_active_change(&event);
        }
        Ok(())
    }

    /// Remove an account.  Atomic semantics (codex review #5 — 9944bed.md):
    ///
    /// - Missing id → no-op (idempotent; `Ok(())`, no observers fire).
    /// - Present, not active → drop + shrink order, no observers fire.
    /// - Present and active → clear active **before** firing observers, then
    ///   notify once with `ActiveChangeEvent { current: None, current_pubkey:
    ///   None }`.  This is the kind:3 / kind:10002 teardown + `FullState
    ///   { active_account: None }` signal.
    #[must_use]
    pub fn remove(&mut self, id: &IdentityId) -> Result<(), AccountError> {
        if !self.accounts.contains_key(id) {
            return Ok(());
        }
        let was_active = self.active.as_deref() == Some(id);
        self.accounts.remove(id);
        self.order.retain(|x| x != id);
        if !was_active {
            return Ok(());
        }
        let previous = self.active.take();
        let event = ActiveChangeEvent {
            previous,
            current: None,
            current_pubkey: None,
        };
        for obs in &self.observers {
            obs.on_active_change(&event);
        }
        Ok(())
    }

    /// Active id.
    #[must_use]
    pub fn active(&self) -> Option<IdentityId> {
        self.active.clone()
    }

    /// All ids, in insertion order.
    #[must_use]
    pub fn accounts(&self) -> Vec<IdentityId> {
        self.order.clone()
    }

    /// Signer for a specific id.
    #[must_use]
    pub fn signer_for(&self, id: &IdentityId) -> Option<Arc<dyn Signer>> {
        self.accounts.get(id).cloned()
    }

    /// Signer for the active id, if any.
    #[must_use]
    pub fn signer_active(&self) -> Option<Arc<dyn Signer>> {
        self.active.as_ref().and_then(|id| self.signer_for(id))
    }

    /// Register an observer for active-account changes.  Observers fire on
    /// every `switch_active` (no-op switches do NOT fire).
    pub fn observe(&mut self, observer: Arc<dyn ActiveChangeObserver>) {
        self.observers.push(observer);
    }

    /// Number of registered observers (test introspection).
    #[must_use]
    pub fn observer_count(&self) -> usize {
        self.observers.len()
    }
}
