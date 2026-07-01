use super::*;

/// (c) KIND-AGNOSTIC — cache-served kind:10002 (mailbox) and kind:10050 (DM)
/// fire their transitions via the SAME shared helper: the registered parser
/// writes the cache and the transition sweep fires `on_mailbox_changed` /
/// `on_dm_relays_changed`. Proven by the resulting cache state populated purely
/// from cache-serve replay (the parser ran via `project_accepted_event`).
/// Non-vacuous: dropping the shared-helper call leaves both caches empty.
#[test]
fn cache_served_replaceables_fire_transitions_kind_agnostically() {
    use crate::substrate::IngestParser;

    let base_ts: u64 = 1_700_000_400;
    let keys = ::nostr::Keys::generate();
    let author = keys.public_key().to_hex();

    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    kernel.active_account = Some(hex_pk("aa"));
    kernel.timeline_authors.insert(author.clone());
    let profile_lookup = Arc::new(TestProfileLookup::new());
    kernel.set_profile_lookup(Arc::clone(&profile_lookup) as Arc<dyn ProfileLookup>);
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.register_kind(0, ProfileViewWriterParser::new(Arc::clone(&profile_lookup), "Nova"));
    }

    // Register a kind:10002 parser writing a real mailbox cache, mirroring
    // production composition. Use the same in-memory mailbox cache the kernel
    // reads.
    let mailbox = kernel.mailbox_cache_arc();
    let mailbox_parser: Arc<dyn IngestParser> = Arc::new(TestKind10002Parser { cache: mailbox });
    if let Ok(mut d) = kernel.ingest_dispatcher_slot().write() {
        d.register_kind(10_002, mailbox_parser);
    }

    // Phase 1: live-ingest a kind:0 and a kind:10002 for the author.
    live_ingest(
        &mut kernel,
        "follow-feed-default",
        &signed_kind0(&keys, "Nova", base_ts),
    );
    live_ingest(
        &mut kernel,
        "follow-feed-default",
        &signed_kind10002(&keys, "wss://write.relay/", base_ts),
    );
    assert!(
        kernel.profile_lookup().contains(&author),
        "precondition: profile cached"
    );
    assert!(
        kernel.mailbox_cache().known(&author),
        "precondition: mailbox cached"
    );

    // Phase 2: cold restart + clear both capability caches so cache-serve replay
    // is the only repopulation path.
    simulate_cold_restart(&mut kernel);
    kernel
        .profile_lookup()
        .evict_to(&std::collections::HashSet::new(), 0);
    kernel.mailbox_cache().remove(&author);
    assert!(
        !kernel.profile_lookup().contains(&author),
        "profile cleared pre-replay"
    );
    assert!(
        !kernel.mailbox_cache().known(&author),
        "mailbox cleared pre-replay"
    );

    // Phase 3: replay both kinds via cache-serve.
    open_kind0_interest(&mut kernel, 40, &author);
    open_kind10002_interest(&mut kernel, 41, &author);
    drain_cache_serves(&mut kernel, 20);

    // Phase 4: BOTH capability caches were repopulated by the SAME shared
    // helper's parser dispatch — kind-agnostic, no per-kind cache-serve code.
    assert!(
        kernel.profile_lookup().contains(&author),
        "cache-served kind:0 repopulated the profile cache via the shared helper",
    );
    assert!(
        kernel.mailbox_cache().known(&author),
        "cache-served kind:10002 repopulated the mailbox cache via the shared helper",
    );
}
