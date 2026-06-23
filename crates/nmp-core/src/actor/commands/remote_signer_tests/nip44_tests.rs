#![cfg(test)]
//! Tests for the `RemoteSignerHandle` NIP-44 seam (ADR-0026).
//!
//! The actor reaches NIP-44 through the same trait it uses for `sign()`.
//! These tests pin the new methods on the trait object via `StubRemoteSigner`.

use nostr::{Keys, SecretKey};
use nostr::nips::nip19::FromBech32;

use super::{StubRemoteSigner, TEST_NSEC, stub_signer};
use crate::remote_signer::RemoteSignerHandle;

// ──────────────────────────────────────────────────────────────────────────
// RemoteSignerHandle NIP-44 seam (ADR-0026): the actor reaches NIP-44
// through the same trait it uses for `sign()`. These tests pin the new
// methods on the trait object via the `StubRemoteSigner` double.
// ──────────────────────────────────────────────────────────────────────────

#[test]
fn remote_handle_nip44_round_trips_through_the_seam() {
    // ADR-0026: encrypt to a recipient, then decrypt from that recipient's
    // perspective — NIP-44 is symmetric in the shared conversation key, so a
    // ciphertext sealed by A to B decrypts with B's key against A's pubkey.
    let alice_sk = SecretKey::from_bech32(TEST_NSEC).expect("valid nsec");
    let alice = StubRemoteSigner::new(Keys::new(alice_sk));
    let bob = StubRemoteSigner::new(Keys::generate());

    let alice_pk = RemoteSignerHandle::pubkey_hex(&alice);
    let bob_pk = RemoteSignerHandle::pubkey_hex(&bob);

    let plaintext = "the kind:13 rumor body";
    let ciphertext = RemoteSignerHandle::nip44_encrypt(&alice, &bob_pk, plaintext)
        .wait(std::time::Duration::from_secs(1))
        .expect("encrypt resolves");
    assert_ne!(
        ciphertext, plaintext,
        "ciphertext must not be the plaintext"
    );

    let decrypted = RemoteSignerHandle::nip44_decrypt(&bob, &alice_pk, &ciphertext)
        .wait(std::time::Duration::from_secs(1))
        .expect("decrypt resolves");
    assert_eq!(
        decrypted, plaintext,
        "round-trip must recover the plaintext"
    );
}

#[test]
fn remote_handle_nip44_encrypt_with_malformed_pubkey_surfaces_err() {
    // D6: a bad hex pubkey through the actor-facing seam must surface as an
    // error, never a panic.
    let (signer, _count) = stub_signer();
    let err = RemoteSignerHandle::nip44_encrypt(&*signer, "not-hex", "plaintext")
        .wait(std::time::Duration::from_millis(100))
        .expect_err("malformed pubkey must surface as Err");
    match err {
        nmp_signer_iface::SignerError::Backend(m) => {
            assert!(m.contains("invalid recipient pubkey"), "got: {m}")
        }
        other => panic!("expected Backend Err, got {other:?}"),
    }
}
