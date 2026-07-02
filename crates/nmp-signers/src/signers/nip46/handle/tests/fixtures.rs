//! Shared fixtures for the NIP-46 `handle` test suite.
//!
//! Provides the stub/failing transports and signer-construction helpers
//! reused across the behavior-area test files in this module.

use std::sync::{Arc, Mutex};

use nmp_signer_iface::{Nip46Rpc, Nip46Transport, SignerError};

use crate::signers::traits::Signer;
use crate::{LocalKeySigner, Nip46Signer, Nip46SignerHandle};

pub(super) const SAMPLE_PK: &str =
    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[derive(Debug, Default)]
pub(super) struct StubTransport {
    pub(super) sent: Mutex<Vec<Nip46Rpc>>,
}

impl Nip46Transport for StubTransport {
    fn send_rpc(&self, rpc: Nip46Rpc) -> Result<(), SignerError> {
        self.sent.lock().unwrap().push(rpc);
        Ok(())
    }
}

/// A transport whose `send_rpc` always fails — exercises `enqueue`'s
/// transmit-failure branch.
#[derive(Debug, Default)]
pub(super) struct FailingTransport;

impl Nip46Transport for FailingTransport {
    fn send_rpc(&self, _rpc: Nip46Rpc) -> Result<(), SignerError> {
        Err(SignerError::Backend("relay pool offline".to_string()))
    }
}

pub(super) fn build_signer_with_remote(
    remote_user: &LocalKeySigner,
) -> (Nip46Signer, Arc<StubTransport>) {
    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com&secret=s1");
    let handle = Nip46SignerHandle::from_bunker_uri(&uri).expect("parse");
    let transport = Arc::new(StubTransport::default());
    let signer = handle.complete(transport.clone(), remote_user.pubkey());
    (signer, transport)
}

/// Drain the single queued RPC, asserting exactly one was sent.
pub(super) fn single_rpc(transport: &StubTransport) -> Nip46Rpc {
    let sent = transport.sent.lock().unwrap();
    assert_eq!(sent.len(), 1, "expected exactly one queued RPC");
    sent[0].clone()
}
