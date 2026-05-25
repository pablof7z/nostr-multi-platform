//! Transport-agnostic wrapper around the `negentropy` crate.

use std::collections::HashSet;
use std::fmt;

use negentropy::{Id, Negentropy, NegentropyStorageVector};

use crate::FRAME_SIZE_LIMIT;

/// One local item participating in reconciliation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SyncedItem {
    /// Event `created_at`, unix seconds.
    pub created_at: u64,
    /// Raw 32-byte event id.
    pub id: [u8; 32],
}

/// Result of one client step.
#[derive(Debug, Eq, PartialEq)]
pub enum ReconcilerOutcome {
    /// Send these bytes in `NEG-MSG` / initial `NEG-OPEN`.
    Send(Vec<u8>),
    /// Reconciliation converged.
    Done {
        /// Ids present locally but absent at the relay.
        have: Vec<[u8; 32]>,
        /// Ids present at the relay but absent locally.
        need: Vec<[u8; 32]>,
    },
}

/// Reconciler errors.
#[derive(Debug)]
pub enum ReconcilerError {
    /// Underlying negentropy engine returned an error.
    Engine(String),
    /// Client was asked to initiate twice.
    AlreadyInitiated,
}

impl fmt::Display for ReconcilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Engine(s) => write!(f, "negentropy engine error: {s}"),
            Self::AlreadyInitiated => f.write_str("client already initiated"),
        }
    }
}

impl std::error::Error for ReconcilerError {}

/// Client-side negentropy reconciler.
pub struct Reconciler {
    inner: Negentropy<'static, NegentropyStorageVector>,
    initiated: bool,
    have_acc: Vec<[u8; 32]>,
    need_acc: Vec<[u8; 32]>,
}

impl Reconciler {
    /// Build a client over the supplied local item set.
    pub fn client(items: Vec<SyncedItem>) -> Result<Self, ReconcilerError> {
        let storage = build_storage(items)?;
        let inner = Negentropy::owned(storage, FRAME_SIZE_LIMIT)
            .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
        Ok(Self {
            inner,
            initiated: false,
            have_acc: Vec::new(),
            need_acc: Vec::new(),
        })
    }

    /// Produce the initial message for `NEG-OPEN`.
    pub fn initiate(&mut self) -> Result<Vec<u8>, ReconcilerError> {
        if self.initiated {
            return Err(ReconcilerError::AlreadyInitiated);
        }
        let bytes = self
            .inner
            .initiate()
            .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
        self.initiated = true;
        Ok(bytes)
    }

    /// Consume one relay response and return the next client outcome.
    pub fn reconcile(&mut self, peer: &[u8]) -> Result<ReconcilerOutcome, ReconcilerError> {
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
            None => ReconcilerOutcome::Done {
                have: std::mem::take(&mut self.have_acc),
                need: std::mem::take(&mut self.need_acc),
            },
        })
    }
}

fn build_storage(items: Vec<SyncedItem>) -> Result<NegentropyStorageVector, ReconcilerError> {
    let mut storage = NegentropyStorageVector::with_capacity(items.len());
    let mut seen = HashSet::with_capacity(items.len());
    for item in items {
        if !seen.insert(item.id) {
            continue;
        }
        storage
            .insert(item.created_at, Id::from_byte_array(item.id))
            .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
    }
    storage
        .seal()
        .map_err(|e| ReconcilerError::Engine(format!("{e:?}")))?;
    Ok(storage)
}

#[cfg(test)]
mod tests {
    use super::*;
    use negentropy::{Negentropy, NegentropyStorageVector};

    fn item(n: u8, ts: u64) -> SyncedItem {
        let mut id = [0u8; 32];
        id[0] = n;
        SyncedItem { created_at: ts, id }
    }

    #[test]
    fn client_converges_against_real_negentropy_server() {
        let local = vec![item(1, 10), item(2, 20)];
        let server_items = vec![item(2, 20), item(3, 30)];

        let mut client = Reconciler::client(local).unwrap();
        let mut server_storage = NegentropyStorageVector::new();
        for item in server_items {
            server_storage
                .insert(item.created_at, Id::from_byte_array(item.id))
                .unwrap();
        }
        server_storage.seal().unwrap();
        let mut server = Negentropy::owned(server_storage, FRAME_SIZE_LIMIT).unwrap();

        let mut msg = client.initiate().unwrap();
        loop {
            let response = server.reconcile(&msg).unwrap();
            match client.reconcile(&response).unwrap() {
                ReconcilerOutcome::Send(next) => msg = next,
                ReconcilerOutcome::Done { have, need } => {
                    assert_eq!(have, vec![item(1, 10).id]);
                    assert_eq!(need, vec![item(3, 30).id]);
                    break;
                }
            }
        }
    }
}
