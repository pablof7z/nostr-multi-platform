use super::*;

// ─────────────────────────────────────────────────────────────────────────
// V-80 OP-centric feed: `TimelineRow::from_snapshot` parses a
// `RootFeedSnapshot` — `{ "cards": [{ "card": TimelineEventCard,
// "attribution": [Nip10ReplyAttribution] }], "page": …, "metrics": … }`.
//
// The feed is thread-roots-only: every entry is one root (depth 0, no chain
// gap). Replies never get their own row — they attribute back to their root
// via the `attribution` array. The partial-chain machinery (`blocks`,
// `Standalone`/`Module`, `is_partial_chain_head`, `ids_from_block`) is gone.
// ─────────────────────────────────────────────────────────────────────────

/// Helper: wrap a bare `TimelineEventCard` JSON value into a `RootCard`
/// (no attribution).
fn root_card(card: serde_json::Value) -> serde_json::Value {
    serde_json::json!({ "card": card, "attribution": [] })
}

#[test]
fn snapshot_rows_follow_card_order() {
    let snapshot = serde_json::json!({
        "cards": [
            root_card(serde_json::json!(
                {"id": "root", "author_pubkey": "aaaaaaaaaaaaaaaa", "kind": 1, "created_at": 3, "content": "root note"})),
            root_card(serde_json::json!(
                {"id": "solo", "author_pubkey": "bbbbbbbbbbbbbbbb", "kind": 1, "created_at": 2, "content": "solo note"})),
        ],
        "page": {"limit": 80, "has_more": false, "total_blocks": 2, "next_cursor": null},
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    // Order is preserved exactly as the engine produced it (newest-first).
    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        vec!["root", "solo"]
    );
    // Every feed row is a thread root: depth 0, no gap, no attribution.
    assert!(rows.iter().all(|row| row.depth == 0));
    assert!(rows.iter().all(|row| !row.has_gap));
    assert!(rows.iter().all(|row| row.thread_attribution.is_empty()));
}

/// New mapping test: a root with a follow's reply attribution. The raw
/// attribution data (pubkey, display mirror, reply id, ts) is preserved; the
/// reply itself is NOT a separate row.
#[test]
fn root_card_with_attribution_keeps_raw_repliers_and_no_reply_row() {
    let replier_pubkey = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    let reply_id = "dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
    let snapshot = serde_json::json!({
        "cards": [{
            "card": {
                "id": "bobroot",
                "author_pubkey": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
                "author_display": {"name": "Bob"},
                "kind": 1,
                "created_at": 100,
                "content": "Building something interesting with Marmot"
            },
            "attribution": [{
                "author_pubkey": replier_pubkey,
                "author_display": {"name": "Alice"},
                "author_display_name": "Alice",
                "author_picture_url": null,
                "reply_event_id": reply_id,
                "reply_created_at": 150,
            }]
        }],
        "page": {"limit": 80, "has_more": false, "total_blocks": 1, "next_cursor": null},
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    // Exactly one row — the ROOT. The reply does NOT surface as its own row.
    assert_eq!(rows.len(), 1, "feed shows only the root, never the reply");
    assert_eq!(rows[0].id, "bobroot");
    assert_eq!(rows[0].author_label(), "Bob");
    // Raw attribution preserved.
    assert_eq!(rows[0].thread_attribution.len(), 1);
    let attribution = &rows[0].thread_attribution[0];
    assert_eq!(attribution.author_pubkey, replier_pubkey);
    assert_eq!(attribution.author_profile.display_name.as_deref(), Some("Alice"));
    assert_eq!(attribution.reply_event_id, reply_id);
    assert_eq!(attribution.reply_created_at, 150);
}

/// New mapping test: the projection carries N attributions raw; the TUI keeps
/// all of them on the row (the renderer picks the most-recent 1 — Q1 display
/// decision lives in `post_list.rs`, not here).
#[test]
fn root_card_preserves_all_attributions_raw() {
    let snapshot = serde_json::json!({
        "cards": [{
            "card": {
                "id": "root", "author_pubkey": "bbbb", "kind": 1, "created_at": 100, "content": "op"
            },
            "attribution": [
                {"author_pubkey": "a1", "author_display": {"name": "A1"}, "reply_event_id": "r1", "reply_created_at": 110},
                {"author_pubkey": "a2", "author_display": {"name": "A2"}, "reply_event_id": "r2", "reply_created_at": 120},
                {"author_pubkey": "a3", "author_display": {"name": "A3"}, "reply_event_id": "r3", "reply_created_at": 130},
            ]
        }],
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0]
            .thread_attribution
            .iter()
            .map(|a| a.author_pubkey.as_str())
            .collect::<Vec<_>>(),
        vec!["a1", "a2", "a3"],
        "all attributions are preserved raw; the renderer chooses how many to show"
    );
}

/// New mapping test: a reposted root. The engine keys repost slots by the
/// *target* id (the superseded note), and `from_event_for_op_feed` forces the
/// inline card's `id` to that target id. So the row resolves to the target id,
/// carries the repost attribution, and the displayed timestamp is the inner
/// note's publish time.
#[test]
fn repost_root_keyed_by_target_id() {
    let target_id = "1111111111111111111111111111111111111111111111111111111111111111";
    let snapshot = serde_json::json!({
        "cards": [{
            "card": {
                // Engine keyed this slot by `target_id`; `from_event_for_op_feed`
                // forced `card.id = target_id`.
                "id": target_id,
                "author_pubkey": "innerinnerinnerinnerinnerinnerinnerinnerinnerinnerinnerinner1234",
                "author_display": {"name": "calle"},
                "kind": 1,
                // Outer `created_at` is the kind:6 repost time.
                "created_at": 100,
                "content": "Imagine BlueSky but with Nutzaps",
                "reposted_by": {
                    "author_pubkey": "reposterreposterreposterreposterreposterreposterreposterreposte",
                    "author_display": {"name": "pablof7z"},
                    "note_created_at": 50,
                }
            },
            "attribution": []
        }],
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows.len(), 1);
    // The row resolves to the target id (not a wrapper id).
    assert_eq!(rows[0].id, target_id);
    assert_eq!(rows[0].author_label(), "calle");
    assert_eq!(rows[0].created_at, 50, "displayed time is the original note's");
    let repost = rows[0].repost.as_ref().expect("repost attribution present");
    assert_eq!(
        repost.author_pubkey,
        "reposterreposterreposterreposterreposterreposterreposterreposte"
    );
    assert_eq!(repost.author_profile.display_name.as_deref(), Some("pablof7z"));
    assert_eq!(repost.repost_created_at, 100, "repost line shows the kind:6 timestamp");
}

#[test]
fn row_uses_profile_display_and_relation_counts_when_present() {
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "aaaaaaaaaaaaaaaa",
            "author_display": {"name": "Alice"},
            "created_at": 1,
            "content": "hello",
            "relation_counts": {
                "replies": {"state": "known", "count": 2},
                "reactions": {"state": "known", "count": 3},
                "reposts": {"state": "known", "count": 1},
                "zaps": {"state": "known", "count": 4}
            }
        }))]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows[0].author_label(), "Alice");
    assert_eq!(
        rows[0].author_profile.display_name.as_deref(),
        Some("Alice")
    );
    assert_eq!(rows[0].relation_counts.replies, RowRelationCount::Known(2));
    assert_eq!(
        rows[0].relation_counts.reactions,
        RowRelationCount::Known(3)
    );
    assert_eq!(rows[0].relation_counts.reposts, RowRelationCount::Known(1));
    assert_eq!(rows[0].relation_counts.zaps, RowRelationCount::Known(4));
}

