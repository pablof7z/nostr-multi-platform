//! Kind:10002 relay lists (NIP-65) — the `r`-tag structural contract, shared
//! by a direct publish and the `create_account` cold-start scenario in
//! `account_coldstart`.

use serde_json::Value;

use crate::support::*;

/// NIP-65 `r`-tag structural check, reused by both the direct-publish test
/// below and the cold-start kind:10002 test in `account_coldstart`. Asserts:
/// one `r` tag per relay, every declared URL present, and any marker column
/// limited to `read`/`write` — and no tag keys besides `r`.
pub(crate) fn assert_nip65_relay_list(event: &Value, expected_urls: &[&str]) {
    assert_eq!(event["kind"], 10002, "relay list must be kind:10002");
    let r_tags = tags_with_key(event, "r");
    assert_eq!(
        r_tags.len(),
        expected_urls.len(),
        "NIP-65 kind:10002 must carry exactly one `r` tag per relay"
    );
    let r_urls = values_for_key(event, "r");
    for url in expected_urls {
        assert!(
            r_urls.contains(&url.to_string()),
            "NIP-65 kind:10002 must carry an `r` tag for relay {url}"
        );
    }
    for tag in &r_tags {
        if let Some(marker) = tag.get(2) {
            assert!(
                marker == "read" || marker == "write",
                "NIP-65 `r` marker must be `read` or `write`, got: {marker:?}"
            );
        }
    }
    assert_only_keys(event, &["r"], "NIP-65 relay list");
}

/// NIP-65: when NMP publishes a kind:10002 relay list, it must carry one `r`
/// tag per relay (optionally `read`/`write`-marked) and nothing else.
///
/// This drives the generic publish path (`publish_unsigned_event`) with a
/// kind:10002 carrying NIP-65 `r` tags — the same kind, tags and signing path
/// `create_account`'s relay-list builder feeds into. It pins that NMP's
/// publish pipeline emits a kind:10002 with its `r`-tag structure intact.
#[test]
fn kind10002_relay_list_carries_r_tags() {
    let mut h = signed_harness();
    let r_tags = vec![
        vec![
            "r".to_string(),
            "wss://nip65-write.test".to_string(),
            "write".to_string(),
        ],
        vec![
            "r".to_string(),
            "wss://nip65-read.test".to_string(),
            "read".to_string(),
        ],
        // An unmarked `r` tag — NIP-65 reads this as both read and write.
        vec!["r".to_string(), "wss://nip65-both.test".to_string()],
    ];
    let event = h.emit_unsigned(10002, r_tags, "");
    assert_nip65_relay_list(
        &event,
        &[
            "wss://nip65-write.test",
            "wss://nip65-read.test",
            "wss://nip65-both.test",
        ],
    );
}
