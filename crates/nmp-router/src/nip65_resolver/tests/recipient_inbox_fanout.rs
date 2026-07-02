//! Code path 4 — recipient-inbox fan-out from `#p` tags: read relays for
//! recipients are unioned in below the fan-out threshold, suppressed at/above
//! it, and carry the `RelaySelectionReason::RecipientInbox { pubkey }`
//! variant.

use std::sync::Arc;

use super::fixtures::{
    find_reason, mk_resolver, no_block, seed_kind10002, seed_relay, threshold_recipients, urls_of,
    AUTHOR_HEX, AUTHOR_WRITE_RELAY, RECIPIENT_HEX, RECIPIENT_READ_RELAY,
};
use crate::InMemoryMailboxCache;
use nmp_core::publish::{OutboxResolver, PublishTarget, RelaySelectionReason};

#[test]
fn nip65_resolver_unions_recipient_reads_for_p_tags() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_kind10002(
        &cache,
        AUTHOR_HEX,
        vec![vec![
            "r".into(),
            "wss://author-write.example".into(),
            "write".into(),
        ]],
    );
    seed_kind10002(
        &cache,
        RECIPIENT_HEX,
        vec![vec![
            "r".into(),
            "wss://recipient-read.example".into(),
            "read".into(),
        ]],
    );
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(urls.contains(AUTHOR_WRITE_RELAY));
    assert!(urls.contains(RECIPIENT_READ_RELAY));
}

#[test]
fn nip65_resolver_skips_recipient_reads_at_p_tag_threshold() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_relay(&cache, AUTHOR_HEX, AUTHOR_WRITE_RELAY, "write");
    seed_relay(&cache, RECIPIENT_HEX, RECIPIENT_READ_RELAY, "read");
    let recipients = threshold_recipients();

    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &recipients,
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let urls = urls_of(&out);

    assert!(urls.contains(AUTHOR_WRITE_RELAY));
    assert!(
        !urls.contains(RECIPIENT_READ_RELAY),
        "15+ distinct p-tagged pubkeys must not fan out to recipient inbox relays"
    );
}

/// The raw hex pubkey rides on the variant verbatim and is emitted unchanged
/// by the kernel projection — D6 forbids backend projections from calling
/// `display::*` abbreviation helpers; the shell renders its own short form.
#[test]
fn resolve_returns_inbox_relay_reason_for_p_tags() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_relay(&cache, AUTHOR_HEX, AUTHOR_WRITE_RELAY, "write");
    seed_relay(&cache, RECIPIENT_HEX, RECIPIENT_READ_RELAY, "read");
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let reason =
        find_reason(&out, RECIPIENT_READ_RELAY).expect("recipient read relay must be present");
    match reason {
        RelaySelectionReason::RecipientInbox { pubkey } => {
            assert_eq!(
                pubkey, RECIPIENT_HEX,
                "recipient pubkey rides verbatim on the variant; the kernel \
                 projection emits the raw hex (D6 — abbreviation lives in the \
                 shell, never in projections)"
            );
        }
        other => panic!("expected RecipientInbox, got {other:?}"),
    }
}
