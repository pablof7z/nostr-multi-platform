//! Execution-scoped capabilities available to [`super::ActionModule`].
//!
//! `ActionContext` is the runtime capability bag for action modules. The first
//! capability is a bounded, cache-only reader over the kernel event store:
//! modules can inspect already-local events while composing an `ActorCommand`,
//! without opening relay subscriptions, waiting for EOSE, or carrying
//! per-module store plumbing.

use std::fmt;
use std::sync::Arc;

use crate::slots::EventStoreSlot;
use nmp_store::{EventStore, StoreQuery, StoredEvent};

/// Maximum number of rows an action-local store query may materialize.
pub const ACTION_LOCAL_STORE_MAX_EVENTS: usize = 256;

/// Action-runtime capability bag.
///
/// The default context has no local store. Runtime dispatch paths install the
/// current kernel store handle; unit tests for store-agnostic modules can keep
/// using `ActionContext::default()`.
#[derive(Clone, Default)]
pub struct ActionContext {
    local_store: Option<ActionLocalStore>,
}

impl ActionContext {
    /// Build a context backed by the host-published event-store slot.
    #[must_use]
    pub fn with_event_store_slot(slot: EventStoreSlot) -> Self {
        Self {
            local_store: Some(ActionLocalStore::from_slot(slot)),
        }
    }

    /// Build a context backed by a direct event-store handle.
    #[must_use]
    pub fn with_event_store(store: Arc<dyn EventStore>) -> Self {
        Self {
            local_store: Some(ActionLocalStore::from_store(store)),
        }
    }

    /// Read one local event by 64-character hex id.
    ///
    /// Malformed ids return `Ok(None)`. Store availability and backend failures
    /// return data errors so callers can decide whether the read is optional
    /// degradation or a synchronous action failure before enqueue.
    pub fn local_event_by_id(&self, id_hex: &str) -> Result<Option<StoredEvent>, ActionReadError> {
        let store = self.local_store()?;
        store.event_by_id(id_hex)
    }

    /// Query already-local events, newest-first per the selected [`StoreQuery`].
    ///
    /// This never opens a relay, registers an interest, or waits for network
    /// acquisition. The caller-supplied `limit` must not exceed
    /// [`ACTION_LOCAL_STORE_MAX_EVENTS`].
    pub fn query_local_events(
        &self,
        query: &StoreQuery,
        limit: usize,
    ) -> Result<Vec<StoredEvent>, ActionReadError> {
        let store = self.local_store()?;
        store.query(query, limit)
    }

    fn local_store(&self) -> Result<&ActionLocalStore, ActionReadError> {
        self.local_store
            .as_ref()
            .ok_or(ActionReadError::StoreUnavailable)
    }
}

impl fmt::Debug for ActionContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionContext")
            .field("has_local_store", &self.local_store.is_some())
            .finish()
    }
}

/// Bounded local-store reader held by [`ActionContext`].
#[derive(Clone)]
pub struct ActionLocalStore {
    source: ActionLocalStoreSource,
    max_events: usize,
}

impl ActionLocalStore {
    #[must_use]
    fn from_slot(slot: EventStoreSlot) -> Self {
        Self {
            source: ActionLocalStoreSource::Slot(slot),
            max_events: ACTION_LOCAL_STORE_MAX_EVENTS,
        }
    }

    #[must_use]
    fn from_store(store: Arc<dyn EventStore>) -> Self {
        Self {
            source: ActionLocalStoreSource::Direct(store),
            max_events: ACTION_LOCAL_STORE_MAX_EVENTS,
        }
    }

    fn event_by_id(&self, id_hex: &str) -> Result<Option<StoredEvent>, ActionReadError> {
        use crate::kernel::hex_to_pubkey_bytes as hex_to_id_bytes;

        let Some(key) = hex_to_id_bytes(id_hex) else {
            return Ok(None);
        };
        let store = self.resolve()?;
        store
            .peek_by_id(&key)
            .map_err(|e| ActionReadError::Store(e.to_string()))
    }

    fn query(&self, query: &StoreQuery, limit: usize) -> Result<Vec<StoredEvent>, ActionReadError> {
        if limit > self.max_events {
            return Err(ActionReadError::LimitExceeded {
                requested: limit,
                max: self.max_events,
            });
        }
        let store = self.resolve()?;
        store
            .query(query, limit)
            .map_err(|e| ActionReadError::Store(e.to_string()))
    }

