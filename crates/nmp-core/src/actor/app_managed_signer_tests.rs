use nostr::nips::nip19::ToBech32;

use super::commands::{
    add_signer, new_bunker_handshake_slot, new_signer_state_slot, sign_with_account_nonblocking,
    switch_active, IdentityRuntime,
};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

const TEST_NSEC: &str = "nsec1vl029mgpspedva04g90vltkh6fvh240zqtv9k0t9af8935ke9laqsnlfe5";

fn fresh() -> (IdentityRuntime, Kernel) {
    (
        IdentityRuntime::new(new_bunker_handshake_slot(), new_signer_state_slot()),
        Kernel::new(DEFAULT_VISIBLE_LIMIT),
    )
}

#[test]
fn app_managed_local_signer_is_hidden_signable_and_not_switchable() {
    let (mut identity, mut kernel) = fresh();
    add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::LocalNsec(zeroize::Zeroizing::new(TEST_NSEC.to_string())),
        true,
        false,
    );
    let active = identity.active_pubkey().unwrap();
    let hidden_keys = nostr::Keys::generate();
    let hidden_pubkey = hidden_keys.public_key().to_hex();
    let hidden_nsec = hidden_keys.secret_key().to_bech32().unwrap();

    add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::AppManagedLocalNsec(zeroize::Zeroizing::new(hidden_nsec)),
        true,
        false,
    );

    assert!(identity.contains_account(&hidden_pubkey));
    let (accounts, account_active) = kernel.account_snapshot();
    assert_eq!(account_active, Some(&active));
    assert!(accounts.iter().all(|account| account.id != hidden_pubkey));

    switch_active(&mut identity, &mut kernel, &hidden_pubkey, false);
    assert_eq!(identity.active_pubkey(), Some(active));
    assert!(kernel
        .last_error_toast_snapshot()
        .is_some_and(|toast| toast.contains("app-managed")));

    let unsigned = nmp_signer_iface::UnsignedEvent {
        pubkey: "ignored-by-signer".into(),
        kind: 1,
        tags: Vec::new(),
        content: "hidden signer publish".into(),
        created_at: 1_700_000_000,
    };
    let signed = sign_with_account_nonblocking(&identity, &hidden_pubkey, &unsigned)
        .expect("hidden signer resolves by pubkey")
        .poll()
        .expect("local hidden signer resolves inline")
        .expect("hidden local sign succeeds");
    assert_eq!(signed.unsigned.pubkey, hidden_pubkey);
}
