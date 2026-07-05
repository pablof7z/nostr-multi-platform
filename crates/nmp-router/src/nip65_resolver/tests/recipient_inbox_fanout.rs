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

/// chirp#119: a kind:1 reply's `p`-tagged parent author may have NO cached
/// kind:10002 at all (never seen one, cache miss, cold start on the reader's
/// side, etc). `lookup_kind10002` returns `None` for that recipient — the
/// resolver must fail OPEN to the author's own already-resolved write relays,
/// never fail-closed / return an empty set just because ONE recipient's
/// inbox could not be resolved. Regression test locking the resolver's
/// step-4 recipient-inbox fan-out as strictly additive: only `AUTHOR_HEX`'s
/// kind:10002 is seeded here — `RECIPIENT_HEX` has no cache entry whatsoever.
#[test]
fn nip65_resolver_still_routes_to_author_write_when_recipient_has_no_kind10002() {
    let cache = Arc::new(InMemoryMailboxCache::new());
    seed_relay(&cache, AUTHOR_HEX, AUTHOR_WRITE_RELAY, "write");
    // Deliberately no `seed_relay`/`seed_kind10002` call for `RECIPIENT_HEX` —
    // the reply's parent author has no cached kind:10002 whatsoever.
    let resolver = mk_resolver(&cache);
    let out = resolver.resolve(
        AUTHOR_HEX,
        &[RECIPIENT_HEX.to_string()],
        &PublishTarget::Auto,
        1,
        &no_block(),
    );
    let urls = urls_of(&out);
    assert!(
        urls.contains(AUTHOR_WRITE_RELAY),
        "the author's own write relay must still be routed even when the \
         p-tagged recipient's inbox cannot be resolved at all (chirp#119) — \
         got {out:?}"
    );
    assert!(
        !out.is_empty(),
        "an unresolvable recipient inbox must never fail-close the whole \
         publish to an empty relay set"
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
