//! Cross-cutting: no command leaks an `e`/`p` tag where the NIP forbids it.

use crate::support::*;

/// A non-reply note and a kind:0 metadata event are the two "tagless" emit
/// paths. A regression that started attaching thread/notification tags to
/// either would be a conformance break — pin both in one place.
#[test]
fn tagless_kinds_never_emit_e_or_p_tags() {
    let mut h = signed_harness();

    let note = h.emit_note("tagless note", None);
    assert!(
        tags_with_key(&note, "e").is_empty() && tags_with_key(&note, "p").is_empty(),
        "a top-level kind:1 note must never emit `e` or `p` tags"
    );

    let metadata = h.emit_unsigned(0, vec![], r#"{"display_name":"Nobody"}"#);
    assert!(
        tags_with_key(&metadata, "e").is_empty() && tags_with_key(&metadata, "p").is_empty(),
        "a kind:0 metadata event must never emit `e` or `p` tags"
    );
}