    fn resolve(&self) -> Result<Arc<dyn EventStore>, ActionReadError> {
        match &self.source {
            ActionLocalStoreSource::Direct(store) => Ok(Arc::clone(store)),
            ActionLocalStoreSource::Slot(slot) => slot
                .lock()
                .map_err(|_| ActionReadError::StoreLockPoisoned)?
                .clone()
                .ok_or(ActionReadError::StoreUnavailable),
        }
    }
}

impl fmt::Debug for ActionLocalStore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActionLocalStore")
            .field("source", &self.source)
            .field("max_events", &self.max_events)
            .finish()
    }
}

#[derive(Clone)]
enum ActionLocalStoreSource {
    Direct(Arc<dyn EventStore>),
    Slot(EventStoreSlot),
}

impl fmt::Debug for ActionLocalStoreSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Direct(_) => f.write_str("Direct"),
            Self::Slot(_) => f.write_str("Slot"),
        }
    }
}

/// Data errors from action-local store reads.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ActionReadError {
    StoreUnavailable,
    StoreLockPoisoned,
    Store(String),
    LimitExceeded { requested: usize, max: usize },
}

impl fmt::Display for ActionReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::StoreUnavailable => write!(f, "action local store is unavailable"),
            Self::StoreLockPoisoned => write!(f, "action local store slot lock is poisoned"),
            Self::Store(message) => write!(f, "action local store read failed: {message}"),
            Self::LimitExceeded { requested, max } => write!(
                f,
                "action local store query limit {requested} exceeds maximum {max}"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::slots::new_event_store_slot;
    use nmp_store::{MemEventStore, RawEvent, VerifiedEvent};

    fn event(id: &str, kind: u32, created_at: u64) -> VerifiedEvent {
        let raw = RawEvent {
            id: id.to_string(),
            pubkey: "11".repeat(32),
            created_at,
            kind,
            tags: Vec::new(),
            content: format!("kind-{kind}"),
            sig: "aa".repeat(64),
        };
        VerifiedEvent::from_raw_unchecked(raw)
    }

    fn store_with_events() -> MemEventStore {
        let store = MemEventStore::default();
        store
            .insert(event(&"22".repeat(32), 9, 2), &"wss://r".to_string(), 2)
            .unwrap();
        store
            .insert(event(&"33".repeat(32), 1, 1), &"wss://r".to_string(), 1)
            .unwrap();
        store
    }

    #[test]
    fn default_context_reports_store_unavailable() {
        let ctx = ActionContext::default();
        let query = StoreQuery::KindTime {
            kinds: vec![9],
            since: None,
            until: None,
        };
        assert!(matches!(
            ctx.local_event_by_id(&"22".repeat(32)),
            Err(ActionReadError::StoreUnavailable)
        ));
        assert!(matches!(
            ctx.query_local_events(&query, 1),
            Err(ActionReadError::StoreUnavailable)
        ));
    }

    #[test]
    fn direct_store_event_by_id_reads_present_event() {
        let ctx = ActionContext::with_event_store(Arc::new(store_with_events()));
        let found = ctx
            .local_event_by_id(&"22".repeat(32))
            .expect("store read succeeds")
            .expect("event exists");
        assert_eq!(found.raw.kind, 9);
        assert!(ctx.local_event_by_id("not-hex").unwrap().is_none());
    }

    #[test]
    fn slot_backed_query_reads_local_events() {
        let slot = new_event_store_slot();
        *slot.lock().unwrap() = Some(Arc::new(store_with_events()));
        let ctx = ActionContext::with_event_store_slot(slot);
        let query = StoreQuery::KindTime {
            kinds: vec![9],
            since: None,
            until: None,
        };
        let rows = ctx
            .query_local_events(&query, 10)
            .expect("store query succeeds");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].raw.id, "22".repeat(32));
    }

    #[test]
    fn query_limit_is_enforced_before_store_read() {
        let ctx = ActionContext::with_event_store(Arc::new(store_with_events()));
        let query = StoreQuery::KindTime {
            kinds: Vec::new(),
            since: None,
            until: None,
        };
        assert!(matches!(
            ctx.query_local_events(&query, ACTION_LOCAL_STORE_MAX_EVENTS + 1),
            Err(ActionReadError::LimitExceeded {
                requested,
                max,
            }) if requested == ACTION_LOCAL_STORE_MAX_EVENTS + 1
                && max == ACTION_LOCAL_STORE_MAX_EVENTS
        ));
    }
}
