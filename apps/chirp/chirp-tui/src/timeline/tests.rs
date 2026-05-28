use super::*;

#[test]
fn snapshot_rows_follow_block_order() {
    let snapshot = serde_json::json!({
        "blocks": [
            {"Module": {"events": ["root", "reply"], "has_gap": true, "root": null}},
            {"Standalone": {"id": "solo"}}
        ],
        "cards": [
            {"id": "solo", "author_pubkey": "bbbbbbbbbbbbbbbb", "kind": 1, "created_at": 3, "content": "solo note"},
            {"id": "reply", "author_pubkey": "cccccccccccccccc", "kind": 1, "created_at": 2, "content": "reply note"},
            {"id": "root", "author_pubkey": "aaaaaaaaaaaaaaaa", "kind": 1, "created_at": 1, "content": "root note"}
        ]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(
        rows.iter().map(|row| row.id.as_str()).collect::<Vec<_>>(),
        vec!["root", "reply", "solo"]
    );
    assert_eq!(rows[0].depth, 0);
    assert_eq!(rows[1].depth, 1);
    assert!(rows[1].has_gap);
    // `root: null` ⇒ head IS the true thread root, so the partial-chain
    // flag must stay false even though `has_gap` is true.
    assert!(!rows[0].is_partial_chain_head);
    assert!(!rows[1].is_partial_chain_head);
}

/// Regression: when a `TimelineBlock::Module` carries `root: Some(_)`,
/// the chain's head event is a reply to a missing ancestor (partial
/// chain) — NOT the true thread root. Only the head must be flagged;
/// subsequent in-module events are ordinary replies. `depth` must
/// stay `0` for the head so the detail-pane navigation anchor still
/// resolves correctly.
#[test]
fn partial_chain_module_head_gets_flag() {
    let snapshot = serde_json::json!({
        "blocks": [
            {"Module": {
                "events": ["reply1", "reply2"],
                "has_gap": true,
                "root": {"Event": {"id": "missing_root", "relay": null, "kind": null}}
            }}
        ],
        "cards": [
            {"id": "reply1", "author_pubkey": "aaa", "created_at": 1, "content": "reply 1"},
            {"id": "reply2", "author_pubkey": "bbb", "created_at": 2, "content": "reply 2"}
        ]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows.len(), 2);
    assert!(
        rows[0].is_partial_chain_head,
        "first event of partial chain should be flagged"
    );
    assert!(
        !rows[1].is_partial_chain_head,
        "subsequent events should not be flagged"
    );
    assert_eq!(
        rows[0].depth, 0,
        "depth must stay 0 for navigation anchoring"
    );
    assert_eq!(rows[1].depth, 1);
}

#[test]
fn event_root_matching_module_head_is_not_partial_chain() {
    let snapshot = serde_json::json!({
        "blocks": [
            {"Module": {
                "events": ["root", "reply"],
                "has_gap": false,
                "root": {"Event": {"id": "root", "relay": null, "kind": null}}
            }}
        ],
        "cards": [
            {"id": "root", "author_pubkey": "aaa", "created_at": 1, "content": "root"},
            {"id": "reply", "author_pubkey": "bbb", "created_at": 2, "content": "reply"}
        ]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows.len(), 2);
    assert!(!rows[0].is_partial_chain_head);
    assert!(!rows[1].is_partial_chain_head);
}

#[test]
fn address_and_external_roots_are_not_partial_event_chains() {
    let snapshot = serde_json::json!({
        "blocks": [
            {"Module": {
                "events": ["article_reply"],
                "has_gap": true,
                "root": {"Address": {"coord": "30023:pubkey:slug", "relay": null, "kind": 30023}}
            }},
            {"Module": {
                "events": ["uri_reply"],
                "has_gap": true,
                "root": {"External": {"uri": "https://example.com/post"}}
            }}
        ],
        "cards": [
            {"id": "article_reply", "author_pubkey": "aaa", "created_at": 1, "content": "article reply"},
            {"id": "uri_reply", "author_pubkey": "bbb", "created_at": 2, "content": "uri reply"}
        ]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows.len(), 2);
    assert!(!rows[0].is_partial_chain_head);
    assert!(!rows[1].is_partial_chain_head);
}

/// A rootless standalone block (`root` field absent) is a genuine thread
/// root — it is never a partial-chain head.
#[test]
fn rootless_standalone_block_is_never_partial_chain_head() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "solo"}}],
        "cards": [
            {"id": "solo", "author_pubkey": "x", "created_at": 1, "content": "solo"}
        ]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows.len(), 1);
    assert!(!rows[0].is_partial_chain_head);
}

/// Rung 2 behaviour delta: a standalone whose `root` Event pointer names a
/// DIFFERENT id is a reply that could not be stitched into a chain. The
/// grouper now preserves that root, so the renderer flags the row as a
/// partial-chain head (the ↳ "reply in thread" indicator lights up).
#[test]
fn standalone_with_mismatched_event_root_is_partial_chain_head() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {
            "id": "reply",
            "root": {"Event": {"id": "missing_root", "relay": null, "kind": null}}
        }}],
        "cards": [
            {"id": "reply", "author_pubkey": "x", "created_at": 1, "content": "a reply"}
        ]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows.len(), 1);
    assert!(
        rows[0].is_partial_chain_head,
        "a standalone reply with a mismatched event root is a partial-chain head"
    );
    assert_eq!(rows[0].depth, 0, "depth stays 0 for navigation anchoring");
}