#[test]
fn mention_pubkeys_extracted_from_content_tree() {
    let mention_a = "a".repeat(64);
    let mention_b = "b".repeat(64);
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "aaaaaaaaaaaaaaaa",
            "created_at": 1,
            "content": "hello",
            "content_tree": {
                "nodes": [
                    {"kind": "text", "text": "hi "},
                    {
                        "kind": "mention",
                        "uri": {
                            "uri": "nostr:npub1...",
                            "kind": "profile",
                            "primary_id": mention_a,
                            "relays": [],
                        }
                    },
                    {"kind": "text", "text": " and "},
                    {
                        "kind": "mention",
                        "uri": {
                            "uri": "nostr:npub1...",
                            "kind": "profile",
                            "primary_id": mention_b,
                            "relays": [],
                        }
                    },
                ],
                "roots": [0, 1, 2, 3],
                "mode": "plaintext"
            }
        }))]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows[0].mention_pubkeys, vec![mention_a, mention_b]);
}

#[test]
fn mention_pubkeys_filter_non_hex_and_short_ids() {
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "aaaaaaaaaaaaaaaa",
            "created_at": 1,
            "content": "hello",
            "content_tree": {
                "nodes": [
                    {
                        "kind": "mention",
                        "uri": {
                            "uri": "nostr:npub1...",
                            "kind": "profile",
                            "primary_id": "too-short",
                            "relays": [],
                        }
                    },
                    {
                        "kind": "mention",
                        "uri": {
                            "uri": "nostr:npub1...",
                            "kind": "profile",
                            // 64 chars but with a non-hex `z` mid-string.
                            "primary_id": "zzzz1111111111111111111111111111111111111111111111111111111111zz",
                            "relays": [],
                        }
                    },
                ],
                "roots": [0, 1],
                "mode": "plaintext"
            }
        }))]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert!(
        rows[0].mention_pubkeys.is_empty(),
        "non-hex / wrong-length mention ids must be filtered, got {:?}",
        rows[0].mention_pubkeys
    );
}

