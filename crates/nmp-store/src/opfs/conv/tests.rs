use super::*;

fn raw() -> RawEvent {
    RawEvent {
        id: "a".repeat(64),
        pubkey: "b".repeat(64),
        created_at: 1_700_000_000,
        kind: 30023,
        tags: vec![
            vec!["d".into(), "slug".into()],
            vec!["e".into(), "c".repeat(64)],
        ],
        content: "hello — ünïcode".into(),
        sig: "f".repeat(128),
    }
}

#[test]
fn raw_engine_round_trips() {
    let r = raw();
    let back = engine_event_into_raw(raw_into_engine(r.clone()));
    assert_eq!(back.id, r.id);
    assert_eq!(back.pubkey, r.pubkey);
    assert_eq!(back.created_at, r.created_at);
    assert_eq!(back.kind, r.kind);
    assert_eq!(back.tags, r.tags);
    assert_eq!(back.content, r.content);
    assert_eq!(back.sig, r.sig);
}

#[test]
fn stored_wraps_arc_and_keeps_arrival() {
    let se = engine::StoredEngineEvent {
        event: raw_into_engine(raw()),
        received_at_ms: 42,
    };
    let s = stored_into(se);
    assert_eq!(s.received_at_ms, 42);
    assert_eq!(s.raw.kind, 30023);
}

#[test]
fn query_tags_collapse_single_letter_to_char() {
    let mut tags: BTreeMap<SingleLetterTag, BTreeSet<String>> = BTreeMap::new();
    tags.insert(
        SingleLetterTag::lowercase(nostr::Alphabet::E),
        BTreeSet::from(["x".to_string()]),
    );
    let q = StoreQuery::Tags {
        authors: BTreeSet::new(),
        kinds: vec![1],
        tags,
        since: Some(1),
        until: None,
    };
    match query_into_engine(&q) {
        engine::EngineQuery::Tags {
            tags, kinds, since, ..
        } => {
            assert_eq!(kinds, vec![1]);
            assert_eq!(since, Some(1));
            assert!(tags.contains_key(&'e'));
        }
        other => panic!("expected Tags, got {other:?}"),
    }
}

#[test]
fn insert_outcome_maps_each_variant() {
    let id = [1u8; 32];
    assert!(matches!(
        insert_outcome(engine::InsertOutcome::Inserted {
            id,
            sources_after: 2
        }),
        InsertOutcome::Inserted {
            sources_after: 2,
            ..
        }
    ));
    assert!(matches!(
        insert_outcome(engine::InsertOutcome::Tombstoned {
            id,
            kind5_event_id: None,
            origin: engine::TombstoneOrigin::NIP40Expiry,
        }),
        InsertOutcome::Tombstoned {
            origin: TombstoneOrigin::NIP40Expiry,
            ..
        }
    ));
    assert!(matches!(
        insert_outcome(engine::InsertOutcome::Rejected {
            id,
            reason: engine::RejectReason::ExpiredOnArrival,
        }),
        InsertOutcome::Rejected {
            reason: RejectReason::ExpiredOnArrival,
            ..
        }
    ));
}

#[test]
fn error_mapping_buckets() {
    assert!(matches!(
        store_err(engine::SqliteWasmError::Open("x".into())),
        StoreError::Io(_)
    ));
    assert!(matches!(
        store_err(engine::SqliteWasmError::Column("x".into())),
        StoreError::Encoding(_)
    ));
    assert!(matches!(
        store_err(engine::SqliteWasmError::Migration(
            "ns on-disk schema 3 is newer than target 1".into()
        )),
        StoreError::SchemaTooNew { .. }
    ));
    assert!(matches!(
        store_err(engine::SqliteWasmError::Migration("step 0→1: boom".into())),
        StoreError::MigrationFailed { .. }
    ));
}

#[test]
fn replaceable_key_param_encodes_dtag_bytes() {
    let k = ReplaceableKey::Parameterized {
        kind: 30023,
        pubkey: [7u8; 32],
        d_tag: "slug".to_string(),
    };
    match replaceable_key(&k) {
        engine::ReplaceableKey::Parameterized { kind, d_tag, .. } => {
            assert_eq!(kind, 30023);
            assert_eq!(d_tag, b"slug".to_vec());
        }
        _ => panic!("expected Parameterized"),
    }
}

#[test]
fn scan_log_gap_and_page_convert() {
    let gap = engine::ScanLogResult::Gap(engine::PullGap {
        requested_after_seq: 5,
        first_available_seq: 9,
    });
    match scan_log_result(gap) {
        ScanLogResult::Gap(g) => {
            assert_eq!(g.requested_after_seq, 5);
            assert_eq!(g.first_available_seq, 9);
        }
        _ => panic!("expected Gap"),
    }
}
