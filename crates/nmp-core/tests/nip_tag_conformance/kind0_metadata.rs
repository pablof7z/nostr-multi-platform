//! Kind:0 profile metadata (NIP-01) — the tagless negative case.
//!
//! NIP-47 note: `kind23194_nwc_request_carries_wallet_p_tag` (V-38) moved to
//! `crates/nmp-nip47/tests/` — the wallet runtime and the conformance
//! harness's wallet driver both left `nmp-core` in V-38.

use crate::support::*;

/// NIP-01: a kind:0 metadata event has NO required tags — the profile fields
/// live in the JSON `content`, not in tags. Conformance is the negative:
/// metadata must not carry tags. Driven through `publish_unsigned_event`, the
/// production path for a profile/display-name update.
#[test]
fn kind0_metadata_carries_no_tags() {
    let mut h = signed_harness();
    let event = h.emit_unsigned(
        0,
        vec![],
        r#"{"name":"marcus","display_name":"Marcus Webb"}"#,
    );

    assert_eq!(event["kind"], 0, "metadata must be kind:0");
    assert!(
        tags_of(&event).is_empty(),
        "NIP-01 kind:0 metadata must carry no tags, got: {:?}",
        tags_of(&event)
    );
    // The profile JSON rides in `content`, not in tags.
    assert!(
        event["content"]
            .as_str()
            .is_some_and(|c| c.contains("Marcus Webb")),
        "kind:0 metadata content must carry the profile JSON"
    );
}
