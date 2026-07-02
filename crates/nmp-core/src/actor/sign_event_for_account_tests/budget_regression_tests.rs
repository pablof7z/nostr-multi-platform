//! §D4 — `SignEventForReturn` named-account budget regression.
//!
//! Mirrors `cipher_for_account_tests::named_roster_key_keeps_its_own_budget_not_
//! the_active_accounts` on the sign-and-return path (`dispatch.rs:606`). A named
//! 90s NIP-55-style roster key signed while a 5s NIP-46-style account is active
//! must park with ITS OWN 90s budget — never inherit the active account's 5s
//! deadline. The D4 bug was `active_sign_deadline()` at the park site; the fix
//! computes `sign_deadline_for(named)`.

use nmp_signer_iface::RemoteSignerHandle;
use nostr::Keys;

use super::signer_fixtures_support::{fresh_identity, PendingRemoteSigner};
use crate::actor::commands;
use crate::actor::signer_port_test_harness::dispatch_one;
use crate::actor::{ActorCommand, SignCommand};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn sign_event_for_return_named_roster_key_keeps_its_own_budget() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    // Active account: a 5s-budget bunker (NIP-46-style). Pending sign so any
    // accidental routing-through-active would park with the 5s deadline.
    let active =
        PendingRemoteSigner::with_op_timeout(Keys::generate(), std::time::Duration::from_secs(5));
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(active)),
        true,
        false,
    );

    // A SECOND, non-active roster key: a 90s-budget signer (NIP-55-style).
    let named =
        PendingRemoteSigner::with_op_timeout(Keys::generate(), std::time::Duration::from_secs(90));
    let named_pk = named.pubkey_hex();
    commands::add_signer(
        &mut identity,
        &mut kernel,
        crate::actor::SignerSource::RemoteHandle(Box::new(named)),
        false, // do NOT make active — the 5s account stays active.
        false,
    );

    // Sign-and-return with the NAMED key while the 5s account is active.
    let before = std::time::Instant::now();
    let parked = dispatch_one(
        ActorCommand::Sign(SignCommand::EventForReturn {
            account_pubkey: named_pk,
            unsigned_json: r#"{"kind":24242,"content":"auth","tags":[]}"#.to_string(),
            correlation_id: "corr-named-budget".to_string(),
        }),
        &mut identity,
        &mut kernel,
    );
    assert_eq!(parked.len(), 1, "named bunker sign-and-return parks");

    // The parked deadline must reflect the NAMED key's 90s budget, NOT the
    // active account's 5s. Generous slack for dispatch latency.
    let deadline = parked[0].deadline;
    let budget = deadline.saturating_duration_since(before);
    assert!(
        budget > std::time::Duration::from_secs(60),
        "named 90s key must keep its own budget on the sign-and-return path (got \
         {budget:?}); the D4 bug would have parked it with the active account's 5s budget"
    );
}