#[test]
fn mention_pubkeys_dedup_and_sort() {
    let a = "a".repeat(64);
    let b = "b".repeat(64);
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "x",
            "created_at": 1,
            "content": "",
            "content_tree": {
                "nodes": [
                    // Duplicate mention should collapse to one entry.
                    {"kind": "mention", "uri": {"uri": "", "kind": "profile", "primary_id": b, "relays": []}},
                    {"kind": "mention", "uri": {"uri": "", "kind": "profile", "primary_id": a, "relays": []}},
                    {"kind": "mention", "uri": {"uri": "", "kind": "profile", "primary_id": b, "relays": []}},
                ],
                "roots": [0, 1, 2],
                "mode": "plaintext"
            }
        }))]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows[0].mention_pubkeys, vec![a, b]);
}

#[test]
fn missing_content_tree_yields_empty_mention_pubkeys() {
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "x",
            "created_at": 1,
            "content": "hello",
        }))]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert!(rows[0].mention_pubkeys.is_empty());
}

#[test]
fn media_urls_include_direct_and_quoted_event_media() {
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "x",
            "created_at": 1,
            "content": "media note",
            "content_tree": {
                "nodes": [{
                    "kind": "media",
                    "media_kind": "image",
                    "urls": ["https://example.com/direct.jpg"],
                }],
                "roots": [0],
                "mode": "plaintext"
            },
            "content_render": {
                "events": {
                    "quoted": {
                        "id": "quoted",
                        "author_pubkey": "y",
                        "content_tree": {
                            "nodes": [{
                                "kind": "image",
                                "alt": "",
                                "src": "https://example.com/quote.webp",
                            }],
                            "roots": [0],
                            "mode": "plaintext"
                        }
                    }
                }
            }
        }))]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(
        rows[0].media_urls(),
        vec![
            "https://example.com/direct.jpg".to_string(),
            "https://example.com/quote.webp".to_string(),
        ]
    );
}

#[test]
fn ordinary_note_has_no_repost_attribution() {
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "aaaaaaaaaaaaaaaa",
            "created_at": 1,
            "content": "hello"
        }))]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert!(rows[0].repost.is_none());
}

#[test]
fn relation_counts_preserve_loading_vs_known_zero() {
    let snapshot = serde_json::json!({
        "cards": [root_card(serde_json::json!({
            "id": "note",
            "author_pubkey": "aaaaaaaaaaaaaaaa",
            "created_at": 1,
            "content": "hello",
            "relation_counts": {
                "replies": {"state": "known", "count": 0},
                "reactions": {"state": "loading", "interest": {"namespace": "nmp.reactions.summary"}},
                "reposts": {"state": "known", "count": 0},
                "zaps": {"state": "known", "count": 0}
            }
        }))]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows[0].relation_counts.replies, RowRelationCount::Known(0));
    assert_eq!(rows[0].relation_counts.reactions, RowRelationCount::Loading);
    assert_eq!(rows[0].relation_counts.reposts, RowRelationCount::Known(0));
    assert_eq!(
        rows[0].relation_counts.summary(),
        "reply 0  react ...  repost 0  zap 0"
    );
}

#[test]
fn no_cards_key_yields_empty_rows() {
    let snapshot = serde_json::json!({
        "page": {"limit": 80, "has_more": false, "total_blocks": 0, "next_cursor": null}
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert!(rows.is_empty());
}

/// A card entry missing its inner `card` object is skipped (defensive: a
/// malformed snapshot must not crash or fabricate a row).
#[test]
fn card_entry_without_card_field_is_skipped() {
    let snapshot = serde_json::json!({
        "cards": [
            {"attribution": []},
            root_card(serde_json::json!({"id": "ok", "author_pubkey": "x", "created_at": 1, "content": "ok"})),
        ]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, "ok");
}
