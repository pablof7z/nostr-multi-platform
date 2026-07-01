//! Store-backed NIP-44 decrypt-session replay for NIP-17 inbox backfill.
//!
//! This is the #1259 batch-capable path. Candidate envelopes come from the
//! canonical `EventStore` query for kind:1059 `#p <active-pubkey>`; scalar
//! `undecrypted_count` remains status only and is never used as a replay source.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use nmp_core::actor::nip44_decrypt_session_port::{
    Nip44DecryptBatchItemPortOutcome, Nip44DecryptBatchPortResult,
    Nip44DecryptSessionBeginPortResult,
};
use nmp_core::actor::{ActorCommand, SignCommand};
use nmp_core::slots::EventStoreSlot;
use nmp_core::CommandSender;
use nmp_signer_iface::{
    Nip44DecryptBatchItem, Nip44DecryptBatchRequest, Nip44DecryptSessionBeginRequest,
    Nip44DecryptSessionEndRequest, Nip44DecryptSessionGrant, NMP_NIP44_BACKFILL_SCOPE,
};
use nmp_store::StoreQuery;
use nostr::{Event, JsonUtil, PublicKey, SingleLetterTag};

use super::store::{BatchBackfillFinish, InboxStore};

const MAX_BATCH_BACKFILL_ENVELOPES: usize = 256;

#[cfg(any(test, feature = "test-support"))]
impl super::DmInboxProjection {
    pub fn launch_batch_backfill_for_test(&self, store: &EventStoreSlot) -> bool {
        self.launch_batch_backfill(store)
    }
}

#[derive(Clone)]
struct OuterItem {
    id: String,
    peer_pubkey: String,
    ciphertext: String,
    source_relay_url: Option<String>,
}

#[derive(Clone)]
struct InnerItem {
    id: String,
    seal: Event,
    peer_pubkey: String,
    ciphertext: String,
    source_relay_url: Option<String>,
}

#[derive(Clone)]
struct BatchReplay {
    tx: CommandSender,
    store: Arc<InboxStore>,
    signer_hex: String,
    generation: u64,
    session_id: String,
    max_batch_items: usize,
    candidate_count: usize,
}

pub(super) fn launch_store_backfill(
    tx: CommandSender,
    inbox_store: Arc<InboxStore>,
    event_store_slot: &EventStoreSlot,
    signer_hex: String,
    generation: u64,
) -> bool {
    if !inbox_store.may_probe_batch_backfill(generation) {
        return false;
    }
    let Some(outer_items) = outer_items_from_store(event_store_slot, &signer_hex) else {
        return false;
    };
    if !inbox_store.begin_batch_backfill(generation, outer_items.len()) {
        return false;
    }

    let max_items = outer_items.len().saturating_mul(2);
    let request = Nip44DecryptSessionBeginRequest {
        scope: NMP_NIP44_BACKFILL_SCOPE.to_string(),
        requester_pubkey: signer_hex.clone(),
        max_items,
        // Zero is an actor-owned clock sentinel. The actor stamps this from
        // `kernel.now_secs()` before the signer sees the request.
        expires_at: 0,
    };

    let candidate_count = outer_items.len();
    let store_for_begin = Arc::clone(&inbox_store);
    let tx_for_begin = tx.clone();
    let signer_for_begin = signer_hex.clone();
    let outer_items = Arc::new(outer_items);
    let continuation =
        nmp_core::actor::nip44_decrypt_session_port::Nip44DecryptSessionBeginContinuation::new(
            move |outcome| match outcome {
                Ok(Nip44DecryptSessionBeginPortResult::Granted(grant)) => {
                    begin_granted(
                        tx_for_begin,
                        store_for_begin,
                        signer_for_begin,
                        generation,
                        candidate_count,
                        grant,
                        outer_items,
                    );
                }
                Ok(Nip44DecryptSessionBeginPortResult::Unsupported { .. }) => {
                    store_for_begin.finish_batch_backfill(
                        generation,
                        candidate_count,
                        BatchBackfillFinish::Unsupported,
                    );
                }
                Err(_) => {
                    store_for_begin.finish_batch_backfill(
                        generation,
                        candidate_count,
                        BatchBackfillFinish::Failed,
                    );
                }
            },
        );

    let cmd = ActorCommand::Sign(SignCommand::Nip44DecryptSessionBegin {
        request,
        signer_pubkey: Some(signer_hex),
        continuation,
    });
    if tx.send(cmd).is_err() {
        inbox_store.finish_batch_backfill(generation, candidate_count, BatchBackfillFinish::Failed);
        return false;
    }
    true
}

