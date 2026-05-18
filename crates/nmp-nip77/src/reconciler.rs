//! Transport-agnostic NIP-77 reconciler.
//!
//! Wraps `negentropy::Negentropy` so callers exchange opaque byte payloads
//! without knowing about WebSockets, JSON envelopes, or relay framing.  The
//! reconciler's `step()` consumes a peer payload (or `None` to initiate) and
//! returns one of:
//!
//! | Outcome | Meaning |
//! |---|---|
//! | `Send(bytes)` | Forward `bytes` to the peer, then call `step` again with the response. |
//! | `Done { synced, have, need, state }` | Reconciliation converged.  `have` are ids the peer must accept from us; `need` are ids we should pull. `state` is an opaque resume blob suitable for [`crate::SyncStrategy::resume_state`] persistence. |
//!
//! ## Roles
//!
//! Negentropy is asymmetric — the *client* drives the protocol, the *server*
//! responds to fingerprint queries.  The reconciler factory mirrors this:
//!
//! * [`Reconciler::client`] builds an initiating client (`set_initiator` is
//!   true by construction; first call to [`Reconciler::step`] with `None`
//!   produces the initial query).
//! * [`Reconciler::server`] builds a responding server.  Calling
//!   [`Reconciler::step`] without a peer payload on a server is a programmer
//!   error and returns [`ReconcilerError::ServerNotInitiator`].
//!
//! ## Doctrine
//!
//! * **D2** — reconciliation is the engine that lets the planner choose
//!   negentropy over REQ when both are available.
//! * **D8** — the reconciler holds an in-memory storage vector sized by the
//!   set under reconciliation (working-set bounded).  Frame size is capped at
//!   [`crate::FRAME_SIZE_LIMIT`].

use negentropy::{Id, Negentropy, NegentropyStorageVector};
use std::collections::HashSet;
use std::fmt;

use crate::FRAME_SIZE_LIMIT;

/// Role of the reconciler instance — chosen at construction time.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReconcilerRole {
    /// Drives the protocol: produces the initial message.
    Client,
    /// Responds to incoming queries.
    Server,
}

/// One item known to either side of the reconciliation.
///
/// `created_at` is the Nostr event timestamp (unix seconds); `id` is the
/// 32-byte event id (`Id` in `negentropy`).  Identical to what
/// `negentropy::NegentropyStorageVector::insert` expects.
#[derive(Clone, Debug)]
pub struct SyncedItem {
    pub created_at: u64,
    pub id: [u8; 32],
}

/// Result of one [`Reconciler::step`] call.
#[derive(Debug)]
pub enum ReconcilerOutcome {
    /// More rounds required — send `bytes` to the peer, then feed the peer's
    /// response back into [`Reconciler::step`].
    Send(Vec<u8>),
    /// Reconciliation converged.  `have` ids should be pushed to the peer
    /// (server-side use); `need` ids should be REQ-fetched (client-side use).
    /// `state` is an opaque resume blob meant for persistence in
    /// [`nmp_core::store::WatermarkRow::last_negentropy_state`].
    Done {
        have: Vec<[u8; 32]>,
        need: Vec<[u8; 32]>,
        state: Vec<u8>,
    },
}

/// Errors a reconciler can raise.  Internal to the M4 substrate — D6 forbids
/// these from crossing FFI; map to `toast` at the action boundary.
#[derive(Debug)]
pub enum ReconcilerError {
    /// Server reconcilers must receive a peer payload on every `step`; calling
    /// `step(None)` on a server is a logic bug.
    ServerNotInitiator,
    /// Underlying `negentropy` engine returned an error (protocol mismatch,
    /// invalid frame, storage exhausted, etc.).  The variant carries the
    /// engine's `Debug` rendering — the precise structured error is internal.
    Engine(String),
}

impl fmt::Display for ReconcilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ServerNotInitiator => f.write_str("server reconciler requires a peer payload"),
            Self::Engine(s) => write!(f, "negentropy engine error: {s}"),
        }
    }
}

impl std::error::Error for ReconcilerError {}

/// Owned wrapper around `negentropy::Negentropy` plus role bookkeeping.
///
/// `have` and `need` accumulate across every [`Reconciler::step`] call —
/// the underlying engine populates them one round at a time but the
/// caller only sees them aggregated on [`ReconcilerOutcome::Done`].
pub struct Reconciler {
    inner: Negentropy<'static, NegentropyStorageVector>,
    role: ReconcilerRole,
    initiated: bool,
    have_acc: Vec<[u8; 32]>,
    need_acc: Vec<[u8; 32]>,
}

impl Reconciler {
    /// Build a client (initiator) reconciler over `items`.
    pub fn client(items: Vec<SyncedItem>) -> Result<Self, ReconcilerError> {
        Self::with_role(items, ReconcilerRole::Client)
    }

    /// Build a server (responder) reconciler over `items`.
    pub fn server(items: Vec<SyncedItem>) -> Result<Self, ReconcilerError> {
        Self::with_role(items, ReconcilerRole::Server)
    }

