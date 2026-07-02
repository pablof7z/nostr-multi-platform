//! Kind:7 reactions (NIP-25) — the `e`+`p` tag pair, including the regression
//! this suite was created to pin (a reaction missing the reacted-to author).

use crate::support::*;

/// NIP-25: a kind:7 reaction must carry an `e` tag (the reacted-to event) AND a
/// `p` tag (that event's author) so the author's relays route the reaction to
/// their notification inbox. The missing `p` tag here was the bug that
/// motivated this whole suite.
#[test]
fn kind7_reaction_carries_e_and_p_tags() {
    let mut h = signed_harness();
    let target_id = hex64('e');
    let target_author = hex64('c');
    // Seed the reacted-to event so its author is resolvable from the read-cache.
    h.seed_note(&target_id, &target_author, vec![]);

    let event = h.emit_reaction(&target_id, "+");
    assert_eq!(event["kind"], 7, "reaction must be kind:7");

    // Exactly one `e` tag → the reacted-to event.
    let e_values = values_for_key(&event, "e");
    assert_eq!(
        e_values,
        vec![target_id.clone()],
        "NIP-25 reaction must carry exactly one `e` tag for the reacted-to event"
    );

    // Exactly one `p` tag → the reacted-to event's author. The regression pin.
    let p_values = values_for_key(&event, "p");
    assert_eq!(
        p_values,
        vec![target_author.clone()],
        "NIP-25 reaction must carry a `p` tag for the reacted-to author — \
         the missing-`p` bug this suite exists to catch"
    );

    assert_only_keys(&event, &["e", "p"], "NIP-25 reaction");
}