fn begin_granted(
    tx: CommandSender,
    store: Arc<InboxStore>,
    signer_hex: String,
    generation: u64,
    candidate_count: usize,
    grant: Nip44DecryptSessionGrant,
    outer_items: Arc<Vec<OuterItem>>,
) {
    if grant.max_batch_items == 0 {
        let replay = BatchReplay {
            tx,
            store,
            signer_hex,
            generation,
            session_id: grant.session_id,
            max_batch_items: 1,
            candidate_count,
        };
        finish_replay(replay, BatchBackfillFinish::Failed);
        return;
    }

    let replay = BatchReplay {
        tx,
        store,
        signer_hex,
        generation,
        session_id: grant.session_id,
        max_batch_items: grant.max_batch_items,
        candidate_count,
    };
    send_outer_batch(replay, outer_items, 0);
}

fn send_outer_batch(replay: BatchReplay, outer_items: Arc<Vec<OuterItem>>, start: usize) {
    if start >= outer_items.len() {
        finish_replay(replay, BatchBackfillFinish::Succeeded);
        return;
    }
    let end = outer_items
        .len()
        .min(start.saturating_add(replay.max_batch_items));
    let chunk = outer_items[start..end].to_vec();
    let request = Nip44DecryptBatchRequest {
        session_id: replay.session_id.clone(),
        items: chunk
            .iter()
            .map(|item| Nip44DecryptBatchItem {
                id: item.id.clone(),
                peer_pubkey: item.peer_pubkey.clone(),
                ciphertext: item.ciphertext.clone(),
            })
            .collect(),
    };
    let by_id: BTreeMap<String, OuterItem> = chunk
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    let tx = replay.tx.clone();
    let replay_for_send_err = replay.clone();
    let continuation =
        nmp_core::actor::nip44_decrypt_session_port::Nip44DecryptBatchContinuation::new(
            move |outcome| match outcome {
                Ok(Nip44DecryptBatchPortResult::Batch(batch)) => {
                    let inner_items = inner_items_from_outer_batch(batch.items, &by_id);
                    send_inner_batch(replay, outer_items, end, Arc::new(inner_items), 0);
                }
                Ok(Nip44DecryptBatchPortResult::Unsupported { .. }) => {
                    finish_replay(replay, BatchBackfillFinish::Unsupported);
                }
                Err(_) => {
                    finish_replay(replay, BatchBackfillFinish::Failed);
                }
            },
        );
    let cmd = ActorCommand::Sign(SignCommand::Nip44DecryptBatch {
        request,
        signer_pubkey: Some(replay_for_send_err.signer_hex.clone()),
        continuation,
    });
    if tx.send(cmd).is_err() {
        finish_replay(replay_for_send_err, BatchBackfillFinish::Failed);
    }
}

fn send_inner_batch(
    replay: BatchReplay,
    outer_items: Arc<Vec<OuterItem>>,
    next_outer_start: usize,
    inner_items: Arc<Vec<InnerItem>>,
    start: usize,
) {
    if inner_items.is_empty() || start >= inner_items.len() {
        send_outer_batch(replay, outer_items, next_outer_start);
        return;
    }
    let end = inner_items
        .len()
        .min(start.saturating_add(replay.max_batch_items));
    let chunk = inner_items[start..end].to_vec();
    let request = Nip44DecryptBatchRequest {
        session_id: replay.session_id.clone(),
        items: chunk
            .iter()
            .map(|item| Nip44DecryptBatchItem {
                id: item.id.clone(),
                peer_pubkey: item.peer_pubkey.clone(),
                ciphertext: item.ciphertext.clone(),
            })
            .collect(),
    };
    let by_id: BTreeMap<String, InnerItem> = chunk
        .into_iter()
        .map(|item| (item.id.clone(), item))
        .collect();
    let tx = replay.tx.clone();
    let replay_for_send_err = replay.clone();
    let continuation =
        nmp_core::actor::nip44_decrypt_session_port::Nip44DecryptBatchContinuation::new(
            move |outcome| match outcome {
                Ok(Nip44DecryptBatchPortResult::Batch(batch)) => {
                    store_inner_batch(&replay, batch.items, &by_id);
                    send_inner_batch(replay, outer_items, next_outer_start, inner_items, end);
                }
                Ok(Nip44DecryptBatchPortResult::Unsupported { .. }) => {
                    finish_replay(replay, BatchBackfillFinish::Unsupported);
                }
                Err(_) => {
                    finish_replay(replay, BatchBackfillFinish::Failed);
                }
            },
        );
    let cmd = ActorCommand::Sign(SignCommand::Nip44DecryptBatch {
        request,
        signer_pubkey: Some(replay_for_send_err.signer_hex.clone()),
        continuation,
    });
    if tx.send(cmd).is_err() {
        finish_replay(replay_for_send_err, BatchBackfillFinish::Failed);
    }
}