/// A standalone whose `root` Event pointer matches its own id (degenerate
/// self-root) is NOT a partial chain. Likewise Address / External roots
/// terminate the chain and never imply a missing event head.
#[test]
fn standalone_with_non_event_or_self_root_is_not_partial_chain() {
    let snapshot = serde_json::json!({
        "blocks": [
            {"Standalone": {
                "id": "self_rooted",
                "root": {"Event": {"id": "self_rooted", "relay": null, "kind": null}}
            }},
            {"Standalone": {
                "id": "article_reply",
                "root": {"Address": {"coord": "30023:pubkey:slug", "relay": null, "kind": 30023}}
            }},
            {"Standalone": {
                "id": "uri_reply",
                "root": {"External": {"uri": "https://example.com/post"}}
            }}
        ],
        "cards": [
            {"id": "self_rooted", "author_pubkey": "a", "created_at": 3, "content": "self"},
            {"id": "article_reply", "author_pubkey": "b", "created_at": 2, "content": "article"},
            {"id": "uri_reply", "author_pubkey": "c", "created_at": 1, "content": "uri"}
        ]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows.len(), 3);
    assert!(rows.iter().all(|row| !row.is_partial_chain_head));
}

/// Per `TimelineBlock::Module.root`'s `#[serde(skip_serializing_if =
/// Option::is_none)]`, a `None` root may be entirely ABSENT from the
/// serialized JSON (not just `null`). Both shapes must yield
/// `is_partial_chain_head == false`.
#[test]
fn module_with_absent_root_field_is_not_partial_chain() {
    let snapshot = serde_json::json!({
        "blocks": [
            // Note: no `root` key at all.
            {"Module": {"events": ["a", "b"], "has_gap": false}}
        ],
        "cards": [
            {"id": "a", "author_pubkey": "x", "created_at": 1, "content": "root"},
            {"id": "b", "author_pubkey": "y", "created_at": 2, "content": "reply"}
        ]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows.len(), 2);
    assert!(!rows[0].is_partial_chain_head);
    assert!(!rows[1].is_partial_chain_head);
}

#[test]
fn row_uses_profile_display_and_relation_counts_when_present() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
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
        }]
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
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
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
        }]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows[0].mention_pubkeys, vec![mention_a, mention_b]);
}

#[test]
fn mention_pubkeys_filter_non_hex_and_short_ids() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
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
        }]
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
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
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
        }]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert_eq!(rows[0].mention_pubkeys, vec![a, b]);
}

#[test]
fn missing_content_tree_yields_empty_mention_pubkeys() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
            "id": "note",
            "author_pubkey": "x",
            "created_at": 1,
            "content": "hello",
        }]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert!(rows[0].mention_pubkeys.is_empty());
}

#[test]
fn media_urls_include_direct_and_quoted_event_media() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
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
        }]
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
fn repost_card_uses_inner_timestamp_and_attaches_repost_attribution() {
    // The card represents the original note (kind:1 author, kind:1 content)
    // but its outer `created_at` is the kind:6 repost time. The row's
    // displayed `created_at` should be the inner note's publish time;
    // `repost` carries the reposter + repost timestamp for the "↻ reposted
    // by" line.
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "repost"}}],
        "cards": [{
            "id": "repost",
            "author_pubkey": "innerinnerinnerinnerinnerinnerinnerinnerinnerinnerinnerinner1234",
            "author_display": {"name": "calle"},
            "created_at": 100,
            "content": "Imagine BlueSky but with Nutzaps",
            "reposted_by": {
                "author_pubkey": "reposterreposterreposterreposterreposterreposterreposterreposte",
                "author_display": {"name": "pablof7z"},
                "note_created_at": 50,
            }
        }]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].author_label(), "calle");
    assert_eq!(
        rows[0].created_at, 50,
        "displayed time is the original note's"
    );
    let repost = rows[0].repost.as_ref().expect("repost attribution present");
    assert_eq!(
        repost.author_pubkey,
        "reposterreposterreposterreposterreposterreposterreposterreposte"
    );
    assert_eq!(
        repost.author_profile.display_name.as_deref(),
        Some("pablof7z")
    );
    assert_eq!(
        repost.repost_created_at, 100,
        "repost line shows the kind:6 timestamp"
    );
}

#[test]
fn ordinary_note_has_no_repost_attribution() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
            "id": "note",
            "author_pubkey": "aaaaaaaaaaaaaaaa",
            "created_at": 1,
            "content": "hello"
        }]
    });
    let rows = TimelineRow::from_snapshot(&snapshot);
    assert!(rows[0].repost.is_none());
}

#[test]
fn relation_counts_preserve_loading_vs_known_zero() {
    let snapshot = serde_json::json!({
        "blocks": [{"Standalone": {"id": "note"}}],
        "cards": [{
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
        }]
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

/// Regression: when `blocks` is present but its IDs temporarily don't
/// match any card (blocks/cards desync mid-session), replies must NOT be
/// promoted to depth 0.  The correct result is an empty row list.
#[test]
fn blocks_present_but_ids_missing_from_cards_yields_empty() {
    let snapshot = serde_json::json!({
        "blocks": [
            {"Module": {"events": ["root", "reply"], "has_gap": false, "root": null}}
        ],
        "cards": [
            {"id": "other", "author_pubkey": "aaaaaaaaaaaaaaaa", "created_at": 1, "content": "reply text"}
        ]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert!(
        rows.is_empty(),
        "blocks present but no matching cards → empty rows; got {:?}",
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn no_blocks_key_yields_empty_rows() {
    let snapshot = serde_json::json!({
        "cards": [
            {"id": "a", "author_pubkey": "aaaaaaaaaaaaaaaa", "created_at": 2, "content": "root"},
            {"id": "b", "author_pubkey": "bbbbbbbbbbbbbbbb", "created_at": 1, "content": "reply"}
        ]
    });

    let rows = TimelineRow::from_snapshot(&snapshot);

    assert!(rows.is_empty());
}