    fn with_role(items: Vec<SyncedItem>, role: ReconcilerRole) -> Result<Self, ReconcilerError> {
        let storage = build_sealed_storage(items)?;
        // Note: do NOT call `set_initiator()` here.  `Negentropy::initiate()`
        // sets the initiator flag itself and errors if the flag is already
        // set (`AlreadyBuiltInitialMessage`).  We only flip the flag manually
        // in [`Reconciler::resume_client`], where `initiate()` is skipped.
        let inner = Negentropy::owned(storage, FRAME_SIZE_LIMIT)
            .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
        Ok(Self {
            inner,
            role,
            initiated: false,
            have_acc: Vec::new(),
            need_acc: Vec::new(),
        })
    }

    /// Construct from a previously-persisted resume blob plus the current
    /// item set.  At the moment the `negentropy` 0.5 crate does not expose a
    /// public deserializer, so we re-seed a client with the current items
    /// (correctness preserved — the protocol converges deterministically) and
    /// surface the `state` blob only as a *coverage hint*.  Persisting the
    /// blob is still useful so future engine versions can resume mid-frame.
    pub fn resume_client(
        items: Vec<SyncedItem>,
        _state: &[u8],
    ) -> Result<Self, ReconcilerError> {
        Self::client(items)
    }

    /// Drive one reconciliation step.  See [`ReconcilerOutcome`] for the
    /// returned variants.  Pass `peer_bytes = None` for the first client
    /// call; afterwards pass the peer's response on every iteration.
    pub fn step(
        &mut self,
        peer_bytes: Option<&[u8]>,
    ) -> Result<ReconcilerOutcome, ReconcilerError> {
        match (self.role, peer_bytes, self.initiated) {
            (ReconcilerRole::Server, None, _) => Err(ReconcilerError::ServerNotInitiator),
            (ReconcilerRole::Client, None, false) => {
                let bytes = self
                    .inner
                    .initiate()
                    .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
                self.initiated = true;
                Ok(ReconcilerOutcome::Send(bytes))
            }
            (ReconcilerRole::Client, Some(peer), _) => {
                let mut have = Vec::<Id>::new();
                let mut need = Vec::<Id>::new();
                let next = self
                    .inner
                    .reconcile_with_ids(peer, &mut have, &mut need)
                    .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
                self.have_acc.extend(have.iter().map(|id| *id.as_bytes()));
                self.need_acc.extend(need.iter().map(|id| *id.as_bytes()));
                Ok(match next {
                    Some(bytes) => ReconcilerOutcome::Send(bytes),
                    None => {
                        let have = std::mem::take(&mut self.have_acc);
                        let need = std::mem::take(&mut self.need_acc);
                        let state = persistable_state_bytes(&have, &need);
                        ReconcilerOutcome::Done { have, need, state }
                    }
                })
            }
            (ReconcilerRole::Server, Some(peer), _) => {
                // Always forward the server's frame to the client — even a
                // 1-byte "protocol version only" payload, which is how the
                // server signals "I have nothing more to say." The client
                // observes that frame and converges to `Done`.
                let bytes = self
                    .inner
                    .reconcile(peer)
                    .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
                Ok(ReconcilerOutcome::Send(bytes))
            }
            (ReconcilerRole::Client, None, true) => {
                // already initiated and waiting for peer; tell caller they
                // must supply peer_bytes (treat as engine error so callers
                // observe a deterministic non-panic path).
                Err(ReconcilerError::Engine(
                    "client already initiated; peer_bytes required for subsequent steps".into(),
                ))
            }
        }
    }

    pub fn role(&self) -> ReconcilerRole {
        self.role
    }
}

fn build_sealed_storage(
    items: Vec<SyncedItem>,
) -> Result<NegentropyStorageVector, ReconcilerError> {
    let mut storage = NegentropyStorageVector::with_capacity(items.len());
    // Deduplicate ids — negentropy requires a set, not a multiset.
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        if !seen.insert(item.id) {
            continue;
        }
        let id = Id::from_byte_array(item.id);
        storage
            .insert(item.created_at, id)
            .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
    }
    storage
        .seal()
        .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
    Ok(storage)
}

/// Compact state hint emitted alongside `Done`: 4-byte LE counts followed by
/// the raw id payloads.  The wire layer never inspects this blob — it's
/// meant for round-tripping into [`nmp_core::store::WatermarkRow::last_negentropy_state`].
fn persistable_state_bytes(have: &[[u8; 32]], need: &[[u8; 32]]) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + 32 * (have.len() + need.len()));
    out.extend((have.len() as u32).to_le_bytes());
    out.extend((need.len() as u32).to_le_bytes());
    for id in have {
        out.extend(id);
    }
    for id in need {
        out.extend(id);
    }
    out
}

// Unit tests live in a sibling file so this production module stays under
// the 300 LOC soft cap (AGENTS.md).  The test module is registered from
// `lib.rs` via `#[cfg(test)] #[path = "reconciler_tests.rs"] mod reconciler_tests;`.