fn finish_replay(replay: BatchReplay, finish: BatchBackfillFinish) {
    replay
        .store
        .finish_batch_backfill(replay.generation, replay.candidate_count, finish);
    let cmd = ActorCommand::Sign(SignCommand::Nip44DecryptSessionEnd {
        request: Nip44DecryptSessionEndRequest {
            session_id: replay.session_id,
        },
        signer_pubkey: Some(replay.signer_hex),
        continuation:
            nmp_core::actor::nip44_decrypt_session_port::Nip44DecryptSessionEndContinuation::new(
                |_| {},
            ),
    });
    let _ = replay.tx.send(cmd);
}

fn inner_items_from_outer_batch(
    outcomes: Vec<Nip44DecryptBatchItemPortOutcome>,
    by_id: &BTreeMap<String, OuterItem>,
) -> Vec<InnerItem> {
    let mut items = Vec::new();
    for outcome in outcomes {
        let Nip44DecryptBatchItemPortOutcome::Plaintext { id, plaintext } = outcome else {
            continue;
        };
        let Some(outer) = by_id.get(&id) else {
            continue;
        };
        let Ok((seal, ciphertext, author)) = nmp_nip59::parse_seal_for_decrypt(&plaintext) else {
            continue;
        };
        items.push(InnerItem {
            id: format!("inner:{}", outer.id),
            seal,
            peer_pubkey: author.to_hex(),
            ciphertext,
            source_relay_url: outer.source_relay_url.clone(),
        });
    }
    items
}

fn store_inner_batch(
    replay: &BatchReplay,
    outcomes: Vec<Nip44DecryptBatchItemPortOutcome>,
    by_id: &BTreeMap<String, InnerItem>,
) {
    for outcome in outcomes {
        let Nip44DecryptBatchItemPortOutcome::Plaintext { id, plaintext } = outcome else {
            continue;
        };
        let Some(inner) = by_id.get(&id) else {
            continue;
        };
        let Ok(gift) = nmp_nip59::parse_rumor(&inner.seal, &plaintext) else {
            continue;
        };
        super::chain::store_rumor(
            &replay.tx,
            &replay.store,
            replay.generation,
            &replay.signer_hex,
            &gift,
            inner.source_relay_url.as_deref(),
        );
    }
}

fn outer_items_from_store(slot: &EventStoreSlot, signer_hex: &str) -> Option<Vec<OuterItem>> {
    let recipient = PublicKey::from_hex(signer_hex).ok()?;
    let store = slot.lock().ok()?.clone()?;
    let tag = SingleLetterTag::from_char('p').ok()?;
    let mut tags = BTreeMap::new();
    tags.insert(tag, BTreeSet::from([signer_hex.to_string()]));
    let query = StoreQuery::Tags {
        authors: BTreeSet::new(),
        kinds: vec![nmp_nip59::KIND_GIFT_WRAP],
        tags,
        since: None,
        until: None,
    };
    let stored_events = store.query(&query, MAX_BATCH_BACKFILL_ENVELOPES).ok()?;
    let items: Vec<OuterItem> = stored_events
        .iter()
        .filter_map(|stored| outer_item_from_stored(store.as_ref(), stored, &recipient))
        .collect();
    if items.is_empty() {
        None
    } else {
        Some(items)
    }
}

fn outer_item_from_stored(
    store: &dyn nmp_store::EventStore,
    stored: &nmp_store::StoredEvent,
    recipient: &PublicKey,
) -> Option<OuterItem> {
    let raw = stored.raw.as_ref();
    let json = serde_json::to_string(raw).ok()?;
    let event = Event::from_json(&json).ok()?;
    let (ciphertext, peer) = nmp_nip59::parse_outer_for_decrypt(&event, recipient).ok()?;
    let source_relay_url = raw.id_bytes().and_then(|id| {
        store
            .provenance_for(&id)
            .ok()
            .and_then(|entries| entries.into_iter().next().map(|entry| entry.relay_url))
    });
    Some(OuterItem {
        id: raw.id.clone(),
        peer_pubkey: peer.to_hex(),
        ciphertext,
        source_relay_url,
    })
}
