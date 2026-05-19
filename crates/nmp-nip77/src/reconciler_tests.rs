//! Unit tests for [`crate::reconciler`].  Extracted into a sibling file so
//! the production module stays under the 300 LOC soft cap (AGENTS.md).

use crate::reconciler::{Reconciler, ReconcilerOutcome, SyncedItem};

fn item(ts: u64, b: u8) -> SyncedItem {
    let mut id = [0u8; 32];
    id[31] = b;
    SyncedItem { created_at: ts, id }
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
