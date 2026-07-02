//! Kind:1 top-level notes (NIP-01) and NIP-10 threaded replies.

use crate::support::*;

// ── Kind 1 — NIP-01 top-level note ──────────────────────────────────────────

/// NIP-01: a plain top-level note has NO required tags. The conformance bar is
/// the *negative*: it must not sprout `e`/`p` tags it has no reason to carry.
#[test]
fn kind1_note_carries_no_tags() {
    let mut h = signed_harness();
    let event = h.emit_note("a plain note, no thread context", None);

    assert_eq!(event["kind"], 1, "note must be kind:1");
    assert!(
        tags_of(&event).is_empty(),
        "NIP-01 top-level note must carry no tags, got: {:?}",
        tags_of(&event)
    );
}

// ── Kind 1 — NIP-10 reply ───────────────────────────────────────────────────

/// NIP-10: a reply to a thread root must carry both an `e` "root" marker and an
/// `e` "reply" marker (marked form), plus a `p` tag re-notifying the parent's
/// author. This is the structure `nmp_nip01::Note::reply_to` emits.
#[test]
fn kind1_reply_carries_nip10_e_markers_and_parent_p_tag() {
    let mut h = signed_harness();
    let root_id = hex64('1');
    let root_author = hex64('a');
    // Seed the parent (which IS the thread root — no NIP-10 refs of its own).
    h.seed_note(&root_id, &root_author, vec![]);

    let event = h.emit_note("a reply to the root", Some(&root_id));
    assert_eq!(event["kind"], 1, "reply must be kind:1");

    // NIP-10 requires exactly one root + one reply `e` marker; here both point
    // at the parent because the parent is itself the root.
    let e_tags = tags_with_key(&event, "e");
    let root_marker = e_tags
        .iter()
        .find(|t| t.get(3).map(String::as_str) == Some("root"))
        .expect("NIP-10 reply must carry an `e` tag with a `root` marker");
    let reply_marker = e_tags
        .iter()
        .find(|t| t.get(3).map(String::as_str) == Some("reply"))
        .expect("NIP-10 reply must carry an `e` tag with a `reply` marker");
    assert_eq!(
        root_marker.get(1).map(String::as_str),
        Some(root_id.as_str()),
        "the `root` marker must reference the thread root event id"
    );
    assert_eq!(
        reply_marker.get(1).map(String::as_str),
        Some(root_id.as_str()),
        "the `reply` marker must reference the direct parent event id"
    );

    // NIP-10 §p-tags: the parent's author must be re-notified. This is the
    // exact class of tag the NIP-25 review found missing on reactions.
    let p_values = values_for_key(&event, "p");
    assert!(
        p_values.contains(&root_author),
        "NIP-10 reply must carry a `p` tag for the parent author ({root_author}), got: {p_values:?}"
    );

    // No tag keys beyond `e` and `p` on a reply.
    assert_only_keys(&event, &["e", "p"], "NIP-10 reply");
}
