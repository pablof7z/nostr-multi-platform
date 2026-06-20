use super::*;

const COMMENT_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const AUTHOR: &str = "2222222222222222222222222222222222222222222222222222222222222222";
const ROOT_EVENT: &str = "3333333333333333333333333333333333333333333333333333333333333333";
const PARENT_COMMENT: &str = "4444444444444444444444444444444444444444444444444444444444444444";

fn comment_event(tags: Vec<Vec<String>>, content: &str) -> KernelEvent {
    KernelEvent {
        id: COMMENT_ID.to_string(),
        author: AUTHOR.to_string(),
        kind: KIND_NIP22_COMMENT,
        created_at: 42,
        tags,
        content: content.to_string(),
        relay_provenance: Vec::new(),
    }
}

fn tag(name: &str, value: &str) -> Vec<String> {
    vec![name.to_string(), value.to_string()]
}

#[test]
fn rejects_non_comment_kind() {
    let event = KernelEvent {
        kind: 1,
        ..comment_event(vec![tag("E", ROOT_EVENT), tag("K", "11")], "hi")
    };
    assert!(try_from_kernel_event(&event).is_none());
}

#[test]
fn rejects_comment_without_root_scope() {
    let event = comment_event(vec![tag("k", "1111")], "orphan");
    assert!(try_from_kernel_event(&event).is_none());
}

#[test]
fn top_level_comment_mirrors_root_as_parent() {
    // A top-level comment carries only the uppercase root scope; its parent is
    // the root itself.
    let event = comment_event(vec![tag("E", ROOT_EVENT), tag("K", "11")], "first!");
    let record = try_from_kernel_event(&event).expect("decoded");

    assert_eq!(record.event_id, COMMENT_ID);
    assert_eq!(record.author_pubkey, AUTHOR);
    assert_eq!(record.body, "first!");
    assert_eq!(record.root_tag_name, "E");
    assert_eq!(record.root_tag_value, ROOT_EVENT);
    assert_eq!(record.root_kind, "11");
    // Parent scope mirrors the (lowercased) root.
    assert_eq!(record.parent_tag_name, "e");
    assert_eq!(record.parent_tag_value, ROOT_EVENT);
    assert_eq!(record.parent_kind, "11");
    assert_eq!(record.created_at, 42);
    assert!(record.is_top_level());
}

#[test]
fn reply_parses_distinct_parent_scope() {
    // A reply keeps the uppercase root but points its lowercase parent scope at
    // the parent kind:1111 comment.
    let event = comment_event(
        vec![
            tag("E", ROOT_EVENT),
            tag("K", "11"),
            tag("e", PARENT_COMMENT),
            tag("k", "1111"),
        ],
        "good point",
    );
    let record = try_from_kernel_event(&event).expect("decoded");

    assert_eq!(record.root_tag_value, ROOT_EVENT);
    assert_eq!(record.root_kind, "11");
    assert_eq!(record.parent_tag_name, "e");
    assert_eq!(record.parent_tag_value, PARENT_COMMENT);
    assert_eq!(record.parent_kind, "1111");
    assert!(!record.is_top_level());
}

#[test]
fn addressable_root_uses_a_scope() {
    let address = "30023:pubkey:essay";
    let event = comment_event(vec![tag("A", address), tag("K", "30023")], "nice essay");
    let record = try_from_kernel_event(&event).expect("decoded");

    assert_eq!(record.root_tag_name, "A");
    assert_eq!(record.root_tag_value, address);
    assert_eq!(record.parent_tag_name, "a");
    assert_eq!(record.parent_tag_value, address);
}

#[test]
fn external_root_uses_i_scope_and_preserves_identifier() {
    let identifier = "podcast:item:guid:abc-123";
    let event = comment_event(vec![tag("I", identifier), tag("K", "")], "loved it");
    let record = try_from_kernel_event(&event).expect("decoded");

    assert_eq!(record.root_tag_name, "I");
    assert_eq!(record.root_tag_value, identifier);
    assert_eq!(record.parent_tag_name, "i");
    assert_eq!(record.parent_tag_value, identifier);
    // Empty K tag is treated as absent.
    assert_eq!(record.root_kind, "");
}

#[test]
fn first_root_scope_wins_when_multiple_present() {
    let event = comment_event(
        vec![tag("E", ROOT_EVENT), tag("A", "30023:pk:d"), tag("K", "11")],
        "redundant scopes",
    );
    let record = try_from_kernel_event(&event).expect("decoded");
    assert_eq!(record.root_tag_name, "E");
    assert_eq!(record.root_tag_value, ROOT_EVENT);
}
