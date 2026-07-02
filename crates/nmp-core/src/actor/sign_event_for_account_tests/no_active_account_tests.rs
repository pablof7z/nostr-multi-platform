//! No active account — the continuation resolves `Err` immediately (D6 — no
//! stuck spinner when there is nothing to sign with).

use super::signer_fixtures_support::{capture_continuation, draft_unsigned, fresh_identity};
use crate::actor::signer_port_test_harness::dispatch_one;
use crate::actor::{ActorCommand, SignCommand};
use crate::kernel::Kernel;
use crate::relay::DEFAULT_VISIBLE_LIMIT;

#[test]
fn no_account_invokes_continuation_with_err_immediately() {
    let mut identity = fresh_identity();
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);

    let (captured, continuation) = capture_continuation();
    let parked = dispatch_one(
        ActorCommand::Sign(SignCommand::EventForAccount {
            unsigned: draft_unsigned(""),
            signer_pubkey: None,
            continuation,
        }),
        &mut identity,
        &mut kernel,
    );
    assert!(parked.is_empty(), "no account → nothing parked");
    let outcome = captured
        .lock()
        .unwrap()
        .take()
        .expect("continuation must run immediately when there is no account");
    assert!(outcome.is_err(), "no active account is an Err outcome");
}
