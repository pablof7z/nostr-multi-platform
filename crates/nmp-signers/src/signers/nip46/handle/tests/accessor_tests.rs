//! `Nip46SignerHandle` accessor behavior: `from_bunker_uri*` parsing and
//! `local_pubkey`/`uri` reads.

use crate::signers::traits::Signer;
use crate::{LocalKeySigner, Nip46SignerHandle};

use super::fixtures::SAMPLE_PK;

#[test]
fn handle_from_bunker_uri_propagates_parse_error() {
    // A malformed URI must surface the typed parse error, not panic.
    assert!(Nip46SignerHandle::from_bunker_uri("not-a-bunker-uri").is_err());
}

#[test]
fn handle_with_explicit_local_key_uses_that_key() {
    // `from_bunker_uri_with_local_key` seeds a deterministic local key;
    // `local_pubkey()` must reflect it (used by tests that need a stable
    // ephemeral identity).
    let local = LocalKeySigner::generate();
    let local_sk =
        nostr::SecretKey::from_hex(local.secret_hex().as_str()).expect("valid secret hex");
    let uri = format!("bunker://{SAMPLE_PK}?relay=wss://relay.example.com");
    let handle = Nip46SignerHandle::from_bunker_uri_with_local_key(&uri, local_sk).expect("parse");
    assert_eq!(handle.local_pubkey(), local.pubkey());
    assert_eq!(handle.uri().remote_pubkey_hex, SAMPLE_PK);
}
