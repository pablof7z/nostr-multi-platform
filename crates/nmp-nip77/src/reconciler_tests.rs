//! Unit tests for [`crate::reconciler`].  Extracted into a sibling file so
//! the production module stays under the 300 LOC soft cap (AGENTS.md).

use crate::reconciler::{Reconciler, ReconcilerError, ReconcilerOutcome, SyncedItem};
use std::collections::HashSet;

fn item(ts: u64, b: u8) -> SyncedItem {
    let mut id = [0u8; 32];
    id[31] = b;
    SyncedItem { created_at: ts, id }
}

/// Set of trailing id bytes — the only byte `item` varies, so this uniquely
/// identifies which synthetic items a `have`/`need` list refers to.
fn id_byte_set(ids: &[[u8; 32]]) -> HashSet<u8> {
    ids.iter().map(|id| id[31]).collect()
}

/// Two empty sets — protocol converges immediately.
#[test]
fn empty_sets_converge() {
    let mut client = Reconciler::client(vec![]).unwrap();
    let mut server = Reconciler::server(vec![]).unwrap();
    drive_to_completion(&mut client, &mut server, 8);
}

/// Client missing all items the server has — converges with non-empty
/// `need` on the client side.
#[test]
fn client_pulls_all_from_server() {
    let server_items: Vec<SyncedItem> = (1..=64u8).map(|b| item(100 + b as u64, b)).collect();
    let mut client = Reconciler::client(vec![]).unwrap();
    let mut server = Reconciler::server(server_items.clone()).unwrap();
    let outcome = drive_to_completion(&mut client, &mut server, 32);
    match outcome {
        ReconcilerOutcome::Done { need, .. } => {
            assert_eq!(need.len(), server_items.len());
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Identical sets — protocol converges with empty `have` and `need`.
#[test]
fn identical_sets_have_no_diff() {
    let items: Vec<SyncedItem> = (1..=16u8).map(|b| item(200 + b as u64, b)).collect();
    let mut client = Reconciler::client(items.clone()).unwrap();
    let mut server = Reconciler::server(items).unwrap();
    let outcome = drive_to_completion(&mut client, &mut server, 16);
    if let ReconcilerOutcome::Done { have, need, .. } = outcome {
        assert!(have.is_empty(), "no have ids for identical sets");
        assert!(need.is_empty(), "no need ids for identical sets");
    }
}

/// Mostly-overlapping sets — client holds {1..8}, server holds {5..12}.
/// The reconciliation diff must surface exactly the four ids the client is
/// missing ({9,10,11,12}) and nothing else.
#[test]
fn mostly_overlapping_sets_diff_only_the_gap() {
    let client_items: Vec<SyncedItem> = (1..=8u8).map(|b| item(300 + b as u64, b)).collect();
    let server_items: Vec<SyncedItem> = (5..=12u8).map(|b| item(300 + b as u64, b)).collect();
    let mut client = Reconciler::client(client_items).unwrap();
    let mut server = Reconciler::server(server_items).unwrap();
    let outcome = drive_to_completion(&mut client, &mut server, 32);
    match outcome {
        ReconcilerOutcome::Done { need, .. } => {
            // Set equality — negentropy does not promise diff ordering.
            assert_eq!(id_byte_set(&need), HashSet::from([9u8, 10, 11, 12]));
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Duplicate ids in the input vec must be collapsed to a set before sealing
/// (`build_sealed_storage` dedupes via HashSet). A client seeded with the
/// same id three times reconciling against a server holding it once must
/// converge cleanly with an empty diff.
#[test]
fn duplicate_input_ids_are_idempotent() {
    let dup = item(500, 7);
    let client = Reconciler::client(vec![dup.clone(), dup.clone(), dup.clone()]);
    let mut client = client.expect("duplicate ids must not fail sealing");
    let mut server = Reconciler::server(vec![item(500, 7)]).unwrap();
    let outcome = drive_to_completion(&mut client, &mut server, 16);
    match outcome {
        ReconcilerOutcome::Done { have, need, .. } => {
            assert!(have.is_empty(), "deduped client has nothing extra");
            assert!(need.is_empty(), "deduped client needs nothing");
        }
        other => panic!("expected Done, got {other:?}"),
    }
}

/// Calling `step(None)` on a server reconciler is a logic bug — the server
/// never initiates and must report it deterministically rather than panic.
#[test]
fn server_step_without_peer_payload_errors() {
    let mut server = Reconciler::server(vec![item(1, 1)]).unwrap();
    let err = server.step(None).unwrap_err();
    assert!(matches!(err, ReconcilerError::ServerNotInitiator));
}

/// A client that has already produced its initial message must not be asked
/// to `step(None)` again — the engine arm returns a deterministic error
/// rather than re-initiating or panicking.
#[test]
fn client_step_none_after_initiate_errors() {
    let mut client = Reconciler::client(vec![item(1, 1)]).unwrap();
    // First call initiates.
    assert!(matches!(client.step(None).unwrap(), ReconcilerOutcome::Send(_)));
    // Second `None` call is a caller bug — surfaced as an engine error.
    let err = client.step(None).unwrap_err();
    assert!(matches!(err, ReconcilerError::Engine(_)));
}

/// `resume_client` re-seeds with the current item set; the resume blob is a
/// coverage hint only, so resuming with an empty/garbage blob must still
/// reconcile correctly against a peer.
#[test]
fn resume_client_reconciles_like_a_fresh_client() {
    let items: Vec<SyncedItem> = (1..=8u8).map(|b| item(600 + b as u64, b)).collect();
    let mut client = Reconciler::resume_client(items.clone(), &[]).unwrap();
    let mut server = Reconciler::server(items).unwrap();
    let outcome = drive_to_completion(&mut client, &mut server, 16);
    if let ReconcilerOutcome::Done { have, need, .. } = outcome {
        assert!(have.is_empty());
        assert!(need.is_empty());
    } else {
        panic!("expected Done");
    }
}

fn drive_to_completion(
    client: &mut Reconciler,
    server: &mut Reconciler,
    max_rounds: usize,
) -> ReconcilerOutcome {
    let mut peer_to_client: Option<Vec<u8>> = None;
    for _ in 0..max_rounds {
        let outcome = client.step(peer_to_client.as_deref()).unwrap();
        let bytes = match outcome {
            ReconcilerOutcome::Send(b) => b,
            done @ ReconcilerOutcome::Done { .. } => return done,
        };
        let response = server.step(Some(&bytes)).unwrap();
        peer_to_client = match response {
            ReconcilerOutcome::Send(b) => Some(b),
            ReconcilerOutcome::Done { .. } => continue,
        };
    }
    panic!("reconciliation did not converge in {max_rounds} rounds");
}
