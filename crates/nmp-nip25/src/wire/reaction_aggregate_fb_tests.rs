use super::*;

fn sample() -> ReactionAggregateSnapshot {
    ReactionAggregateSnapshot {
        targets: vec![
            ReactionTargetAggregate {
                target_event_id: "a".repeat(64),
                total: 3,
                by_emoji: vec![
                    ReactionEmojiCount { token: "+".into(), count: 2 },
                    ReactionEmojiCount { token: "🔥".into(), count: 1 },
                ],
                reactors: vec!["1".repeat(64), "2".repeat(64), "3".repeat(64)],
            },
            ReactionTargetAggregate {
                target_event_id: "b".repeat(64),
                total: 0,
                by_emoji: Vec::new(),
                reactors: Vec::new(),
            },
        ],
    }
}

#[test]
fn round_trips_counts_emoji_and_reactors() {
    let snapshot = sample();
    let bytes = encode_reaction_aggregate_snapshot(&snapshot);
    let decoded = decode_reaction_aggregate_snapshot(&bytes).expect("decode");
    assert_eq!(decoded, snapshot);
}

#[test]
fn buffer_carries_n25a_identifier() {
    let bytes = encode_reaction_aggregate_snapshot(&sample());
    assert_eq!(&bytes[4..8], REACTION_AGGREGATE_FILE_IDENTIFIER);
    assert_eq!(REACTION_AGGREGATE_SCHEMA_ID, "nmp.nip25.reactions");
}

#[test]
fn empty_snapshot_round_trips() {
    let snapshot = ReactionAggregateSnapshot::empty();
    let bytes = encode_reaction_aggregate_snapshot(&snapshot);
    assert_eq!(decode_reaction_aggregate_snapshot(&bytes).unwrap(), snapshot);
}

#[test]
fn rejects_foreign_identifier() {
    assert!(decode_reaction_aggregate_snapshot(b"not-a-buffer").is_err());
}
