//! Native unit tests for the pure SQL builders (`super`). Split out so
//! `query/sql.rs` stays under the 500-LOC hard cap; the builders and these
//! assertions are one logical unit — they pin the exact SQL shape + param order
//! that the (wasm-only) bind/step path depends on, so they live beside it.

use super::*;

fn ints(params: &[OwnedVal]) -> Vec<i64> {
    params
        .iter()
        .filter_map(|v| match v {
            OwnedVal::Int(n) => Some(*n),
            _ => None,
        })
        .collect()
}

#[test]
fn author_kind_empty_kinds_matches_nothing() {
    assert!(build_author_kind(&[1u8; 32], &[], None, None, 10).is_none());
}

#[test]
fn author_kind_is_index_ordered_with_bounds() {
    let (sql, params) = build_author_kind(&[7u8; 32], &[1, 6], Some(100), Some(200), 50).unwrap();
    assert!(sql.contains("FROM events WHERE 1"));
    assert!(sql.contains("pubkey = ?"));
    assert!(sql.contains("kind IN (?,?)"));
    assert!(sql.contains("created_at >= ?"));
    assert!(sql.contains("created_at <= ?"));
    assert!(sql.ends_with("ORDER BY created_at DESC, id ASC LIMIT ?"));
    // kinds 1,6, since 100, until 200, limit 50 — in append order.
    assert_eq!(ints(&params), vec![1, 6, 100, 200, 50]);
}

#[test]
fn authors_kind_empty_sets_match_nothing() {
    let one: BTreeSet<PubKey> = [[1u8; 32]].into_iter().collect();
    assert!(build_authors_kind(&BTreeSet::new(), &[1], None, None, 10).is_none());
    assert!(build_authors_kind(&one, &[], None, None, 10).is_none());
}

#[test]
fn authors_kind_global_order_across_authors() {
    let authors: BTreeSet<PubKey> = [[1u8; 32], [2u8; 32]].into_iter().collect();
    let (sql, _) = build_authors_kind(&authors, &[1], None, None, 10).unwrap();
    assert!(sql.contains("pubkey IN (?,?)"));
    // ONE global ORDER BY — not a per-author UNION — gives the merged order.
    assert_eq!(sql.matches("ORDER BY").count(), 1);
    assert!(sql.ends_with("ORDER BY created_at DESC, id ASC LIMIT ?"));
}

#[test]
fn kind_time_empty_kinds_is_any_kind() {
    let (sql, params) = build_kind_time(&[], None, None, 10);
    assert!(
        !sql.contains("kind IN"),
        "empty kinds must not constrain kind"
    );
    assert!(sql.contains("FROM events WHERE 1 ORDER BY"));
    assert_eq!(ints(&params), vec![10]); // just the limit
}

#[test]
fn kind_time_with_kinds_constrains() {
    let (sql, _) = build_kind_time(&[1, 7], None, None, 10);
    assert!(sql.contains("kind IN (?,?)"));
}

#[test]
fn kind_dtag_seeks_primary_index() {
    let (sql, params) = build_kind_dtag(30023, b"slug", None, None, 5);
    assert!(sql.contains("kind = ?"));
    assert!(sql.contains("d_tag = ?"));
    assert!(sql.ends_with("ORDER BY created_at DESC, id ASC LIMIT ?"));
    // d_tag is a blob param.
    assert!(matches!(params[1], OwnedVal::Blob(ref b) if b == b"slug"));
}

#[test]
fn tags_empty_or_empty_values_match_nothing() {
    assert!(build_tags(&BTreeSet::new(), &[], &BTreeMap::new(), None, None, 10).is_none());
    let mut tags = BTreeMap::new();
    tags.insert('e', BTreeSet::<String>::new());
    assert!(build_tags(&BTreeSet::new(), &[], &tags, None, None, 10).is_none());
}

#[test]
fn tags_and_across_letters_or_within_values_index_served() {
    // #e in {X,Y} AND #p in {P1} — two letters, OR within e's values.
    let mut tags: BTreeMap<char, BTreeSet<String>> = BTreeMap::new();
    tags.insert('e', ["X".to_owned(), "Y".to_owned()].into_iter().collect());
    tags.insert('p', ["P1".to_owned()].into_iter().collect());
    let (sql, params) = build_tags(&BTreeSet::new(), &[], &tags, None, None, 25).unwrap();

    // Candidates come from event_tags (the tci/atci/ktci source), not a scan
    // of `events`.
    assert!(sql.contains("FROM event_tags WHERE"));
    // OR within e's value set, OR across the two letters.
    assert!(sql.contains("tag_value IN (?,?)")); // e: X,Y
    assert!(sql.contains(" OR ")); // across letters
                                   // AND across letters via the distinct-letter count == 2.
    assert!(sql.contains("HAVING COUNT(DISTINCT tag_name) = ?"));
    // Newest-first over the joined candidate set.
    assert!(sql.contains("ORDER BY e.created_at DESC, e.id ASC LIMIT ?"));

    // Param order: 'e', X, Y, 'p', P1, HAVING-count 2, LIMIT 25.
    let texts: Vec<&str> = params
        .iter()
        .filter_map(|v| match v {
            OwnedVal::Text(s) => Some(s.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, vec!["e", "X", "Y", "p", "P1"]);
    assert_eq!(ints(&params), vec![2, 25]); // distinct-letter count, then limit
}

#[test]
fn tags_pushes_author_kind_into_subquery() {
    let authors: BTreeSet<PubKey> = [[3u8; 32]].into_iter().collect();
    let mut tags: BTreeMap<char, BTreeSet<String>> = BTreeMap::new();
    tags.insert('t', ["nostr".to_owned()].into_iter().collect());
    let (sql, _) = build_tags(&authors, &[1], &tags, Some(10), Some(20), 5).unwrap();
    // Author/kind/time constrain the indexed event_tags subquery (atci/ktci),
    // BEFORE the GROUP BY — not a post-join filter on `events`.
    let sub = &sql[..sql.find("GROUP BY").unwrap()];
    assert!(sub.contains("pubkey IN (?)"));
    assert!(sub.contains("kind IN (?)"));
    assert!(sub.contains("created_at >= ?"));
    assert!(sub.contains("created_at <= ?"));
}

#[test]
fn expiring_before_is_ascending_strict() {
    let (sql, params) = build_expiring_before(1_700, 10);
    assert!(sql.contains("expires_at IS NOT NULL"));
    assert!(sql.contains("expires_at < ?"));
    assert!(sql.contains("ORDER BY expires_at ASC"));
    assert_eq!(ints(&params), vec![1_700, 10]);
}

#[test]
fn build_query_dispatches_each_variant() {
    assert!(build_query(
        &EngineQuery::KindTime {
            kinds: vec![],
            since: None,
            until: None
        },
        10
    )
    .is_some());
    // The "matches nothing" shapes propagate None through dispatch.
    assert!(build_query(
        &EngineQuery::AuthorKind {
            author: [0u8; 32],
            kinds: vec![],
            since: None,
            until: None
        },
        10
    )
    .is_none());
}
