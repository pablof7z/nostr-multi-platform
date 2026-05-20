//! `AccountManager` — multi-account runtime state with synchronous
//! active-switch + applesauce-style signer post-conditions.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use nmp_core::substrate::UnsignedEvent;
use nostr::PublicKey;
use serde::{Deserialize, Serialize};

use crate::signers::{Signer, SignerError};

/// Identity id is the hex-encoded pubkey of the account (matches NDK + applesauce).
pub type IdentityId = String;

/// AccountManager error variants.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum AccountError {
    /// No account exists with this id.
    NotFound(IdentityId),
    /// The signer's `sign(test_template)` post-condition failed — refused.
    SignerMismatch(String),
    /// The signer's `sign(test_template)` itself errored.
    SignerError(SignerError),
}

impl std::fmt::Display for AccountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AccountError::NotFound(id) => write!(f, "account not found: {id}"),
            AccountError::SignerMismatch(m) => write!(f, "signer mismatch: {m}"),
            AccountError::SignerError(e) => write!(f, "signer error: {e}"),
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
    /// Timeout for the per-account add-time signer post-condition.  Default
    /// 5s — generous because NIP-46 round-trips can be slow on cold
    /// connections.  Can be lowered for tests via `with_post_condition_timeout`.
    post_condition_timeout: Duration,
}

impl std::fmt::Debug for AccountManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountManager")
            .field("account_count", &self.accounts.len())
            .field("active", &self.active)
            .field("observers", &self.observers.len())
            .finish()
    }
}

impl Default for AccountManager {
    fn default() -> Self {
        Self::new()
    }
}

impl AccountManager {
    /// Empty manager.
    pub fn new() -> Self {
        Self {
            accounts: HashMap::new(),
            order: Vec::new(),
            active: None,
            observers: Vec::new(),
            post_condition_timeout: Duration::from_secs(5),
        }
    }

    /// Override the add-time post-condition timeout (test convenience).
    pub fn with_post_condition_timeout(mut self, timeout: Duration) -> Self {
        self.post_condition_timeout = timeout;
        self
    }

    /// Add an account.  Runs the applesauce `SignerMismatchError` post-condition:
    /// signs a fixed test template and verifies the returned pubkey + computed
    /// id match.  Refuses the account if the check fails — protects against
    /// malicious or buggy signers that mutate the event before signing.
    ///
    /// **PD-004 (same nsec = same account):** `IdentityId` is permanently
    /// `pubkey_hex`.  Adding a pubkey that is already known is an idempotent
    /// no-op — it returns the existing id, keeps the originally-installed
    /// signer, and does NOT re-run the post-condition.  NMP explicitly rejects
    /// the applesauce "two accounts for one pubkey" model: one pubkey is always
    /// exactly one account slot (at most a future relay-policy merge — the
    /// `Signer` trait carries no policy today, so nothing to merge yet).
    pub fn add(&mut self, signer: Arc<dyn Signer>) -> Result<IdentityId, AccountError> {
        let pubkey = signer.pubkey();
        let id = pubkey.to_hex();
        if self.accounts.contains_key(&id) {
            return Ok(id);
        }
        self.verify_signer(&signer)?;
        self.accounts.insert(id.clone(), signer);
        self.order.push(id.clone());
        Ok(id)
    }

    /// Add an account WITHOUT the add-time signature post-condition.  Use
    /// only for restoration paths where the signer cannot perform a sign
    /// operation eagerly (e.g. NIP-46 with no connected transport).  Callers
    /// MUST run their own verification before relying on the signer.
    ///
    /// **PD-004:** same idempotent one-account-per-pubkey contract as
    /// [`AccountManager::add`] — a known pubkey returns its existing id and
    /// keeps the originally-installed signer.
    pub fn add_unverified(&mut self, signer: Arc<dyn Signer>) -> Result<IdentityId, AccountError> {
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
    pub fn active(&self) -> Option<IdentityId> {
        self.active.clone()
    }

    /// All ids, in insertion order.
    pub fn accounts(&self) -> Vec<IdentityId> {
        self.order.clone()
    }

    /// Signer for a specific id.
    pub fn signer_for(&self, id: &IdentityId) -> Option<Arc<dyn Signer>> {
        self.accounts.get(id).cloned()
    }

    /// Signer for the active id, if any.
    pub fn signer_active(&self) -> Option<Arc<dyn Signer>> {
        self.active.as_ref().and_then(|id| self.signer_for(id))
    }

    /// Register an observer for active-account changes.  Observers fire on
    /// every `switch_active` (no-op switches do NOT fire).
    pub fn observe(&mut self, observer: Arc<dyn ActiveChangeObserver>) {
        self.observers.push(observer);
    }

    /// Number of registered observers (test introspection).
    pub fn observer_count(&self) -> usize {
        self.observers.len()
    }

    fn verify_signer(&self, signer: &Arc<dyn Signer>) -> Result<(), AccountError> {
        let pubkey = signer.pubkey();
        let template = UnsignedEvent {
            pubkey: pubkey.to_hex(),
            kind: 1,
            tags: vec![vec!["t".to_string(), "nmp-post-condition".to_string()]],
            content: "nmp-signers post-condition probe".to_string(),
            created_at: 0,
        };

        // ----- Pre-compute the canonical event id (applesauce / synthesis
        // §1.1 mandatory post-condition).  If the signer mutates ANY field of
        // the event before signing — content, tags, kind, created_at, even
        // tag ordering — the id will not match and we refuse the account.
        let expected_id = compute_event_id(&template).ok_or_else(|| {
            AccountError::SignerMismatch(
                "could not pre-compute event id for post-condition probe".to_string(),
            )
        })?;

        let signed = signer
            .sign(template.clone())
            .wait(self.post_condition_timeout)
            .map_err(AccountError::SignerError)?;

        // ----- Mandatory post-conditions (synthesis §1.1).
        if signed.unsigned.pubkey != pubkey.to_hex() {
            return Err(AccountError::SignerMismatch(format!(
                "signed pubkey {} != claimed {}",
                signed.unsigned.pubkey,
                pubkey.to_hex()
            )));
        }
        if signed.id != expected_id {
            return Err(AccountError::SignerMismatch(format!(
                "signer mutated event before signing (id mismatch): expected {expected_id}, got {}",
                signed.id
            )));
        }
        Ok(())
    }
}

/// Pre-compute the canonical Nostr event id for an `UnsignedEvent` template.
///
/// Uses `nostr::EventBuilder` to drive the same hashing path the signer would
/// use.  Returns `None` if any field is malformed in a way that prevents id
/// computation (invalid pubkey hex, malformed tags) — the caller treats `None`
/// as a hard refusal.
fn compute_event_id(template: &UnsignedEvent) -> Option<String> {
    use nostr::{EventBuilder, Kind, PublicKey, Tag, Timestamp};
    let pk = PublicKey::from_hex(&template.pubkey).ok()?;
    let tags: Vec<Tag> = template
        .tags
        .iter()
        .filter_map(|t| Tag::parse(t).ok())
        .collect();
    let mut unsigned = EventBuilder::new(Kind::from_u16(template.kind as u16), &template.content)
        .tags(tags)
        .custom_created_at(Timestamp::from(template.created_at))
        .build(pk);
    Some(unsigned.id().to_hex())
}

